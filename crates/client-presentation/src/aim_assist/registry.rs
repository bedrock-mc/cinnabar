use std::{collections::BTreeMap, sync::Arc};

use bevy::prelude::Resource;
use protocol::{
    CameraAimAssistAction, CameraAimAssistActorPriority, CameraAimAssistCategory,
    CameraAimAssistPreset, CameraAimAssistRegistry, CameraAimAssistSettings, CameraEvent,
};

/// Borrowed per-item rules; lookup never copies a registry or allocates.
#[derive(Debug, Clone, Copy)]
pub struct AimAssistCategory<'a> {
    pub preset: Option<&'a CameraAimAssistPreset>,
    pub category: Option<&'a CameraAimAssistCategory>,
    pub target_liquids: bool,
}

/// Packet mutations allocate only when registry data changes, never during evaluation.
#[derive(Debug, Default, Resource)]
pub struct ServerAimAssist {
    categories: Vec<CameraAimAssistCategory>,
    presets: Vec<CameraAimAssistPreset>,
    actor_priorities: BTreeMap<(i32, i32, i32), i32>,
    settings: Option<CameraAimAssistSettings>,
    identity: Option<(u64, i32)>,
    last_sequence: u64,
    semantic_skips: u64,
    revision: u64,
}

impl ServerAimAssist {
    /// Dimensions invalidate targets; level-owned policy survives until the session changes.
    pub fn observe_identity(&mut self, identity: Option<(u64, i32)>) {
        if self.identity != identity {
            if self.identity.map(|identity| identity.0) != identity.map(|identity| identity.0) {
                self.categories.clear();
                self.presets.clear();
                self.actor_priorities.clear();
                self.settings = None;
                self.last_sequence = 0;
            }
            self.identity = identity;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Consumes each committed packet once while preserving registry update order.
    pub fn apply(&mut self, sequence: u64, event: &CameraEvent) {
        if sequence <= self.last_sequence {
            return;
        }
        self.last_sequence = sequence;
        match event {
            CameraEvent::AimAssist(settings) => self.set(settings),
            CameraEvent::AimAssistPresets(registry) => self.update_registry(registry),
            CameraEvent::AimAssistActorPriority(priorities) => self.update_priorities(priorities),
            _ => {}
        }
    }

    /// Clear removes the active request; unknown presets leave the previous request intact.
    pub fn set(&mut self, settings: &CameraAimAssistSettings) {
        if settings.action == CameraAimAssistAction::Clear {
            self.settings = None;
            self.revision = self.revision.wrapping_add(1);
        } else if settings.preset_id.is_empty()
            || self
                .presets
                .iter()
                .any(|preset| preset.identifier == settings.preset_id)
        {
            let _ = sim::minecraft_sin(0.0);
            self.settings = Some(settings.clone());
            self.revision = self.revision.wrapping_add(1);
        } else {
            self.semantic_skips = self.semantic_skips.saturating_add(1);
        }
    }

    /// Camera activation requires the referenced priority preset to exist.
    #[must_use]
    pub fn has_preset(&self, identifier: &str) -> bool {
        self.presets
            .iter()
            .any(|preset| preset.identifier.as_ref() == identifier)
    }

    /// Borrows the current command, including its unmodified target mode and view bounds.
    #[must_use]
    pub fn settings(&self) -> Option<&CameraAimAssistSettings> {
        self.settings.as_ref()
    }

    /// Invalidates cached candidates when an authoritative policy changes.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Counts well-formed commands that reference unknown registry entries.
    #[must_use]
    pub const fn semantic_skips(&self) -> u64 {
        self.semantic_skips
    }

    /// Empty hands use hand settings; unmatched held items use the default item category.
    #[must_use]
    pub fn category(&self, held_item: Option<&str>) -> AimAssistCategory<'_> {
        let preset = self.settings.as_ref().and_then(|settings| {
            self.presets
                .iter()
                .find(|preset| preset.identifier == settings.preset_id)
        });
        let name = preset.and_then(|preset| match held_item.filter(|item| !item.is_empty()) {
            None => preset.hand_settings.as_deref(),
            Some(item) => preset
                .item_settings
                .iter()
                .find(|entry| entry.item.as_ref() == item)
                .map(|entry| entry.category.as_ref())
                .or(preset.default_item_settings.as_deref()),
        });
        AimAssistCategory {
            preset,
            category: name.and_then(|name| {
                self.categories
                    .iter()
                    .find(|entry| entry.name.as_ref() == name)
            }),
            target_liquids: preset.is_some_and(|preset| {
                held_item.is_some_and(|item| {
                    preset
                        .liquid_targeting_list
                        .iter()
                        .any(|entry| entry.as_ref() == item)
                })
            }),
        }
    }

    /// Metadata carries table indices; missing entries have priority zero.
    #[must_use]
    pub fn actor_priority(&self, preset: i32, category: i32, actor: i32) -> Option<i32> {
        if self.actor_priorities.get(&(preset, 0, actor)) == Some(&-2) {
            None
        } else {
            Some(self.player_priority(preset, category, actor))
        }
    }

    /// Players use category priorities without the non-player exclusion sentinel.
    #[must_use]
    pub fn player_priority(&self, preset: i32, category: i32, actor: i32) -> i32 {
        self.actor_priorities
            .get(&(preset, category, actor))
            .copied()
            .unwrap_or(0)
    }

    /// Priority packets update individual keys and preserve unrelated values.
    fn update_priorities(&mut self, priorities: &[CameraAimAssistActorPriority]) {
        for entry in priorities {
            self.actor_priorities.insert(
                (entry.preset_index, entry.category_index, entry.actor_index),
                entry.priority,
            );
        }
    }

    /// Registry set replaces both tables; add updates named definitions in place.
    fn update_registry(&mut self, registry: &CameraAimAssistRegistry) {
        if registry.replace {
            self.categories.clear();
            self.presets.clear();
        }
        for category in registry.categories.iter() {
            if let Some(old) = self
                .categories
                .iter_mut()
                .find(|old| old.name == category.name)
            {
                *old = category.clone();
            } else {
                self.categories.push(category.clone());
            }
        }
        for preset in registry.presets.iter() {
            if let Some(old) = self
                .presets
                .iter_mut()
                .find(|old| old.identifier == preset.identifier)
            {
                *old = preset.clone();
            } else {
                self.presets.push(preset.clone());
            }
        }
    }
}

impl AimAssistCategory<'_> {
    /// Exact block exclusions and any matching tag suppress a candidate before ranking.
    #[must_use]
    pub fn block_priority(&self, identifier: &str, tags: &[Arc<str>]) -> Option<i32> {
        if self.preset.is_some_and(|preset| {
            preset
                .exclusions
                .blocks
                .iter()
                .any(|entry| entry.as_ref() == identifier)
                || preset
                    .exclusions
                    .block_tags
                    .iter()
                    .any(|entry| tags.contains(entry))
        }) {
            return None;
        }
        let Some(category) = self.category else {
            return Some(0);
        };
        let priorities = &category.priorities;
        let mut priority = priorities
            .blocks
            .iter()
            .find(|entry| entry.identifier.as_ref() == identifier)
            .map(|entry| entry.priority);
        for entry in priorities
            .block_tags
            .iter()
            .filter(|entry| tags.contains(&entry.identifier))
        {
            if entry.priority > priority.unwrap_or(-1) {
                priority = Some(entry.priority);
            }
        }
        priority.or(priorities.block_default).or(Some(0))
    }
}
