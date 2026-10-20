//! Incoming address syntax is metadata only; DNS and OS service resolution are deliberately omitted.
//! URL component admission follows the actual Go net/url consumer, without browser normalization.

use crate::login::client_data::relay_url::relay_url_host;
use std::net::Ipv6Addr;

/// Checks shape only; the actual selected target and socket supply the final signed ServerAddress.
pub(super) fn valid(address: &str) -> bool {
    if address.contains("://") {
        return echoed_url_port(address);
    }
    if !address.contains(':') {
        // The original consumer ignores resolution errors when no colon exists.
        return true;
    }
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    if port.contains(['[', ']']) {
        return false;
    }
    if let Some(host) = host.strip_prefix('[') {
        let Some(host) = host.strip_suffix(']') else {
            return false;
        };
        if host.contains(['[', ']']) {
            return false;
        }
    } else if host.contains([':', '[', ']']) {
        return false;
    }
    port_syntax(port)
}

/// Preserves empty/signed numeric zero; named service lookup is part of the explicit resolution gap.
fn port_syntax(port: &str) -> bool {
    let (digits, negative) = if let Some(value) = port.strip_prefix('-') {
        (value, true)
    } else {
        (port.strip_prefix('+').unwrap_or(port), false)
    };
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    if digits.is_empty() {
        return true;
    }
    digits
        .parse::<u16>()
        .is_ok_and(|value| !negative || value == 0)
}

/// Go's title URL form echoes the original textual port after the complete parsed URL.
fn echoed_url_port(address: &str) -> bool {
    let Some((url, challenge)) = address.rsplit_once(':') else {
        return false;
    };
    let (url, fragment) = url.split_once('#').unwrap_or((url, ""));
    if url.bytes().any(|byte| byte < b' ' || byte == 0x7f) || !escaped(fragment) {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let (rest, _) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !escaped(path) {
        return false;
    }
    let host = if let Some((user, host)) = authority.rsplit_once('@') {
        if !user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&byte))
            || !escaped(user)
        {
            return false;
        }
        host
    } else {
        authority
    };
    let Some(host) = relay_url_host(host).filter(|host| !host.is_empty()) else {
        return false;
    };
    let port = if let Some(literal) = host.strip_prefix('[') {
        let Some((literal, port)) = literal.rsplit_once(']') else {
            return false;
        };
        let ip = match literal.split_once('%') {
            Some((ip, zone)) if !zone.is_empty() => ip,
            Some(_) => return false,
            None => literal,
        };
        if ip.parse::<Ipv6Addr>().is_err() {
            return false;
        }
        port.strip_prefix(':')
    } else {
        host.rsplit_once(':').map(|(_, port)| port)
    };
    port.is_some_and(|port| !port.is_empty() && port == challenge)
}

/// Path, userinfo and fragment decoding checks escapes; RawQuery deliberately bypasses this check.
fn escaped(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && !(bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                && bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit()))
        {
            return false;
        }
    }
    true
}
