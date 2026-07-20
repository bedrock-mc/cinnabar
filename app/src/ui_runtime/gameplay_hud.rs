//! App-owned authoritative gameplay-HUD state beyond the basic stat rows:
//! hotbar/offhand stacks, armor equipment, status effects, air, freezing, and
//! the local mount. Every field mirrors server state the Bedrock protocol
//! actually exposes; nothing here invents state.

use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorHandedness, ActorMetadata, ActorMetadataValue,
    ArmorEquipmentEvent, EquipmentEvent, HOTBAR_SLOT_COUNT, InventoryEvent, NetworkItemStack,
};

pub const MAX_HUD_EFFECTS: usize = 32;

/// Pinned protocol-1001 SetEntityData keys consumed by the HUD.
/// (`MetadataDictionaryItemKey::{Air, MaxAirdataMaxAir, FreezingEffectStrength}`.)
const METADATA_KEY_AIR_SUPPLY: i32 = 7;
const METADATA_KEY_MAX_AIR_SUPPLY: i32 = 42;
const METADATA_KEY_FREEZING_EFFECT_STRENGTH: i32 = 120;

/// Pinned vanilla Bedrock effect ids whose hearts recolor (poison family and
/// wither). Fatal poison shares poison's presentation.
const EFFECT_ID_POISON: i32 = 19;
const EFFECT_ID_WITHER: i32 = 20;
const EFFECT_ID_FATAL_POISON: i32 = 25;

/// Pinned vanilla protocol-1001 effect ids the HUD can present. Instant
/// effects (6, 7, 23) have no HUD surface; the presentation icon table pins
/// exactly this set, witnessed for equivalence in the layout tests.
pub(crate) const fn is_renderable_effect_id(effect_id: i32) -> bool {
    matches!(effect_id, 1..=5 | 8..=22 | 24..=30)
}

/// One retained authoritative status effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudEffect {
    pub effect_id: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub particles: bool,
    /// Server tick after which the effect is no longer presented. `None` is an
    /// effectively infinite (negative wire duration) effect.
    pub expires_at_tick: Option<u64>,
}

impl HudEffect {
    #[must_use]
    pub fn visible_at_tick(&self, now_tick: Option<u64>) -> bool {
        match (self.expires_at_tick, now_tick) {
            (None, _) => true,
            // Without a server clock the effect stays visible until removed.
            (Some(_), None) => true,
            (Some(expires), Some(now)) => now < expires,
        }
    }

    /// Remaining whole seconds, used for the Java expiry blink.
    #[must_use]
    pub fn remaining_ticks(&self, now_tick: Option<u64>) -> Option<u64> {
        match (self.expires_at_tick, now_tick) {
            (Some(expires), Some(now)) => Some(expires.saturating_sub(now)),
            _ => None,
        }
    }
}

/// Heart row recolor derived from authoritative effects and freezing state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeartVariant {
    #[default]
    Normal,
    Poisoned,
    Withered,
    Frozen,
}

/// The local player's authoritative armor stacks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArmorSlots {
    pub helmet: NetworkItemStack,
    pub chestplate: NetworkItemStack,
    pub leggings: NetworkItemStack,
    pub boots: NetworkItemStack,
    pub body: NetworkItemStack,
}

/// Bounded diagnostics for skipped/odd gameplay-HUD data. Odd remote values
/// are counted and dropped; they never end the session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameplayHudDiagnostics {
    pub skipped_effect_actions: u64,
    pub evicted_effects: u64,
    pub odd_metadata_values: u64,
    pub dropped_inventory_events: u64,
    /// Well-formed attribute updates whose values were semantically odd
    /// (non-finite, inverted range); the field was skipped, never fatal.
    pub odd_attribute_values: u64,
    /// Well-formed HUD/game-mode packets carrying semantically odd values
    /// (negative SetHealth, negative title durations, unknown modes).
    pub odd_hud_packets: u64,
    /// Server chat rows beyond the retention byte bound, skipped whole.
    pub oversized_chat_rows: u64,
    /// Effect ids outside the pinned renderable table, skipped so they can
    /// never evict a renderable effect from the bounded list.
    pub unknown_effect_ids: u64,
}

/// App-owned retained gameplay HUD state fed exclusively by committed
/// authoritative events.
#[derive(Clone, Debug, Default)]
pub struct GameplayHudState {
    hotbar: [Option<NetworkItemStack>; HOTBAR_SLOT_COUNT as usize],
    hotbar_known: bool,
    offhand: Option<NetworkItemStack>,
    armor: Option<ArmorSlots>,
    effects: Vec<HudEffect>,
    air_supply_ticks: Option<i16>,
    max_air_supply_ticks: Option<i16>,
    freezing_strength: f32,
    mount_unique_id: Option<i64>,
    diagnostics: GameplayHudDiagnostics,
}

