//! Vanilla client definitions use the greatest minimum compatible with the pack's engine.
use super::*;
use std::cmp::Ordering;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    parts: [u16; 3],
    prerelease: Box<str>,
}

impl Version {
    fn release(parts: [u16; 3]) -> Self {
        Self {
            parts,
            prerelease: "".into(),
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ => compare_prerelease(&self.prerelease, &other.prerelease),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        let (left, right) = match (left.next(), right.next()) {
            (Some(left), Some(right)) => (left, right),
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        };
        let order = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

fn suffix(text: &str, prerelease: bool) -> bool {
    text.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !(prerelease
                && part.len() > 1
                && part.starts_with('0')
                && part.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn string_version(text: &str) -> Option<Version> {
    if text == "beta" {
        return Some(Version {
            parts: [9999; 3],
            prerelease: "beta".into(),
        });
    }
    let (text, build) = match text.split_once('+') {
        Some((text, build)) if suffix(build, false) => (text, true),
        Some(_) => return None,
        None => (text, false),
    };
    let (core, prerelease) = match text.split_once('-') {
        Some((core, prerelease)) if suffix(prerelease, true) => (core, prerelease),
        Some(_) => return None,
        None => (text, ""),
    };
    let mut parts = [0; 3];
    let mut count = 0;
    for part in core.split('.') {
        if count == parts.len()
            || part.is_empty()
            || part.len() > 5
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        parts[count] = part.parse().ok()?;
        count += 1;
    }
    if count != 3 && (build || !prerelease.is_empty()) {
        return None;
    }
    Some(Version {
        parts,
        prerelease: prerelease.into(),
    })
}

fn version(value: &Value) -> Option<Version> {
    match value {
        Value::String(text) => string_version(text),
        // The asset compiler validates array components as integers.
        Value::Array(parts) if parts.len() == 3 => {
            let mut result = [0; 3];
            for (slot, part) in result.iter_mut().zip(parts) {
                *slot = u16::try_from(part.as_u64()?).ok()?;
            }
            Some(Version::release(result))
        }
        _ => None,
    }
}

fn ceiling(payloads: &SourcePayloads) -> Result<Version, AssetError> {
    if let Some(bytes) = payloads.get("manifest.json") {
        let manifest = parse_unique_json(Path::new("manifest.json"), bytes)?;
        return version(&manifest["header"]["min_engine_version"]).ok_or_else(|| {
            invalid("manifest.json has missing or invalid header.min_engine_version")
        });
    }
    // Manifest-less base-asset inputs use the compiler's pinned target engine.
    let target: Value =
        serde_json::from_str(include_str!("../../../../assets/bedrock-target.json"))
            .map_err(|_| invalid("invalid Bedrock target manifest"))?;
    version(&target["game_version"]).ok_or_else(|| invalid("invalid target engine version"))
}

pub(super) fn select_vanilla_definitions(
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    payloads: &SourcePayloads,
) -> Result<(), AssetError> {
    let ceiling = Some(ceiling(payloads)?);
    let mut selected = BTreeMap::<Box<str>, (Option<Version>, Box<str>, bool)>::new();
    for symbol in symbols
        .values()
        .filter(|symbol| symbol.kind == EntityAssetKind::Entity)
    {
        let bytes = payloads
            .get(&symbol.source_path)
            .ok_or_else(|| invalid("entity source is absent"))?;
        let definition = parse_unique_json(Path::new(symbol.source_path.as_ref()), bytes)?;
        let schema = definition.get("format_version").and_then(version);
        let minimum = if schema.is_some_and(|schema| schema < Version::release([1, 8, 0])) {
            None
        } else {
            animation::roots::description(&definition)
                .and_then(|description| description.get("min_engine_version"))
                .and_then(version)
        };
        if minimum > ceiling {
            continue;
        }
        let entry = selected.entry(symbol.identifier.clone());
        match entry {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((minimum, symbol.source_path.clone(), false));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let current = entry.get_mut();
                if minimum > current.0 {
                    *current = (minimum, symbol.source_path.clone(), false);
                } else if minimum == current.0 {
                    current.2 = true;
                }
            }
        }
    }
    if let Some((identifier, _)) = selected.iter().find(|(_, (_, _, tied))| *tied) {
        return Err(invalid(format!(
            "equally versioned client definitions require pack-order merging: {identifier}"
        )));
    }
    symbols.retain(|_, symbol| {
        symbol.kind != EntityAssetKind::Entity
            || selected
                .get(&symbol.identifier)
                .is_some_and(|(_, path, _)| *path == symbol.source_path)
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_versions_compare_numerically_in_both_json_forms() {
        let expected = Some(Version::release([1, 17, 10]));
        assert_eq!(version(&serde_json::json!("1.17.10")), expected);
        assert_eq!(version(&serde_json::json!([1, 17, 10])), expected);
        assert!(version(&serde_json::json!("1.17.10")) > version(&serde_json::json!("1.2.6")));
        for value in [
            Value::Null,
            serde_json::json!("invalid"),
            serde_json::json!([1, -1, 0]),
        ] {
            assert_eq!(version(&value), None);
        }
    }

    #[test]
    fn short_string_versions_fill_missing_components_with_zero() {
        for (text, expected) in [("1", [1, 0, 0]), ("1.17", [1, 17, 0]), ("0", [0; 3])] {
            assert_eq!(string_version(text), Some(Version::release(expected)));
        }
        assert_eq!(
            string_version("65535.65535.65535").unwrap().parts,
            [65535; 3]
        );
    }

    #[test]
    fn invalid_versions_do_not_gain_compatibility_from_permissive_numeric_parsing() {
        for text in [
            "",
            "1.",
            "1.2.",
            "1.2.3.4",
            "+1.2.3",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "65536",
            "1.65536.0",
            "1.2.65536",
            "1.2.3-01",
            "1.2.3-alpha..1",
            "1.2.3+",
            "1.2.3+a+b",
            "1.2-beta",
            "1.2+build",
            "*",
            " 1.2.3",
        ] {
            assert_eq!(string_version(text), None, "invalid version {text}");
        }
        for value in [
            serde_json::json!([1, 2]),
            serde_json::json!([1, 2, 3, 4]),
            serde_json::json!([1, 2, 65536]),
            serde_json::json!([1, 2, 3.5]),
            serde_json::json!([1, true, null]),
        ] {
            assert_eq!(version(&value), None, "strict array version {value}");
        }
    }

    #[test]
    fn prerelease_components_use_numeric_then_text_precedence_and_ignore_builds() {
        let ordered = [
            "1.2.3-alpha",
            "1.2.3-alpha.1",
            "1.2.3-alpha.2",
            "1.2.3-alpha.10",
            "1.2.3-alpha.beta",
            "1.2.3-beta",
            "1.2.3-beta.2",
            "1.2.3-beta.11",
            "1.2.3-rc.1",
            "1.2.3",
        ];
        for pair in ordered.windows(2) {
            assert!(
                string_version(pair[0]) < string_version(pair[1]),
                "{pair:?}"
            );
        }
        assert_eq!(string_version("1.2.3+001.foo-bar"), string_version("1.2.3"));
        assert_eq!(
            string_version("1.2.3-beta.2+build.1"),
            string_version("1.2.3-beta.2+build.2")
        );
        assert_eq!(
            string_version("beta"),
            string_version("9999.9999.9999-beta")
        );
    }
}
