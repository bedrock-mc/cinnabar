//! Bedrock's application payload inside a RakNet unconnected pong.

use std::io;

/// The advertised fields consumed by the server list, independent of connection admission.
#[derive(Debug, PartialEq, Eq)]
pub struct ServerListStatus {
    /// Original server name/MOTD, before presentation trimming.
    pub name: String,
    /// Signed advertised population; presentation may clamp negative values.
    pub players: i64,
    /// Signed advertised capacity; this is not a connection limit.
    pub max_players: i64,
}

impl ServerListStatus {
    /// Parses the fields used by the pinned Gophertunnel ParsePong owner.
    /// Unknown version/mode fields do not invalidate otherwise usable status.
    ///
    /// # Errors
    /// Rejects missing fields or nonintegral/overflowed population values.
    pub fn parse_pong(payload: &[u8]) -> io::Result<Self> {
        // The existing Go splitPong discards backslashes before splitting every semicolon.
        let text = String::from_utf8_lossy(payload).replace('\\', "");
        let fields: Vec<_> = text.splitn(8, ';').collect();
        if fields.len() < 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid pong data",
            ));
        }
        let number = |value: &str| {
            value
                .parse::<i64>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid pong population"))
        };
        Ok(Self {
            name: fields[1].to_owned(),
            players: number(fields[4])?,
            max_players: number(fields[5])?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Used-field errors fail while unknown modes/versions and signed counts preserve status.
    #[test]
    fn pong_projection_preserves_original_lenient_unused_fields() {
        let status = ServerListStatus::parse_pong(
            b"future;A\\ Server;unknown;future;+12;-1;id;sub;Unknown;",
        )
        .unwrap();
        assert_eq!(
            status,
            ServerListStatus {
                name: "A Server".into(),
                players: 12,
                max_players: -1
            }
        );
        for payload in [
            "MCPE;short",
            "MCPE;A;900;1;x;100;1;sub",
            "MCPE;A;900;1;1;x;1;sub",
            "MCPE;A;900;1;9223372036854775808;1;1;sub",
        ] {
            assert!(ServerListStatus::parse_pong(payload.as_bytes()).is_err());
        }
    }
}
