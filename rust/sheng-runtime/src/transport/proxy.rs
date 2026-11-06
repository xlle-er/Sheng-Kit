//! TCP connections through no proxy, an HTTP `CONNECT` proxy or a SOCKS5 proxy.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

use crate::config::endpoint::ProxyChoice;

const MAX_PROXY_RESPONSE: usize = 8 * 1024;
const SOCKS_VERSION: u8 = 5;
const SOCKS_NO_AUTH: u8 = 0;
const SOCKS_USER_PASS: u8 = 2;
const SOCKS_CONNECT: u8 = 1;
const SOCKS_DOMAIN: u8 = 3;

fn proxy_error(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionRefused, reason)
}

fn proxy_address(url: &Url, default_port: u16) -> io::Result<(String, u16)> {
    let host = url
        .host_str()
        .ok_or_else(|| proxy_error("proxy has no host"))?;
    Ok((
        host.trim_matches(['[', ']']).to_owned(),
        url.port().unwrap_or(default_port),
    ))
}

/// Opens a TCP connection to `host:port`, tunnelled through `proxy` when one is set.
pub async fn open_tcp(host: &str, port: u16, proxy: &ProxyChoice) -> io::Result<TcpStream> {
    match super::system_proxy::resolve(proxy, host) {
        ProxyChoice::Http(url) => {
            let (proxy_host, proxy_port) = proxy_address(&url, 80)?;
            let mut stream = TcpStream::connect((proxy_host.as_str(), proxy_port)).await?;
            http_connect(&mut stream, host, port, &url).await?;
            Ok(stream)
        }
        ProxyChoice::Socks5(url) => {
            let (proxy_host, proxy_port) = proxy_address(&url, 1080)?;
            let mut stream = TcpStream::connect((proxy_host.as_str(), proxy_port)).await?;
            socks5_connect(&mut stream, host, port, &url).await?;
            Ok(stream)
        }
        ProxyChoice::Direct | ProxyChoice::System => TcpStream::connect((host, port)).await,
    }
}

fn credentials(url: &Url) -> Option<(String, String)> {
    if url.username().is_empty() {
        return None;
    }
    let decode = |s: &str| {
        url::form_urlencoded::parse(format!("v={s}").as_bytes())
            .next()
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    };
    Some((decode(url.username()), decode(url.password().unwrap_or(""))))
}

async fn http_connect(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    proxy: &Url,
) -> io::Result<()> {
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some((user, password)) = credentials(proxy) {
        let token = base64(format!("{user}:{password}").as_bytes());
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    let head = read_head(stream).await?;
    let status_ok = head
        .split(|b| *b == b' ')
        .nth(1)
        .is_some_and(|code| code.starts_with(b"2"));
    if !status_ok {
        return Err(proxy_error("proxy refused the tunnel"));
    }
    Ok(())
}

/// Reads up to the blank line ending an HTTP response head, byte by byte so nothing of the
/// tunnelled stream is consumed.
async fn read_head(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_PROXY_RESPONSE {
            return Err(proxy_error("proxy response too long"));
        }
        head.push(stream.read_u8().await?);
    }
    Ok(head)
}

async fn socks5_connect(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    proxy: &Url,
) -> io::Result<()> {
    let auth = credentials(proxy);
    let method = if auth.is_some() {
        SOCKS_USER_PASS
    } else {
        SOCKS_NO_AUTH
    };
    stream.write_all(&[SOCKS_VERSION, 1, method]).await?;
    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice).await?;
    if choice != [SOCKS_VERSION, method] {
        return Err(proxy_error("socks proxy refused the authentication method"));
    }
    if let Some((user, password)) = auth {
        socks5_authenticate(stream, &user, &password).await?;
    }
    let name = host.trim_matches(['[', ']']).as_bytes();
    let length = u8::try_from(name.len()).map_err(|_| proxy_error("host name too long"))?;
    let mut request = vec![SOCKS_VERSION, SOCKS_CONNECT, 0, SOCKS_DOMAIN, length];
    request.extend_from_slice(name);
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await?;
    let mut reply = [0u8; 4];
    stream.read_exact(&mut reply).await?;
    if reply[0] != SOCKS_VERSION || reply[1] != 0 {
        return Err(proxy_error("socks proxy refused the connection"));
    }
    let address_len = match reply[3] {
        1 => 4,
        4 => 16,
        SOCKS_DOMAIN => usize::from(stream.read_u8().await?),
        _ => return Err(proxy_error("socks proxy sent an unknown address type")),
    };
    let mut rest = vec![0u8; address_len + 2];
    stream.read_exact(&mut rest).await?;
    Ok(())
}

async fn socks5_authenticate(stream: &mut TcpStream, user: &str, password: &str) -> io::Result<()> {
    let too_long = || proxy_error("socks credentials too long");
    let user_len = u8::try_from(user.len()).map_err(|_| too_long())?;
    let password_len = u8::try_from(password.len()).map_err(|_| too_long())?;
    let mut message = vec![1, user_len];
    message.extend_from_slice(user.as_bytes());
    message.push(password_len);
    message.extend_from_slice(password.as_bytes());
    stream.write_all(&message).await?;
    let mut status = [0u8; 2];
    stream.read_exact(&mut status).await?;
    if status[1] != 0 {
        return Err(proxy_error("socks proxy rejected the credentials"));
    }
    Ok(())
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding, for the proxy `Basic` header.
pub fn base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(BASE64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
