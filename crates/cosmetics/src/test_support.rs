//! Small synthetic clip handles for parser, UI and articulated-rig regressions.
use super::*;

/// Admits a synthetic clip without activating or copying any production catalog item.
pub fn emote(id: &str, name: &str, free: bool) -> emotes::Item {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../core/cinnabarapi/schema/emote-clips.json"
    ))
    .unwrap();
    let bytes = serde_json::to_vec(&cases[0]["clip"]).unwrap();
    let descriptor = emotes::Descriptor {
        id: id.to_owned(),
        display_name: name.to_owned(),
        free,
        clip: Asset {
            url: String::new(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes: bytes.len(),
        },
    };
    emotes::Item::admit(descriptor, &bytes).unwrap()
}

/// Serializes tests that replace the process-wide active catalog.
pub fn catalog_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

/// Admits the shared synthetic playback fixture without changing catalog ownership.
pub fn playback_emote() -> emotes::Item {
    emote("fixture:playback", "Clip", true)
}

/// Installs one owned synthetic catalog clip while excluding other catalog-mutating tests.
pub fn playback_catalog() -> std::sync::MutexGuard<'static, ()> {
    let guard = catalog_lock();
    emotes::activate(&[playback_emote()]);
    guard
}
