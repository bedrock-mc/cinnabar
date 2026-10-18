//! Bounded bindings to facts the host already observes, without guest polling or serialization.

use serde::{Deserialize, Serialize};

/// Selects native rows within the card's explicitly reserved row budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowSource {
    pub kind: NativeRows,
    pub limit: u8,
    #[serde(default = "super::white")]
    pub color: [f32; 4],
    #[serde(default)]
    pub brackets: bool,
}

/// Read-only presentation of the current session's admitted inventory, effects or sidebar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeRows {
    /// Displays native local wall-clock time; no game clock or guest polling is involved.
    Clock {
        military: bool,
        am_pm: bool,
    },
    /// A native local activation counter; inventory quantity never contributes to this value.
    TotemUses {
        flip: bool,
        count_color: bool,
    },
    Inventory {
        hotbar: bool,
    },
    Armor {
        held: bool,
        percent: bool,
        empty: bool,
    },
    Effects {
        icons: bool,
    },
    Sidebar {
        scores: bool,
    },
    /// Counts known inventory and offhand stacks matching these identities or block items.
    ItemCount {
        label: String,
        items: Vec<String>,
        blocks: bool,
        /// Also counts admitted potion forms whose auxiliary type grants this effect.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        potion_effect: Option<String>,
    },
}

/// Projects native replicated health over eligible actors, without exposing unobserved state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorHealth {
    pub players: bool,
    pub mobs: bool,
    pub range: f32,
    #[serde(default = "super::white")]
    pub color: [f32; 4],
}

/// Depth-tested outlines of native actor bounds, separate from projected health text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorBounds {
    pub players: bool,
    pub mobs: bool,
    pub range: f32,
    pub color: [f32; 4],
}

impl ActorBounds {
    /// Keeps outline selection within the same bounded actor observation range.
    pub fn validate(&self) -> Result<(), String> {
        if !self.range.is_finite() || !(0.0..=128.0).contains(&self.range) {
            return Err("actor outline range exceeds its bounds".into());
        }
        super::color(self.color)
    }
}

impl ActorHealth {
    /// Keeps projected label work and distances within the actor observation boundary.
    pub fn validate(&self) -> Result<(), String> {
        if !self.range.is_finite() || !(0.0..=128.0).contains(&self.range) {
            return Err("actor health label range exceeds its bounds".into());
        }
        super::color(self.color)
    }
}

impl RowSource {
    /// Reserves the maximum materialized rows before any observations become available.
    pub fn validate(&self) -> Result<(), String> {
        if self.limit == 0 || usize::from(self.limit) > super::MAX_CARD_ROWS {
            return Err("native HUD source requires a bounded positive row limit".into());
        }
        if matches!(
            self.kind,
            NativeRows::Clock { .. } | NativeRows::TotemUses { .. }
        ) && (self.limit != 1 || self.brackets)
        {
            return Err(
                "native scalar sources reserve one plain row; text plates own brackets".into(),
            );
        }
        if let NativeRows::ItemCount {
            label,
            items,
            potion_effect,
            ..
        } = &self.kind
        {
            super::text(label)?;
            if items.len() > 16 || items.iter().any(|id| !super::item_identifier(id)) {
                return Err("native item count exceeds its identity bounds".into());
            }
            if potion_effect
                .as_ref()
                .is_some_and(|effect| assets::effect_descriptor_by_identifier(effect).is_none())
            {
                return Err("native item count requires a known potion effect".into());
            }
        }
        super::color(self.color)
    }
}
