//! Lenient manifest reading: only identity, module kinds, and subpack requirements
//! matter for application, so every other field is ignored.

use serde_json::Value;
use uuid::Uuid;

use crate::{AdmissionError, MAX_SUBPACKS, Subpack, normalize_jsonc};

/// A `major.minor.patch` pack version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Version(pub(crate) [u32; 3]);

impl Version {
    /// Parses a dotted triple; leading zeros are tolerated and a semver
    /// pre-release or build suffix on the patch is ignored.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().splitn(3, '.');
        let mut parsed = [0; 3];
        for part in &mut parsed {
            let text = parts.next()?;
            let digits = text
                .find(|c: char| !c.is_ascii_digit())
                .map_or(text, |end| &text[..end]);
            *part = digits.parse().ok()?;
        }
        Some(Self(parsed))
    }

    /// Reads either manifest version representation.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(text) => Self::parse(text),
            Value::Array(parts) if parts.len() == 3 => {
                let mut parsed = [0; 3];
                for (slot, part) in parsed.iter_mut().zip(parts) {
                    *slot = u32::try_from(part.as_u64()?).ok()?;
                }
                Some(Self(parsed))
            }
            _ => None,
        }
    }
}

pub(crate) struct Manifest {
    pub(crate) subpacks: Vec<Subpack>,
}

/// Validates a manifest against the server-selected identity.
pub(crate) fn read_manifest(
    bytes: &[u8],
    pack_id: Uuid,
    version: &str,
) -> Result<Manifest, AdmissionError> {
    let json = normalize_jsonc(bytes).ok_or(AdmissionError::MalformedManifest)?;
    let root: Value =
        serde_json::from_slice(&json).map_err(|_| AdmissionError::MalformedManifest)?;
    let format = match &root["format_version"] {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    };
    if !matches!(format, Some(1..=3)) {
        return Err(AdmissionError::UnsupportedManifestFormat);
    }
    let header = &root["header"];
    let uuid = header["uuid"]
        .as_str()
        .and_then(|text| Uuid::parse_str(text.trim()).ok())
        .ok_or(AdmissionError::MalformedManifest)?;
    let manifest_version =
        Version::from_value(&header["version"]).ok_or(AdmissionError::InvalidVersion)?;
    let selected_version = Version::parse(version).ok_or(AdmissionError::InvalidVersion)?;
    if uuid != pack_id || manifest_version != selected_version {
        return Err(AdmissionError::ManifestIdentityMismatch);
    }
    let has_resources = root["modules"].as_array().is_some_and(|modules| {
        modules
            .iter()
            .any(|module| module["type"].as_str() == Some("resources"))
    });
    if !has_resources {
        return Err(AdmissionError::InvalidModules);
    }
    let subpacks = root["subpacks"].as_array().map_or(&[][..], Vec::as_slice);
    if subpacks.len() > MAX_SUBPACKS {
        return Err(AdmissionError::InvalidSubpack);
    }
    let subpacks = subpacks
        .iter()
        .filter_map(|subpack| {
            let folder = subpack["folder_name"].as_str()?;
            (!folder.is_empty() && !folder.contains(['/', '\\']) && !matches!(folder, "." | ".."))
                .then(|| Subpack {
                    folder: folder.into(),
                    name: subpack["name"].as_str().unwrap_or(folder).into(),
                    memory_tier: crate::subpacks::manifest_memory_tier(subpack),
                })
        })
        .collect();
    Ok(Manifest { subpacks })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Uuid = Uuid::from_u128(1);

    fn manifest(format: &str, version: &str, modules: &str) -> String {
        format!(
            r#"{{"format_version": {format}, "header": {{"uuid": "{ID}", "version": {version},
            "unknown": true}}, "modules": [{modules}], "dependencies": [{{"module_name": "@x"}}],
            "extra": {{}}}}"#
        )
    }

    #[test]
    fn accepts_formats_one_to_three_and_ignores_unknown_fields() {
        let resources = r#"{"type": "resources"}, {"type": "script", "language": "js"}"#;
        for (format, version) in [("1", "[1, 2, 3]"), ("2", "[1, 2, 3]"), ("3", "\"1.2.3\"")] {
            let text = manifest(format, version, resources);
            assert!(
                read_manifest(text.as_bytes(), ID, "1.2.3").is_ok(),
                "format {format}"
            );
        }
        let text = manifest("4", "[1, 2, 3]", resources);
        assert_eq!(
            read_manifest(text.as_bytes(), ID, "1.2.3").err(),
            Some(AdmissionError::UnsupportedManifestFormat)
        );
    }

    #[test]
    fn rejects_identity_mismatch_and_behavior_only_packs() {
        let text = manifest("2", "[1, 2, 3]", r#"{"type": "resources"}"#);
        assert_eq!(
            read_manifest(text.as_bytes(), ID, "1.2.4").err(),
            Some(AdmissionError::ManifestIdentityMismatch)
        );
        let text = manifest("2", "[1, 2, 3]", r#"{"type": "data"}"#);
        assert_eq!(
            read_manifest(text.as_bytes(), ID, "1.2.3").err(),
            Some(AdmissionError::InvalidModules)
        );
    }

    #[test]
    fn version_parse_tolerates_leading_zeros_and_semver_suffix() {
        assert_eq!(Version::parse("01.0.11"), Some(Version([1, 0, 11])));
        assert_eq!(Version::parse("1.0.0-beta"), Some(Version([1, 0, 0])));
        assert_eq!(Version::parse("1.0"), None);
    }
}
