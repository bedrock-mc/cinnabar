//! Lenient manifest reading: only identity, module kinds, and subpack requirements
//! matter for application, so every other field is ignored.

use serde_json::Value;
use uuid::Uuid;

use bedrock_packs::jsonc::parse_jsonc_lenient;

use crate::{AdmissionError, Subpack};

mod format;

use format::Layout;

/// A `major.minor.patch` pack version, compared by number as the client compares pack identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Version(pub(crate) [u32; 3]);

impl Version {
    /// Parses a manifest semantic version; opaque advertisement bytes use `parse_wire`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        format::SemVersion::parse(text).map(|version| Self(version.triple))
    }

    /// Parses up to three numeric prefixes, filling absent components with zero.
    /// The original wire bytes remain unchanged.
    /// Leading whitespace and opaque component suffixes retain the text parser's leniency.
    pub(crate) fn parse_wire(text: impl AsRef<[u8]>) -> Option<Self> {
        let text = text.as_ref();
        let prefix = text.utf8_chunks().next()?.valid();
        let text = &text[prefix.len() - prefix.trim_start().len()..];
        let mut parsed = [0u32; 3];
        for (part, text) in parsed.iter_mut().zip(text.splitn(3, |byte| *byte == b'.')) {
            let digits = text.iter().take_while(|byte| byte.is_ascii_digit());
            let mut count = 0;
            for digit in digits {
                *part = part
                    .checked_mul(10)?
                    .checked_add(u32::from(*digit - b'0'))?;
                count += 1;
            }
            if count == 0 {
                return None;
            }
        }
        Some(Self(parsed))
    }

    /// Reads a manifest version written as a string or a `[major, minor, patch]` array.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(text) => Self::parse(text),
            Value::Array(parts) if parts.len() == 3 => {
                let mut parsed = [0; 3];
                for (slot, part) in parsed.iter_mut().zip(parts) {
                    *slot = u32::from(u16::try_from(part.as_u64()?).ok()?);
                }
                Some(Self(parsed))
            }
            _ => None,
        }
    }
}

/// Acquisition verifies opaque identity; application still requires a renderable resources module.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Purpose {
    Rendering,
    Identity,
}

/// The pack identity a manifest declares, read with the layout its format version selects.
pub(crate) fn manifest_identity(
    bytes: &[u8],
    root: &Value,
) -> Result<(Uuid, Version), AdmissionError> {
    let layout = format::layout(bytes, root).ok_or(AdmissionError::UnsupportedManifestFormat)?;
    declared_identity(root, layout)
}

/// The pack identity a manifest declares in its layout's header keys.
fn declared_identity(root: &Value, layout: Layout) -> Result<(Uuid, Version), AdmissionError> {
    let (uuid_key, version_key) = layout.identity_keys();
    let header = &root["header"];
    let uuid = header[uuid_key]
        .as_str()
        .and_then(|text| Uuid::parse_str(text.trim()).ok())
        .ok_or(AdmissionError::MalformedManifest)?;
    let version =
        Version::from_value(&header[version_key]).ok_or(AdmissionError::InvalidVersion)?;
    Ok((uuid, version))
}

pub(crate) struct Manifest {
    pub(crate) has_resources: bool,
    pub(crate) subpacks: Vec<Subpack>,
    pub(crate) has_behaviors: bool,
}

/// Local selections must match their archive; server selections use negotiated transport IDs.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum IdentityPolicy {
    Manifest,
    ServerAdvertisement,
}

/// Validates a manifest against a locally selected identity.
pub(crate) fn read_manifest(
    bytes: &[u8],
    pack_id: Uuid,
    version: impl AsRef<[u8]>,
    purpose: Purpose,
) -> Result<Manifest, AdmissionError> {
    read_manifest_with_policy(bytes, pack_id, version, purpose, IdentityPolicy::Manifest)
}

