//! The packet-mod benchmark capture: repeated records `[kind u8][len u32 LE][body]`.
//!
//! Kind 0 starts a session; its body is a u32-LE length and the identity JSON
//! (`display_name`, `xuid`, `uuid`), a u32-LE length and the login client-data JSON, then the
//! startup batch body. Kinds 1 and 2 carry a from-client and a from-server batch body; kind 3 is
//! one tick and has an empty body.

use super::{Direction, SessionIdentity};
use anyhow::{Context, Result, bail};

/// One replayed call.
#[derive(Debug, PartialEq, Eq)]
pub enum Record {
    Start {
        identity: SessionIdentity,
        client_data: String,
        startup: Vec<u8>,
    },
    Batch {
        direction: Direction,
        body: Vec<u8>,
    },
    Tick,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityJson {
    display_name: String,
    xuid: String,
    uuid: String,
}

/// Parses a whole capture, refusing truncated records and unknown kinds.
pub fn parse(mut bytes: &[u8]) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    while let Some((&kind, rest)) = bytes.split_first() {
        let (body, rest) = split_prefixed(rest).context("truncated capture record")?;
        bytes = rest;
        records.push(match kind {
            0 => start(body)?,
            1 | 2 => Record::Batch {
                direction: if kind == 1 {
                    Direction::FromClient
                } else {
                    Direction::FromServer
                },
                body: body.to_vec(),
            },
            3 => Record::Tick,
            _ => bail!("unknown capture record kind {kind}"),
        });
    }
    Ok(records)
}

/// Decodes a start record's identity, client data and startup batch body.
fn start(body: &[u8]) -> Result<Record> {
    let (identity, rest) = split_prefixed(body).context("truncated start identity")?;
    let (client_data, startup) = split_prefixed(rest).context("truncated start client data")?;
    let identity: IdentityJson = serde_json::from_slice(identity).context("start identity")?;
    Ok(Record::Start {
        identity: SessionIdentity {
            display_name: identity.display_name,
            xuid: identity.xuid,
            uuid: identity.uuid,
        },
        client_data: String::from_utf8(client_data.to_vec()).context("start client data")?,
        startup: startup.to_vec(),
    })
}

/// Splits a u32-LE length-prefixed field from the front of `bytes`.
fn split_prefixed(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, rest) = bytes.split_first_chunk::<4>()?;
    let length = usize::try_from(u32::from_le_bytes(*length)).ok()?;
    (rest.len() >= length).then(|| rest.split_at(length))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Appends one length-prefixed field.
    fn field(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        out.extend_from_slice(bytes);
    }

    #[test]
    fn capture_records_round_trip_every_kind() {
        let mut start = Vec::new();
        field(
            &mut start,
            br#"{"display_name":"Steve","xuid":"1","uuid":"u"}"#,
        );
        field(&mut start, br#"{"GameVersion":"1"}"#);
        start.extend_from_slice(&[2, 9, 9]);
        let mut capture = vec![0];
        field(&mut capture, &start);
        for (kind, body) in [(1, &[1, 7][..]), (2, &[1, 8]), (3, &[])] {
            capture.push(kind);
            field(&mut capture, body);
        }
        let records = parse(&capture).unwrap();
        assert_eq!(
            records,
            [
                Record::Start {
                    identity: SessionIdentity {
                        display_name: "Steve".into(),
                        xuid: "1".into(),
                        uuid: "u".into(),
                    },
                    client_data: r#"{"GameVersion":"1"}"#.into(),
                    startup: vec![2, 9, 9],
                },
                Record::Batch {
                    direction: Direction::FromClient,
                    body: vec![1, 7],
                },
                Record::Batch {
                    direction: Direction::FromServer,
                    body: vec![1, 8],
                },
                Record::Tick,
            ]
        );
    }

    #[test]
    fn truncated_and_unknown_records_are_refused() {
        assert!(parse(&[1, 4, 0, 0, 0, 1]).is_err());
        assert!(parse(&[9, 0, 0, 0, 0]).is_err());
        assert!(parse(&[0, 2, 0, 0, 0, 1, 0]).is_err());
    }
}
