//! Lenient parsing and endpoint-based merging of external servers.

use std::collections::HashSet;

use crate::menu::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, SavedServer, split_address};

/// The vanilla file that stores external server entries.
pub const FILE_NAME: &str = "external_servers.txt";

/// Parsed player entries and the number of lines that could not be used.
#[derive(Clone, Debug, Default)]
pub struct ParsedServers {
    pub entries: Vec<SavedServer>,
    pub skipped: usize,
}

/// Parses colon-separated id, name, host, port and optional metadata fields.
pub fn parse(bytes: &[u8]) -> ParsedServers {
    let mut parsed = ParsedServers::default();
    for bytes in bytes.split(|byte| *byte == b'\n') {
        let Ok(line) = std::str::from_utf8(bytes) else {
            parsed.skipped += 1;
            continue;
        };
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        match entry(line) {
            Some(server) => parsed.entries.push(server),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

/// Validates one row while preserving a bracketed IPv6 host.
fn entry(line: &str) -> Option<SavedServer> {
    let (id, remainder) = line.split_once(':')?;
    id.parse::<u64>().ok()?;
    let (name, remainder) = remainder.split_once(':')?;
    let name = name.trim();
    let (host, remainder) = if let Some(bracketed) = remainder.strip_prefix('[') {
        let (host, tail) = bracketed.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        (format!("[{host}]"), tail.strip_prefix(':')?)
    } else {
        let (host, tail) = remainder.split_once(':')?;
        (host.trim().to_owned(), tail)
    };
    let port = remainder.split(':').next()?.trim().parse::<u16>().ok()?;
    if port == 0
        || host.is_empty()
        || host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'))
        || name.is_empty()
        || name.len() > MAX_SERVER_NAME_BYTES
        || name.chars().any(char::is_control)
    {
        return None;
    }
    let address = format!("{host}:{port}");
    (address.len() <= MAX_SERVER_ADDRESS_BYTES).then_some(SavedServer {
        name: name.to_owned(),
        address,
        favorite: false,
        last_joined_unix: 0,
    })
}

/// Adds only new endpoints, retaining existing names, flags and order.
pub fn merge(existing: &mut Vec<SavedServer>, incoming: &[SavedServer]) -> usize {
    let mut endpoints = existing
        .iter()
        .map(|server| endpoint(&server.address))
        .collect::<HashSet<_>>();
    let mut skipped = 0;
    for server in incoming {
        if endpoints.insert(endpoint(&server.address)) {
            existing.push(server.clone());
        } else {
            skipped += 1;
        }
    }
    skipped
}

/// Normalizes DNS case, trailing dots, numeric ports and IPv6 spelling for deduplication.
fn endpoint(address: &str) -> (String, String) {
    let (host, port) = split_address(address);
    let literal = host.trim_matches(['[', ']']);
    let host = literal.parse::<std::net::IpAddr>().map_or_else(
        |_| literal.trim_end_matches('.').to_ascii_lowercase(),
        |address| address.to_string(),
    );
    let port = port
        .parse::<u16>()
        .map_or(port.clone(), |port| port.to_string());
    (host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_rows_do_not_hide_valid_servers() {
        let rows = parse(b"1:Test:example.test:19132:0\r\ninvalid\n2:IPv6:[2001:db8::1]:19133:0\n3:bad:host:0:0\n4:bad:host:no:0\n");
        assert_eq!(rows.entries.len(), 2);
        assert_eq!(rows.skipped, 3);
        assert_eq!(rows.entries[1].address, "[2001:db8::1]:19133");
    }

    #[test]
    fn merging_preserves_player_entries_and_normalizes_addresses() {
        let mut existing =
            parse(b"1:Original:EXAMPLE.test.:19132:0\n2:IPv6:[2001:db8::1]:19132:0").entries;
        existing[0].favorite = true;
        let rows = parse(b"3:Duplicate:example.test:19132:0\n4:Duplicate:[2001:db8:0:0:0:0:0:1]:19132:0\n5:New:another.test:19132:0");
        assert_eq!(merge(&mut existing, &rows.entries), 2);
        assert_eq!(existing.len(), 3);
        assert_eq!(existing[0].name, "Original");
        assert!(existing[0].favorite);
        assert_eq!(merge(&mut existing, &rows.entries), 3);
    }

    /// Parsing keeps every valid row, including entries beyond the former list quota.
    #[test]
    fn large_server_lists_keep_their_last_entry() {
        let text = (0..600)
            .map(|i| format!("{i}:Server {i}:server-{i}.example:19132\n"))
            .collect::<String>();
        let parsed = parse(text.as_bytes());
        assert_eq!(parsed.entries.len(), 600);
        assert_eq!(parsed.skipped, 0);
        let mut merged = Vec::new();
        assert_eq!(merge(&mut merged, &parsed.entries), 0);
        assert_eq!(merged, parsed.entries);
    }
}
