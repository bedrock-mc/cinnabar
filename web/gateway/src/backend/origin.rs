//! The Go relay's exact-origin policy, without DNS or browser-style URL normalization.
//! Host parsing follows Go's BSD-licensed net/url rules; see THIRD_PARTY_NOTICES.md.

use std::net::Ipv6Addr;

/// Requires a usable HTTP origin before comparing its original spelling or decoded authority.
pub(super) fn allowed(origin: &str, host: &str, configured: &[String]) -> bool {
    if origin.is_empty() {
        return true;
    }
    let Some(authority) = authority(origin) else {
        return false;
    };
    if configured.is_empty() {
        authority == host
    } else {
        configured.iter().any(|allowed| allowed == origin)
    }
}

/// Preserves net/url's empty query/fragment and host spelling; neither approves an upstream target.
fn authority(origin: &str) -> Option<String> {
    let (without_fragment, fragment) = origin.split_once('#').unwrap_or((origin, ""));
    let (without_query, query) = without_fragment
        .split_once('?')
        .unwrap_or((without_fragment, ""));
    if !fragment.is_empty() || !query.is_empty() {
        return None;
    }
    let (scheme, host) = without_query.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || host.is_empty()
        || host.contains(['/', '@'])
    {
        return None;
    }
    let decoded = protocol::relay_url_host(host)?;
    if let Some(literal) = decoded.strip_prefix('[') {
        let (literal, _) = literal.rsplit_once(']')?;
        let address = match literal.split_once('%') {
            Some((address, zone)) if !zone.is_empty() => address,
            Some(_) => return None,
            None => literal,
        };
        address.parse::<Ipv6Addr>().ok()?;
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        origin: String,
        host: String,
        configured: Vec<String>,
        accepted: bool,
    }

    /// The same corpus is checked against the actual Go relay, including case/zone/empty-tail edges.
    #[test]
    fn origin_policy_matches_the_shared_go_cases() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../tests/fixtures/origins.json")).unwrap();
        for case in cases {
            assert_eq!(
                allowed(&case.origin, &case.host, &case.configured),
                case.accepted,
                "{}",
                case.name
            );
        }
    }
}
