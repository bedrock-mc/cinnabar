//! Store settings, read from `store.json` beside the launcher config. Purchases stay off until the
//! file says `"store_purchases_enabled": true`; anything unreadable or malformed means off.

use std::{fs::File, io::Read, path::Path};

pub const SETTINGS_FILE: &str = "store.json";
const MAX_SETTINGS_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoreSettings {
    pub purchases_enabled: bool,
}

/// The settings in `path`; a missing, oversized or malformed file yields the defaults.
pub fn load(path: &Path) -> StoreSettings {
    let mut bytes = Vec::new();
    let read =
        File::open(path).and_then(|file| file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes));
    if read.is_err() || bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return StoreSettings::default();
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> StoreSettings {
    let enabled = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| value.get("store_purchases_enabled")?.as_bool())
        .unwrap_or(false);
    StoreSettings {
        purchases_enabled: enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_true_enables_purchases() {
        assert!(parse(br#"{"store_purchases_enabled": true}"#).purchases_enabled);
        for off in [
            &br#"{"store_purchases_enabled": false}"#[..],
            br#"{"store_purchases_enabled": "true"}"#,
            br#"{"store_purchases_enabled": 1}"#,
            br#"{}"#,
            b"not json",
            b"",
        ] {
            assert!(!parse(off).purchases_enabled, "{off:?}");
        }
    }

    #[test]
    fn a_missing_file_leaves_purchases_off() {
        assert!(!load(Path::new("/definitely/not/here/store.json")).purchases_enabled);
    }
}
