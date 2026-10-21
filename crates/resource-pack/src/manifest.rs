//! Lenient manifest reading: only identity, module kinds, and subpack requirements
//! matter for application, so every other field is ignored.

use serde_json::Value;
use uuid::Uuid;

use pack_json::parse_jsonc;

use crate::{AdmissionError, Subpack};

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

/// Local selections must match their archive; server selections use negotiated transport IDs.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum IdentityPolicy {
    Manifest,
    #[cfg(feature = "handoff")]
    ServerAdvertisement,
}

/// Validates a manifest against a locally selected identity.
pub(crate) fn read_manifest(
    bytes: &[u8],
    pack_id: Uuid,
    version: &str,
) -> Result<Manifest, AdmissionError> {
    read_manifest_with_policy(bytes, pack_id, version, IdentityPolicy::Manifest)
}

/// Validates manifest structure while keeping server transfer identity separate from authored metadata.
pub(crate) fn read_manifest_with_policy(
    bytes: &[u8],
    pack_id: Uuid,
    version: &str,
    identity: IdentityPolicy,
) -> Result<Manifest, AdmissionError> {
    let root: Value = parse_jsonc(bytes).map_err(|_| AdmissionError::MalformedManifest)?;
    let format = match &root["format_version"] {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<u64>().ok().map(|number| number as f64),
        _ => None,
    };
    if !format.is_some_and(|format| (1.0..=3.0).contains(&format) && format.fract() == 0.0) {
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
    if identity == IdentityPolicy::Manifest
        && (uuid != pack_id || manifest_version != selected_version)
    {
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

    #[test]
    #[cfg(feature = "handoff")]
    fn server_identity_policy_still_validates_manifest_and_selected_metadata() {
        let valid = manifest("2", "[1, 2, 3]", r#"{"type":"resources"}"#);
        let check = |text: &str, version: &str| {
            read_manifest_with_policy(
                text.as_bytes(),
                Uuid::from_u128(2),
                version,
                IdentityPolicy::ServerAdvertisement,
            )
        };
        assert!(check(&valid, "9.8.7").is_ok());
        assert_eq!(
            check(&valid, "invalid").err(),
            Some(AdmissionError::InvalidVersion)
        );
        assert_eq!(
            check(&valid.replace(&ID.to_string(), "invalid"), "9.8.7").err(),
            Some(AdmissionError::MalformedManifest),
        );
        assert_eq!(
            check(&manifest("2", "[1, 2]", r#"{"type":"resources"}"#), "9.8.7").err(),
            Some(AdmissionError::InvalidVersion),
        );
        assert_eq!(
            check(&manifest("2", "[1, 2, 3]", r#"{"type":"data"}"#), "9.8.7").err(),
            Some(AdmissionError::InvalidModules),
        );
    }

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
    fn accepts_whole_numeric_manifest_formats_without_accepting_fractional_versions() {
        let resources = r#"{"type": "resources"}"#;
        for format in ["2.0", "1.0", "3.0", "2e0"] {
            let text = manifest(format, "[1, 2, 3]", resources);
            assert!(
                read_manifest(text.as_bytes(), ID, "1.2.3").is_ok(),
                "format {format}"
            );
        }
        for format in ["2.5", "-2.0", "0.0", "4.0", "1e100"] {
            let text = manifest(format, "[1, 2, 3]", resources);
            assert_eq!(
                read_manifest(text.as_bytes(), ID, "1.2.3").err(),
                Some(AdmissionError::UnsupportedManifestFormat),
                "format {format}"
            );
        }
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
