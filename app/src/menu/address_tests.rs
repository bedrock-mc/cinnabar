//! Saved server addresses preserve IPv6 literals and explicit ports.
use super::*;

#[test]
fn bare_ipv6_hosts_keep_the_default_port() {
    for host in ["::1", "2001:db8::1234", "::ffff:192.0.2.1"] {
        assert_eq!(split_address(host), (host.into(), DEFAULT_PORT.into()));
        assert_eq!(
            split_address(&format!("[{host}]")),
            (host.into(), DEFAULT_PORT.into())
        );
        assert_eq!(
            split_address(&format!("[{host}]:1234")),
            (host.into(), "1234".into())
        );
    }
}
