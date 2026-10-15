//! Dial items whose icon is one frame of a vertical animation strip.

use super::RuntimeIconCatalog;

/// What a dial item reads, which decides how its frame is chosen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialKind {
    /// Points at the world spawn in natural dimensions and spins elsewhere.
    Compass,
    /// Points at the player's last death in its dimension and spins elsewhere.
    RecoveryCompass,
    /// Shows the sky's celestial angle in natural dimensions and spins elsewhere.
    Clock,
}

/// A dial item, the strip the native client loads its frames from, and the icon identifier
/// those frames are carried under, one entry per frame with the frame index as metadata.
#[derive(Clone, Copy, Debug)]
pub struct DialStrip {
    pub item: &'static str,
    pub kind: DialKind,
    pub strip_path: &'static str,
    pub frames_identifier: &'static str,
}

/// Every dial item the icon carrier bakes frames for.
pub const DIAL_STRIPS: [DialStrip; 3] = [
    DialStrip {
        item: crate::facts::VanillaItem::Compass.identifier(),
        kind: DialKind::Compass,
        strip_path: "textures/items/compass_atlas.png",
        frames_identifier: "minecraft:compass_atlas",
    },
    DialStrip {
        item: crate::facts::VanillaItem::RecoveryCompass.identifier(),
        kind: DialKind::RecoveryCompass,
        strip_path: "textures/items/recovery_compass_atlas.png",
        frames_identifier: "minecraft:recovery_compass_atlas",
    },
    DialStrip {
        item: crate::facts::VanillaItem::Clock.identifier(),
        kind: DialKind::Clock,
        strip_path: "textures/items/watch_atlas.png",
        frames_identifier: "minecraft:watch_atlas",
    },
];

/// The dial strip for item `identifier`, if it is a dial.
#[must_use]
pub fn dial_strip(identifier: &str) -> Option<&'static DialStrip> {
    DIAL_STRIPS.iter().find(|strip| strip.item == identifier)
}

impl RuntimeIconCatalog {
    /// How many consecutive frames, from metadata 0, the catalog carries for `identifier`.
    #[must_use]
    pub fn frame_count(&self, identifier: &str) -> u32 {
        let start = self
            .entries
            .partition_point(|entry| entry.identifier.as_ref() < identifier);
        self.entries[start..]
            .iter()
            .take_while(|entry| entry.identifier.as_ref() == identifier)
            .enumerate()
            .take_while(|(frame, entry)| entry.metadata as usize == *frame)
            .count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IconEntry, IconSprite, encode_icon_catalog};

    /// A catalog carrying `entries`, all drawn with one blank sprite.
    fn catalog(entries: &[(&str, u32)]) -> RuntimeIconCatalog {
        let sprite = IconSprite {
            width: 1,
            height: 1,
            rgba8: vec![0; 4].into(),
        };
        let mut entries: Vec<_> = entries
            .iter()
            .map(|(identifier, metadata)| IconEntry {
                identifier: (*identifier).into(),
                metadata: *metadata,
                sprite: 0,
            })
            .collect();
        entries.sort_by(|a, b| {
            (a.identifier.as_ref(), a.metadata).cmp(&(b.identifier.as_ref(), b.metadata))
        });
        RuntimeIconCatalog::decode(&encode_icon_catalog([0; 32], &[sprite], &entries).unwrap())
            .unwrap()
    }

    #[test]
    fn frame_count_counts_only_consecutive_frames_from_zero() {
        let catalog = catalog(&[
            ("minecraft:compass", 0),
            ("minecraft:compass_atlas", 0),
            ("minecraft:compass_atlas", 1),
            ("minecraft:compass_atlas", 2),
            ("minecraft:compass_atlas", 4),
            ("minecraft:watch_atlas", 1),
        ]);
        assert_eq!(catalog.frame_count("minecraft:compass_atlas"), 3);
        assert_eq!(catalog.frame_count("minecraft:watch_atlas"), 0);
        assert_eq!(catalog.frame_count("minecraft:unknown_atlas"), 0);
    }

    #[test]
    fn every_dial_item_has_its_own_strip() {
        for strip in &DIAL_STRIPS {
            assert_eq!(dial_strip(strip.item).unwrap().strip_path, strip.strip_path);
            assert_ne!(strip.item, strip.frames_identifier);
        }
        assert!(dial_strip("minecraft:stick").is_none());
    }
}
