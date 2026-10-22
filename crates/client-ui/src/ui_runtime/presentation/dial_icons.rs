//! Animated dial icons: compasses and clocks show their strip frame for this frame.

use super::UiPresentationRuntime;

impl UiPresentationRuntime {
    /// The strip frames dial icons show.
    #[must_use]
    pub const fn dial_frames(&self) -> inventory::DialFrames {
        self.dial_frames
    }

    /// Replaces the strip frames dial icons show from now on.
    pub fn set_dial_frames(&mut self, frames: inventory::DialFrames) {
        self.dial_frames = frames;
    }

    /// The carried strip frame dial item `identifier` shows, as an icon identity. `None` for
    /// session overrides, other items, dials without a frame, or a catalog without their frames.
    #[must_use]
    pub fn dial_icon_key(&self, identifier: &str, metadata: u32) -> Option<(&'static str, u32)> {
        if self.session_item_icon(identifier, metadata).is_some() {
            return None;
        }
        self.dial_frames
            .icon_key(identifier, self.icon_catalog.as_deref()?)
    }

    /// Returns all carried frames of the currently animated dial, respecting session overrides.
    #[must_use]
    pub fn dial_icon_frames(
        &self,
        identifier: &str,
        metadata: u32,
    ) -> Option<(&'static str, std::ops::Range<u32>)> {
        let (identifier, _) = self.dial_icon_key(identifier, metadata)?;
        let count = self.icon_catalog.as_deref()?.frame_count(identifier);
        Some((identifier, 0..count))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_support::fixture_hud;
    use crate::ui_runtime::UiRuntime;
    use crate::ui_runtime::presentation::publish::item_icons::test_support::stack_icon;
    use ui_engine::test_support::fixture_font;

    #[test]
    fn dial_frames_preserve_session_icons_and_restore_after_removal() {
        use super::super::session_icons::{SessionIcon, SessionIcons, observe};
        let mut presentation = presentation(inventory::COMPASS_FRAMES);
        presentation.set_dial_frames(inventory::DialFrames {
            compass: Some(7),
            recovery_compass: Some(7),
            clock: Some(7),
        });
        let original = icon(&presentation, "minecraft:compass");
        let identifiers = [
            "minecraft:compass",
            "minecraft:recovery_compass",
            "minecraft:clock",
        ];
        let overrides = Arc::new(SessionIcons {
            icons: identifiers
                .iter()
                .map(|identifier| SessionIcon {
                    identifier: (*identifier).into(),
                    metadata: 0,
                    width: 1,
                    height: 1,
                    rgba8: vec![255, 0, 0, 255].into(),
                })
                .collect(),
            ..Default::default()
        });
        observe(&mut presentation, Some(&overrides));
        for identifier in identifiers {
            assert_eq!(presentation.dial_icon_key(identifier, 0), None);
            assert_eq!(presentation.dial_icon_frames(identifier, 0), None);
            assert_eq!(
                icon(&presentation, identifier),
                presentation.item_icon(identifier, 0)
            );
        }
        assert_ne!(icon(&presentation, "minecraft:compass"), original);
        observe(&mut presentation, None);
        assert_eq!(icon(&presentation, "minecraft:compass"), original);
        assert_eq!(
            presentation.dial_icon_frames("minecraft:compass", 0),
            Some(("minecraft:compass_atlas", 0..inventory::COMPASS_FRAMES))
        );
    }

    /// A presentation whose catalog has a static compass and clock and `frames` compass strip
    /// frames, each its own sprite.
    fn presentation(frames: u32) -> UiPresentationRuntime {
        let mut entries = vec![
            assets::IconEntry {
                identifier: "minecraft:clock".into(),
                metadata: 0,
                sprite: 0,
            },
            assets::IconEntry {
                identifier: "minecraft:compass".into(),
                metadata: 0,
                sprite: 0,
            },
        ];
        entries.extend((0..frames).map(|frame| assets::IconEntry {
            identifier: "minecraft:compass_atlas".into(),
            metadata: frame,
            sprite: frame + 1,
        }));
        let sprites: Vec<_> = (0..=frames)
            .map(|sprite| assets::IconSprite {
                width: 1,
                height: 1,
                rgba8: vec![sprite as u8; 4].into(),
            })
            .collect();
        let icons = assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([7; 32], &sprites, &entries).unwrap(),
        )
        .unwrap();
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), Arc::new(icons))
            .unwrap()
    }

    /// The slot icon of a one-item stack of `identifier`.
    fn icon(presentation: &UiPresentationRuntime, identifier: &str) -> Option<ui::IconRef> {
        let stack = protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        };
        let item = client_world::CanonicalItemStack {
            use_animation: assets::item_use::ItemUseAnimation::None,
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            identity: assets::ItemStackIdentity::empty(),
            identifier: Some(identifier.into()),
            visual: assets::ItemVisualRoute::Missing,
            map_id: None,
            charged_projectile: None,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            damage: None,
            durability: assets::item::durability::ItemDurability::NotDamageable,
            enchanted: false,
        };
        stack_icon(&UiRuntime::new(1), presentation, &stack, item, None)
    }

    #[test]
    fn compass_icons_show_their_frame_and_keep_the_static_icon_without_frames() {
        let mut presentation = presentation(inventory::COMPASS_FRAMES);
        let still = icon(&presentation, "minecraft:compass");
        assert_eq!(still, presentation.item_icon("minecraft:compass", 0));
        assert_eq!(presentation.dial_icon_frames("minecraft:compass", 0), None);
        presentation.set_dial_frames(inventory::DialFrames {
            compass: Some(7),
            recovery_compass: None,
            clock: Some(3),
        });
        assert_eq!(
            icon(&presentation, "minecraft:compass"),
            presentation.item_icon("minecraft:compass_atlas", 7)
        );
        assert_eq!(
            icon(&presentation, "minecraft:clock"),
            presentation.item_icon("minecraft:clock", 0),
            "no clock frames are carried"
        );
        assert_eq!(presentation.dial_icon_frames("minecraft:clock", 0), None);
    }
}
