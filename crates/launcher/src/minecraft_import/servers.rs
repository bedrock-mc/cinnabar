//! Bounded, lenient parsing and endpoint-based merging of external servers.

use crate::menu::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, SavedServer, split_address};

/// Parsed player entries and the number of lines that could not be used.
#[derive(Clone, Debug, Default)]
pub struct ParsedServers {
    pub entries: Vec<SavedServer>,
    pub skipped: usize,
}

/// Parses colon-separated id, name, host, port and optional metadata fields.
pub fn parse(bytes: &[u8], limit: usize) -> ParsedServers {
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
        if parsed.entries.len() >= limit {
            parsed.skipped += 1;
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

/// Adds only new endpoints, retaining existing names, flags, order and capacity.
pub fn merge(existing: &mut Vec<SavedServer>, incoming: &[SavedServer], limit: usize) -> usize {
    let mut skipped = 0;
    for server in incoming {
        if existing.len() >= limit
            || existing
                .iter()
                .any(|old| endpoint(&old.address) == endpoint(&server.address))
        {
            skipped += 1;
        } else {
            existing.push(server.clone());
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
        let rows = parse(b"1:Test:example.test:19132:0\r\ninvalid\n2:IPv6:[2001:db8::1]:19133:0\n3:bad:host:0:0\n4:bad:host:no:0\n", 256);
        assert_eq!(rows.entries.len(), 2);
        assert_eq!(rows.skipped, 3);
        assert_eq!(rows.entries[1].address, "[2001:db8::1]:19133");
    }

    #[test]
    fn merging_preserves_player_entries_and_normalizes_addresses() {
        let mut existing = parse(
            b"1:Original:EXAMPLE.test.:19132:0\n2:IPv6:[2001:db8::1]:19132:0",
            256,
        )
        .entries;
        existing[0].favorite = true;
        let rows = parse(b"3:Duplicate:example.test:19132:0\n4:Duplicate:[2001:db8:0:0:0:0:0:1]:19132:0\n5:New:another.test:19132:0", 256);
        assert_eq!(merge(&mut existing, &rows.entries, 256), 2);
        assert_eq!(existing.len(), 3);
        assert_eq!(existing[0].name, "Original");
        assert!(existing[0].favorite);
        assert_eq!(merge(&mut existing, &rows.entries, 256), 3);
    }

    #[test]
    fn row_and_merge_limits_are_enforced() {
        let parsed = parse(b"1:A:a.test:19132\n2:B:b.test:19132", 1);
        assert_eq!(parsed.skipped, 1);
        let mut existing = parsed.entries.clone();
        assert_eq!(merge(&mut existing, &parsed.entries, 1), 1);
    }
}
