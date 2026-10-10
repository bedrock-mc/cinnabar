//! JSON field encodings the manifest has always used.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::Artifact;

/// Older signers wrote an absent artifact map as `null`.
pub(crate) fn null_as_empty<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<BTreeMap<String, Artifact>, D::Error> {
    Ok(Option::deserialize(input)?.unwrap_or_default())
}

/// RFC 3339 timestamps, as the manifest has always carried them.
pub(crate) mod rfc3339 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _, ser::Error as _};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    pub(crate) fn serialize<S: Serializer>(at: &OffsetDateTime, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(&at.format(&Rfc3339).map_err(S::Error::custom)?)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        input: D,
    ) -> Result<OffsetDateTime, D::Error> {
        let text = String::deserialize(input)?;
        OffsetDateTime::parse(&text, &Rfc3339).map_err(D::Error::custom)
    }
}
