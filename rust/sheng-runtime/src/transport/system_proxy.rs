//! `networkProxy.mode:"system"`: the macOS system proxy settings decide, per destination host,
//! for HTTP and WebSocket alike. The standard environment variables are only a fallback when
//! the system defines no proxy.

use std::net::IpAddr;

use url::Url;

use crate::config::endpoint::ProxyChoice;

/// Proxy settings read from the system, independent of the platform API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProxySettings {
    /// Enabled secure web proxy `(host, port)`.
    pub https: Option<(String, u16)>,
    /// Enabled SOCKS proxy `(host, port)`.
    pub socks: Option<(String, u16)>,
    /// Hosts and domains that bypass the proxy.
    pub exceptions: Vec<String>,
    /// Single-label host names bypass the proxy.
    pub exclude_simple_hostnames: bool,
}

const DEFAULT_HTTP_PROXY_PORT: u16 = 80;
const DEFAULT_SOCKS_PORT: u16 = 1080;

impl SystemProxySettings {
    fn has_proxy(&self) -> bool {
        self.https.is_some() || self.socks.is_some()
    }

    fn bypasses(&self, host: &str) -> bool {
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if self.exclude_simple_hostnames && !host.contains('.') && !host.contains(':') {
            return true;
        }
        self.exceptions
            .iter()
            .any(|entry| exception_matches(entry, &host))
    }
}

/// `example.com`, `.example.com` and `*.example.com` cover the domain and its subdomains;
/// other entries must match exactly. Address ranges are not interpreted.
fn exception_matches(entry: &str, host: &str) -> bool {
    let entry = entry.trim().to_ascii_lowercase();
    let domain = entry
        .strip_prefix("*.")
        .or_else(|| entry.strip_prefix('.'))
        .unwrap_or(&entry);
    if domain.is_empty() {
        return false;
    }
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn proxy_url(scheme: &str, (host, port): &(String, u16)) -> Option<Url> {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    Url::parse(&format!("{scheme}://{host}:{port}")).ok()
}

/// Chooses the proxy for `host` from the system settings. Loopback is always direct.
pub fn choose(settings: &SystemProxySettings, host: &str) -> ProxyChoice {
    if is_loopback(host) || settings.bypasses(host) {
        return ProxyChoice::Direct;
    }
    let https = settings.https.as_ref().and_then(|p| proxy_url("http", p));
    let socks = settings.socks.as_ref().and_then(|p| proxy_url("socks5", p));
    match (https, socks) {
        (Some(url), _) => ProxyChoice::Http(url),
        (None, Some(url)) => ProxyChoice::Socks5(url),
        (None, None) => ProxyChoice::Direct,
    }
}

/// Fallback: `HTTPS_PROXY`/`ALL_PROXY` from the environment.
pub fn environment_proxy(host: &str) -> ProxyChoice {
    if is_loopback(host) {
        return ProxyChoice::Direct;
    }
    let configured = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()));
    let Some(url) = configured.and_then(|raw| Url::parse(raw.trim()).ok()) else {
        return ProxyChoice::Direct;
    };
    match url.scheme() {
        "http" => ProxyChoice::Http(url),
        "socks5" | "socks5h" => ProxyChoice::Socks5(url),
        _ => ProxyChoice::Direct,
    }
}

/// Resolves `system` mode for one destination host; never returns `ProxyChoice::System`.
pub fn resolve_system(host: &str) -> ProxyChoice {
    match read_system_settings().filter(SystemProxySettings::has_proxy) {
        Some(settings) => choose(&settings, host),
        None => environment_proxy(host),
    }
}

/// Resolves any proxy choice for `host`; `system` is looked up now.
pub fn resolve(choice: &ProxyChoice, host: &str) -> ProxyChoice {
    match choice {
        ProxyChoice::System => resolve_system(host),
        other => other.clone(),
    }
}

#[cfg(target_os = "macos")]
fn read_system_settings() -> Option<SystemProxySettings> {
    macos::read()
}

#[cfg(not(target_os = "macos"))]
fn read_system_settings() -> Option<SystemProxySettings> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::base::{CFType, TCFType};
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesExcludeSimpleHostnames,
        kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort, kSCPropNetProxiesHTTPSProxy,
        kSCPropNetProxiesSOCKSEnable, kSCPropNetProxiesSOCKSPort, kSCPropNetProxiesSOCKSProxy,
    };

    use super::{SystemProxySettings, DEFAULT_HTTP_PROXY_PORT, DEFAULT_SOCKS_PORT};

    type Proxies = CFDictionary<CFString, CFType>;

    fn number(map: &Proxies, key: CFStringRef) -> Option<i64> {
        map.find(key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i64())
    }

    fn proxy(
        map: &Proxies,
        enable: CFStringRef,
        host: CFStringRef,
        port: CFStringRef,
        default_port: u16,
    ) -> Option<(String, u16)> {
        if number(map, enable) != Some(1) {
            return None;
        }
        let host = map
            .find(host)
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty())?;
        let port = number(map, port)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(default_port);
        Some((host.trim().to_owned(), port))
    }

    fn exceptions(map: &Proxies) -> Vec<String> {
        // SAFETY: the key is a constant CFString exported by the SystemConfiguration framework.
        let key = unsafe { kSCPropNetProxiesExceptionsList };
        let Some(list) = map.find(key).and_then(|value| value.downcast::<CFArray>()) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|item| {
                // SAFETY: array items are CF objects owned by the array for this borrow.
                let item = unsafe { CFType::wrap_under_get_rule(*item) };
                item.downcast::<CFString>().map(|s| s.to_string())
            })
            .collect()
    }

    pub(super) fn read() -> Option<SystemProxySettings> {
        let store = SCDynamicStoreBuilder::new("sheng-engine").build()?;
        let map = store.get_proxies()?;
        // SAFETY: the keys are constant CFStrings exported by the SystemConfiguration framework.
        let settings = unsafe {
            SystemProxySettings {
                https: proxy(
                    &map,
                    kSCPropNetProxiesHTTPSEnable,
                    kSCPropNetProxiesHTTPSProxy,
                    kSCPropNetProxiesHTTPSPort,
                    DEFAULT_HTTP_PROXY_PORT,
                ),
                socks: proxy(
                    &map,
                    kSCPropNetProxiesSOCKSEnable,
                    kSCPropNetProxiesSOCKSProxy,
                    kSCPropNetProxiesSOCKSPort,
                    DEFAULT_SOCKS_PORT,
                ),
                exceptions: Vec::new(),
                exclude_simple_hostnames: number(&map, kSCPropNetProxiesExcludeSimpleHostnames)
                    == Some(1),
            }
        };
        Some(SystemProxySettings {
            exceptions: exceptions(&map),
            ..settings
        })
    }
}