impl GameplayHudState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub const fn diagnostics(&self) -> GameplayHudDiagnostics {
        self.diagnostics
    }

    /// The authoritative hotbar stack for a slot, if inventory content has
    /// arrived. Empty stacks read as `None`.
    #[must_use]
    pub fn hotbar_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.hotbar
            .get(usize::from(slot))?
            .as_ref()
            .filter(|stack| !stack.is_empty())
    }

    #[must_use]
    pub const fn hotbar_known(&self) -> bool {
        self.hotbar_known
    }

    #[must_use]
    pub fn offhand_stack(&self) -> Option<&NetworkItemStack> {
        self.offhand.as_ref().filter(|stack| !stack.is_empty())
    }

    #[must_use]
    pub const fn armor(&self) -> Option<&ArmorSlots> {
        self.armor.as_ref()
    }

    #[must_use]
    pub fn effects(&self) -> &[HudEffect] {
        &self.effects
    }

    /// Air `(current, maximum)` in ticks when both authoritative values are
    /// known and coherent.
    #[must_use]
    pub fn air_ticks(&self) -> Option<(u16, u16)> {
        let current = self.air_supply_ticks?;
        let maximum = self.max_air_supply_ticks?;
        if maximum <= 0 {
            return None;
        }
        let maximum = maximum as u16;
        let current = current.clamp(0, maximum as i16) as u16;
        Some((current, maximum))
    }

    #[cfg(test)]
    #[must_use]
    pub const fn freezing_strength(&self) -> f32 {
        self.freezing_strength
    }

    #[must_use]
    pub const fn mount_unique_id(&self) -> Option<i64> {
        self.mount_unique_id
    }

    /// The heart recolor derived from authoritative effects and freezing.
    /// Freezing wins, then wither, then poison — matching the Java reference
    /// presentation priority for simultaneous states.
    #[must_use]
    pub fn heart_variant(&self, now_tick: Option<u64>) -> HeartVariant {
        if self.freezing_strength >= 1.0 {
            return HeartVariant::Frozen;
        }
        let mut variant = HeartVariant::Normal;
        for effect in &self.effects {
            if !effect.visible_at_tick(now_tick) {
                continue;
            }
            match effect.effect_id {
                EFFECT_ID_WITHER => return HeartVariant::Withered,
                EFFECT_ID_POISON | EFFECT_ID_FATAL_POISON => variant = HeartVariant::Poisoned,
                _ => {}
            }
        }
        variant
    }

    /// Whether the pinned hunger-effect recolor applies (Bedrock effect 17).
    #[must_use]
    pub fn hunger_effect_active(&self, now_tick: Option<u64>) -> bool {
        self.effects
            .iter()
            .any(|effect| effect.effect_id == 17 && effect.visible_at_tick(now_tick))
    }

    pub fn apply_effect(&mut self, event: ActorEffectEvent) {
        let expires_at_tick = if event.duration_ticks < 0 {
            None
        } else {
            Some(event.tick.saturating_add(event.duration_ticks as u64))
        };
        match event.action {
            ActorEffectAction::Add | ActorEffectAction::Update => {
                // An id outside the pinned renderable table is odd remote
                // data: counted and skipped, never stored, so it cannot evict
                // a renderable effect from the bounded list.
                if !is_renderable_effect_id(event.effect_id) {
                    self.diagnostics.unknown_effect_ids =
                        self.diagnostics.unknown_effect_ids.saturating_add(1);
                    return;
                }
                if let Some(existing) = self
                    .effects
                    .iter_mut()
                    .find(|effect| effect.effect_id == event.effect_id)
                {
                    existing.amplifier = event.amplifier;
                    existing.ambient = event.ambient;
                    existing.particles = event.particles;
                    existing.expires_at_tick = expires_at_tick;
                    return;
                }
                if self.effects.len() >= MAX_HUD_EFFECTS {
                    // Bounded retention: evict the soonest-expiring effect so a
                    // hostile stream cannot grow the list.
                    let Some(evict) = self
                        .effects
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, effect)| effect.expires_at_tick.unwrap_or(u64::MAX))
                        .map(|(index, _)| index)
                    else {
                        return;
                    };
                    self.effects.swap_remove(evict);
                    self.diagnostics.evicted_effects =
                        self.diagnostics.evicted_effects.saturating_add(1);
                }
                self.effects.push(HudEffect {
                    effect_id: event.effect_id,
                    amplifier: event.amplifier,
                    ambient: event.ambient,
                    particles: event.particles,
                    expires_at_tick,
                });
            }
            ActorEffectAction::Remove => {
                self.effects
                    .retain(|effect| effect.effect_id != event.effect_id);
            }
            ActorEffectAction::Unknown(_) => {
                self.diagnostics.skipped_effect_actions =
                    self.diagnostics.skipped_effect_actions.saturating_add(1);
            }
        }
    }

    /// Drops effects that expired on the authoritative clock. The vanilla
    /// client counts durations down locally; the server's Remove stays the
    /// final authority for early clears.
    pub fn expire_effects(&mut self, now_tick: Option<u64>) {
        let Some(now) = now_tick else { return };
        self.effects
            .retain(|effect| effect.visible_at_tick(Some(now)));
    }

    pub fn apply_armor(&mut self, event: &ArmorEquipmentEvent) {
        self.armor = Some(ArmorSlots {
            helmet: event.helmet.clone(),
            chestplate: event.chestplate.clone(),
            leggings: event.leggings.clone(),
            boots: event.boots.clone(),
            body: event.body.clone(),
        });
    }

    /// Counts one semantically odd attribute value that was skipped.
    pub fn note_odd_attribute(&mut self) {
        self.diagnostics.odd_attribute_values =
            self.diagnostics.odd_attribute_values.saturating_add(1);
    }

    /// Counts one semantically odd HUD/game-mode packet that was skipped.
    pub fn note_odd_hud_packet(&mut self) {
        self.diagnostics.odd_hud_packets = self.diagnostics.odd_hud_packets.saturating_add(1);
    }

    /// Counts one oversized server chat row that was skipped whole.
    pub fn note_oversized_chat_row(&mut self) {
        self.diagnostics.oversized_chat_rows =
            self.diagnostics.oversized_chat_rows.saturating_add(1);
    }

    pub fn apply_metadata(&mut self, metadata: &[ActorMetadata]) {
        for entry in metadata {
            match (entry.key, &entry.value) {
                (METADATA_KEY_AIR_SUPPLY, ActorMetadataValue::Short(value)) => {
                    self.air_supply_ticks = Some(*value);
                }
                (METADATA_KEY_MAX_AIR_SUPPLY, ActorMetadataValue::Short(value)) => {
                    if *value > 0 {
                        self.max_air_supply_ticks = Some(*value);
                    } else {
                        self.diagnostics.odd_metadata_values =
                            self.diagnostics.odd_metadata_values.saturating_add(1);
                    }
                }
                (METADATA_KEY_FREEZING_EFFECT_STRENGTH, ActorMetadataValue::Float(value)) => {
                    if value.is_finite() {
                        self.freezing_strength = value.clamp(0.0, 1.0);
                    } else {
                        self.diagnostics.odd_metadata_values =
                            self.diagnostics.odd_metadata_values.saturating_add(1);
                    }
                }
                (
                    METADATA_KEY_AIR_SUPPLY
                    | METADATA_KEY_MAX_AIR_SUPPLY
                    | METADATA_KEY_FREEZING_EFFECT_STRENGTH,
                    _,
                ) => {
                    // A known key with an unexpected wire type is odd remote
                    // data: count it and keep the previous value.
                    self.diagnostics.odd_metadata_values =
                        self.diagnostics.odd_metadata_values.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    pub fn set_mount(&mut self, ridden_unique_id: Option<i64>) {
        self.mount_unique_id = ridden_unique_id;
    }

    /// Routes one local MobEquipment echo. Left-hand events carry the offhand
    /// stack; the main-hand slot echo is retained by the caller.
    pub fn apply_offhand_equipment(&mut self, event: &EquipmentEvent) -> bool {
        if event.handedness != Some(ActorHandedness::Left) {
            return false;
        }
        self.offhand = Some(event.stack.clone());
        true
    }

    /// Applies one committed inventory event to the retained hotbar/offhand
    /// mirror. Container-UI events (open/close/response/data) are dropped and
    /// counted until the Phase 5.5 container store takes over this drain.
    pub fn apply_inventory(&mut self, event: &InventoryEvent) {
        match event {
            InventoryEvent::Content(content) => match content.container.window_id {
                Some(0) => {
                    for slot in 0..usize::from(HOTBAR_SLOT_COUNT) {
                        self.hotbar[slot] = content.slots.get(slot).cloned();
                    }
                    self.hotbar_known = true;
                }
                Some(119) => {
                    self.offhand = content.slots.first().cloned();
                }
                _ => {
                    self.diagnostics.dropped_inventory_events =
                        self.diagnostics.dropped_inventory_events.saturating_add(1);
                }
            },
            InventoryEvent::Slot(slot_event) => {
                let slot = usize::from(slot_event.identity.slot);
                match slot_event.identity.container.window_id {
                    Some(0) if slot < usize::from(HOTBAR_SLOT_COUNT) => {
                        self.hotbar[slot] = Some(slot_event.stack.clone());
                        self.hotbar_known = true;
                    }
                    Some(0) => {}
                    Some(119) if slot == 0 => {
                        self.offhand = Some(slot_event.stack.clone());
                    }
                    _ => {
                        self.diagnostics.dropped_inventory_events =
                            self.diagnostics.dropped_inventory_events.saturating_add(1);
                    }
                }
            }
            // SelectedSlot is consumed by the caller's slot-precedence logic;
            // the remaining container events are not modeled yet.
            InventoryEvent::SelectedSlot(_)
            | InventoryEvent::Authority(_)
            | InventoryEvent::Response(_)
            | InventoryEvent::Open(_)
            | InventoryEvent::Close(_)
            | InventoryEvent::Data(_) => {
                self.diagnostics.dropped_inventory_events =
                    self.diagnostics.dropped_inventory_events.saturating_add(1);
            }
        }
    }
}
