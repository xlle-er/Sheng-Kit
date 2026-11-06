//! `system` proxy mode: one resolution per destination host, shared by HTTP and WebSocket.

use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::transport::system_proxy::{choose, resolve, SystemProxySettings};

fn settings() -> SystemProxySettings {
    SystemProxySettings {
        https: Some(("proxy.example.net".into(), 8443)),
        socks: Some(("socks.example.net".into(), 1080)),
        exceptions: vec!["*.local".into(), "intranet.example.com".into()],
        exclude_simple_hostnames: true,
    }
}

fn url_of(choice: ProxyChoice) -> String {
    match choice {
        ProxyChoice::Http(url) | ProxyChoice::Socks5(url) => url.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn secure_web_proxy_is_preferred_over_socks() {
    let choice = choose(&settings(), "api.example.com");

    assert!(matches!(choice, ProxyChoice::Http(_)));
    assert_eq!(url_of(choice), "http://proxy.example.net:8443/");
}

#[test]
fn socks_is_used_when_only_socks_is_enabled() {
    let only_socks = SystemProxySettings {
        https: None,
        ..settings()
    };

    let choice = choose(&only_socks, "api.example.com");

    assert!(matches!(choice, ProxyChoice::Socks5(_)));
}

#[test]
fn exceptions_simple_names_and_loopback_bypass_the_proxy() {
    for host in [
        "printer.local",
        "intranet.example.com",
        "a.intranet.example.com",
        "myhost",
        "localhost",
        "127.0.0.1",
        "[::1]",
    ] {
        assert_eq!(choose(&settings(), host), ProxyChoice::Direct, "{host}");
    }
    assert!(matches!(
        choose(&settings(), "other-intranet.example.com"),
        ProxyChoice::Http(_)
    ));
}

#[test]
fn no_enabled_proxy_means_direct() {
    let none = SystemProxySettings::default();

    assert_eq!(choose(&none, "api.example.com"), ProxyChoice::Direct);
}

#[test]
fn explicit_choices_are_not_looked_up() {
    assert_eq!(
        resolve(&ProxyChoice::Direct, "api.example.com"),
        ProxyChoice::Direct
    );
    assert_eq!(
        resolve(&ProxyChoice::System, "localhost"),
        ProxyChoice::Direct
    );
}