/// Validates authored metadata under the caller's rendering and transport-identity policies.
pub(crate) fn read_manifest_with_policy(
    bytes: &[u8],
    pack_id: Uuid,
    version: impl AsRef<[u8]>,
    purpose: Purpose,
    identity: IdentityPolicy,
) -> Result<Manifest, AdmissionError> {
    let root: Value = parse_jsonc_lenient(bytes).map_err(|_| AdmissionError::MalformedManifest)?;
    let layout = format::layout(bytes, &root).ok_or(AdmissionError::UnsupportedManifestFormat)?;
    let (uuid, manifest_version) = declared_identity(&root, layout)?;
    let selected_version = Version::parse_wire(version).ok_or(AdmissionError::InvalidVersion)?;
    if identity == IdentityPolicy::Manifest
        && (uuid != pack_id || manifest_version != selected_version)
    {
        return Err(AdmissionError::ManifestIdentityMismatch);
    }
    if purpose == Purpose::Identity {
        validate_identity_modules(layout.modules(&root))?;
    }
    let has_resources = layout.modules(&root).as_array().is_some_and(|modules| {
        modules
            .iter()
            .any(|module| module["type"].as_str() == Some("resources"))
    });
    if purpose == Purpose::Rendering && !has_resources {
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
    let has_behaviors = layout.modules(&root).as_array().is_some_and(|modules| {
        modules
            .iter()
            .any(|module| matches!(module["type"].as_str(), Some("data" | "client_data")))
    });
    Ok(Manifest {
        has_resources,
        subpacks,
        has_behaviors,
    })
}

/// Checks consumed module shapes while preserving null defaults and unknown type names.
fn validate_identity_modules(modules: &Value) -> Result<(), AdmissionError> {
    match modules {
        Value::Null => Ok(()),
        Value::Array(modules)
            if modules.iter().all(|module| match module {
                Value::Null => true,
                Value::Object(module) => matches!(
                    module.get("type"),
                    None | Some(Value::Null | Value::String(_))
                ),
                _ => false,
            }) =>
        {
            Ok(())
        }
        _ => Err(AdmissionError::InvalidModules),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Uuid = Uuid::from_u128(1);

    #[test]
    fn pack_version_components_stop_at_u16_max() {
        for component in 0..3 {
            let mut parts = [0_u32; 3];
            parts[component] = 65536;
            assert_eq!(Version::from_value(&serde_json::json!(parts)), None);
            parts[component] = 65535;
            assert_eq!(
                Version::from_value(&serde_json::json!(parts)),
                Some(Version(parts))
            );
        }
    }

    #[test]
    fn server_identity_policy_still_validates_manifest_and_selected_metadata() {
        let valid = manifest("2", "[1, 2, 3]", r#"{"type":"resources"}"#);
        let check = |text: &str, version: &str| {
            read_manifest_with_policy(
                text.as_bytes(),
                Uuid::from_u128(2),
                version,
                Purpose::Rendering,
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
    fn numeric_formats_one_to_three_read_the_current_header_and_ignore_unknown_fields() {
        let resources = r#"{"type": "resources"}, {"type": "script", "language": "js"}"#;
        // The client reads a number's literal text as a semantic version and truncates it for the
        // legacy parser, so 2.5 is format 2 while 2e0 is not a version at all.
        for (format, version) in [
            ("1", "[1, 2, 3]"),
            ("2", "[1, 2, 3]"),
            ("2.0", "[1, 2, 3]"),
            ("2.5", "[1, 2, 3]"),
            ("3", "\"1.2.3\""),
            ("3.0", "[1, 2, 3]"),
            ("\"3.0.0\"", "[1, 2, 3]"),
            ("[3, 0, 0]", "[1, 2, 3]"),
        ] {
            let text = manifest(format, version, resources);
            assert!(
                read_manifest(text.as_bytes(), ID, "1.2.3", Purpose::Rendering).is_ok(),
                "format {format}"
            );
        }
        let text = format!(
            "# manifest header\n{} trailing garbage /* unfinished",
            manifest("2", "[1, 2, 3]", resources)
        );
        assert!(read_manifest(text.as_bytes(), ID, "1.2.3", Purpose::Rendering).is_ok());
        for format in [
            "4", "3.1", "2e0", "-2.0", "1e100", "\"4.0\"", "\"x\"", "[2, 0]", "{}",
        ] {
            let text = manifest(format, "[1, 2, 3]", resources);
            assert_eq!(
                read_manifest(text.as_bytes(), ID, "1.2.3", Purpose::Rendering).err(),
                Some(AdmissionError::UnsupportedManifestFormat),
                "format {format}"
            );
        }
    }

    #[test]
    fn string_and_array_formats_below_three_read_the_format_zero_header() {
        // The legacy parser reads these as format 0, whose header names the pack `pack_id`.
        for format in ["\"2\"", "\"2.0\"", "\"2.0.0\"", "[2, 0, 0]"] {
            let text = manifest(format, "[1, 2, 3]", r#"{"type": "resources"}"#);
            assert_eq!(
                read_manifest(text.as_bytes(), ID, "1.2.3", Purpose::Rendering).err(),
                Some(AdmissionError::MalformedManifest),
                "format {format}"
            );
        }
        let format_zero = format!(
            r#"{{"format_version": "2.0", "header": {{"pack_id": "{ID}", "packs_version": "1.2.3",
            "modules": [{{"type": "resources"}}]}}}}"#
        );
        assert!(read_manifest(format_zero.as_bytes(), ID, "1.2.3", Purpose::Rendering).is_ok());
        let unversioned = format_zero.replace(r#""format_version": "2.0", "#, "");
        assert!(read_manifest(unversioned.as_bytes(), ID, "1.2.3", Purpose::Rendering).is_ok());
    }

    #[test]
    fn rejects_identity_mismatch_and_behavior_only_packs() {
        let text = manifest("2", "[1, 2, 3]", r#"{"type": "resources"}"#);
        assert_eq!(
            read_manifest(text.as_bytes(), ID, "1.2.4", Purpose::Rendering).err(),
            Some(AdmissionError::ManifestIdentityMismatch)
        );
        let text = manifest("2", "[1, 2, 3]", r#"{"type": "data"}"#);
        assert_eq!(
            read_manifest(text.as_bytes(), ID, "1.2.3", Purpose::Rendering).err(),
            Some(AdmissionError::InvalidModules)
        );
    }

    #[test]
    fn versions_parse_with_the_client_grammar() {
        assert_eq!(Version::parse("1.0.0-beta"), Some(Version([1, 0, 0])));
        assert_eq!(Version::parse("1.0"), Some(Version([1, 0, 0])));
        assert_eq!(Version::parse("2"), Some(Version([2, 0, 0])));
        assert_eq!(Version::parse("01.0.11"), None);
        assert_eq!(Version::parse("x.0.0"), None);
    }

    #[test]
    fn partial_selected_version_matches_the_manifest_triple() {
        let text = manifest("2", "[1, 0, 0]", r#"{"type": "resources"}"#);
        assert!(read_manifest(text.as_bytes(), ID, "1.0", Purpose::Rendering).is_ok());
    }
}
