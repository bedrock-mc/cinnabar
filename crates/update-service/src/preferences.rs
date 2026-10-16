//! The desired release channel, persisted apart from the installed payload's channel.

use serde::{Deserialize, Serialize};
use std::{io, path::Path};
use update_manifest::channel::Channel;

const FILE: &str = "preferences.json";

#[derive(Deserialize, Serialize)]
struct Stored {
    channel: Channel,
}

/// The saved channel; without one, `legacy` (the game's old setting) is migrated once.
/// An unpublished choice reads as the default channel.
pub fn load(update_dir: &Path, legacy: impl FnOnce() -> Option<Channel>) -> Channel {
    let saved = std::fs::read(update_dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .map(|stored| stored.channel);
    let channel = match saved {
        Some(channel) => channel,
        None => {
            let migrated = legacy().unwrap_or_default();
            if let Err(error) = save(update_dir, migrated) {
                tracing::warn!("could not save the update channel: {error}");
            }
            migrated
        }
    };
    if channel.is_published() {
        channel
    } else {
        Channel::default()
    }
}

/// Replaces the saved channel atomically.
pub fn save(update_dir: &Path, channel: Channel) -> io::Result<()> {
    std::fs::create_dir_all(update_dir)?;
    let mut file = tempfile::NamedTempFile::new_in(update_dir)?;
    serde_json::to_writer(&mut file, &Stored { channel })?;
    file.persist(update_dir.join(FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_setting_migrates_once_and_later_choices_persist() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            load(root.path(), || Some(Channel::Nightly)),
            Channel::Nightly
        );
        // The migrated value is saved, so a later game setting no longer applies.
        assert_eq!(load(root.path(), || Some(Channel::Beta)), Channel::Nightly);
        save(root.path(), Channel::Beta).unwrap();
        assert_eq!(
            load(root.path(), || panic!("already migrated")),
            Channel::Beta
        );
    }

    #[test]
    fn missing_corrupt_or_unpublished_choices_fall_back_to_the_default() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(load(root.path(), || None), Channel::default());
        std::fs::write(root.path().join(FILE), b"{").unwrap();
        assert_eq!(load(root.path(), || None), Channel::default());
        save(root.path(), Channel::Stable).unwrap();
        let expected = if Channel::Stable.is_published() {
            Channel::Stable
        } else {
            Channel::default()
        };
        assert_eq!(load(root.path(), || None), expected);
    }
}
