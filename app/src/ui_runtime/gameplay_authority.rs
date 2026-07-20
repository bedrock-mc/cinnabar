//! Local gameplay authority on [`UiRuntime`]: hotbar-slot precedence, the
//! retained gameplay-HUD event appliers, and the per-frame inventory drain.
//! Split from the runtime root to honor the production line budget.

use std::sync::Arc;

use protocol::{ActorEffectEvent, ActorMetadata, ArmorEquipmentEvent, InventoryEvent};
use ui::BoundedStat;

use super::{GameplayHudState, SequencedLocalAttributes, UiRuntime, UiRuntimeError, hud_adapter};

/// Bedrock's fixed wire cadence: 20 server ticks per second.
const MILLIS_PER_SERVER_TICK: u64 = 50;

/// The reference charges the mount jump bar from empty to full over half a
/// second of held jump input; pinned in milliseconds as a bounded recorded
/// approximation pending the native comparison gallery.
const MOUNT_JUMP_CHARGE_FULL_MILLIS: u64 = 500;

impl UiRuntime {
    /// Installs an explicit authoritative game mode. Stats are never
    /// fabricated or cleared here: attributes remain the only stat authority,
    /// and visibility is a pure presentation gate on the mode. Production
    /// bootstrap goes through [`Self::publish_bootstrap_game_modes`]; the
    /// witnesses drive this directly.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn publish_player_game_mode(&mut self, game_mode: protocol::PlayerGameMode) {
        self.player_game_mode = Some(game_mode);
        self.player_mode_from_default = false;
    }

    /// Installs the StartGame game modes: the resolved player mode, the
    /// world's default mode, and whether the player is bound to that default
    /// (StartGame carried the level-default sentinel).
    pub(crate) fn publish_bootstrap_game_modes(
        &mut self,
        player: protocol::PlayerGameMode,
        world_default: protocol::PlayerGameMode,
        player_uses_world_default: bool,
    ) {
        self.player_game_mode = Some(player);
        self.world_default_game_mode = Some(world_default);
        self.player_mode_from_default = player_uses_world_default;
    }

    pub(super) fn apply_game_mode_update(
        &mut self,
        update: protocol::GameModeUpdate,
    ) -> super::UiApplyOutcome {
        match update {
            protocol::GameModeUpdate::Explicit(mode) => {
                self.player_game_mode = Some(mode);
                self.player_mode_from_default = false;
                super::UiApplyOutcome::Applied
            }
            protocol::GameModeUpdate::WorldDefault => match self.world_default_game_mode {
                Some(default) => {
                    self.player_game_mode = Some(default);
                    self.player_mode_from_default = true;
                    super::UiApplyOutcome::Applied
                }
                // No retained default to resolve against: keep the current
                // authoritative mode and count the skip.
                None => {
                    self.gameplay_hud.note_odd_hud_packet();
                    super::UiApplyOutcome::IgnoredByReceiveStore
                }
            },
            protocol::GameModeUpdate::Unknown(_) => {
                self.gameplay_hud.note_odd_hud_packet();
                super::UiApplyOutcome::IgnoredByReceiveStore
            }
        }
    }

    pub(super) fn apply_default_game_mode_update(
        &mut self,
        update: protocol::GameModeUpdate,
    ) -> super::UiApplyOutcome {
        match update {
            protocol::GameModeUpdate::Explicit(mode) => {
                self.world_default_game_mode = Some(mode);
                if self.player_mode_from_default {
                    self.player_game_mode = Some(mode);
                }
                super::UiApplyOutcome::Applied
            }
            // A default-of-default or unknown default is odd; keep state.
            protocol::GameModeUpdate::WorldDefault | protocol::GameModeUpdate::Unknown(_) => {
                self.gameplay_hud.note_odd_hud_packet();
                super::UiApplyOutcome::IgnoredByReceiveStore
            }
        }
    }

    pub(crate) const fn player_game_mode(&self) -> Option<protocol::PlayerGameMode> {
        self.player_game_mode
    }

    pub(crate) const fn survival_stats_visible(&self) -> bool {
        match self.player_game_mode {
            Some(game_mode) => game_mode.shows_survival_stats(),
            None => true,
        }
    }

    pub(crate) fn selected_hotbar_slot(&self) -> Option<u8> {
        // Local selection is client-authoritative in Bedrock: once the player picks a slot
        // (number key / scroll / controller) that prediction wins over the server-echoed
        // equipment slot. A server PlayerHotbar with select_slot clears the local
        // prediction when it drains, so it takes effect at its FIFO position.
        self.local_selected_slot
            .or(self.server_selected_slot)
            .or_else(|| {
                self.local_selected_equipment
                    .as_ref()
                    .map(|equipment| equipment.event.selected_slot)
            })
            .or_else(|| {
                self.player_game_mode
                    .filter(|game_mode| game_mode.shows_hotbar())
                    .map(|_| 0)
            })
    }

    /// The authoritative stack in the selected hotbar slot: inventory content
    /// when known, otherwise the last main-hand MobEquipment echo for the same
    /// slot. `None` when the slot is empty or contents are unknown.
    pub(crate) fn selected_stack(&self) -> Option<&protocol::NetworkItemStack> {
        let slot = self.selected_hotbar_slot()?;
        if self.gameplay_hud.hotbar_known() {
            return self.gameplay_hud.hotbar_stack(slot);
        }
        self.local_selected_equipment
            .as_ref()
            .filter(|equipment| equipment.event.selected_slot == slot)
            .map(|equipment| &equipment.event.stack)
            .filter(|stack| !stack.is_empty())
    }

    pub(crate) const fn gameplay_hud(&self) -> &GameplayHudState {
        &self.gameplay_hud
    }

    /// The stack presented in one hotbar cell: the authoritative inventory
    /// mirror when known, otherwise the MobEquipment echo for the selected
    /// slot — authoritative before any container content has arrived.
    pub(crate) fn presented_hotbar_stack(&self, slot: u8) -> Option<&protocol::NetworkItemStack> {
        self.gameplay_hud.hotbar_stack(slot).or_else(|| {
            (self.selected_hotbar_slot() == Some(slot))
                .then(|| self.selected_stack())
                .flatten()
        })
    }

    /// The estimated authoritative tick at `now_millis`: the last observed
    /// server tick advanced by the local millis elapsed since it was
    /// observed, at the fixed 20 tps wire cadence. This is the presentation
    /// clock for effect expiry and blink phases, so finite durations keep
    /// counting down during quiet sessions with no new packets; a server
    /// Remove stays the final authority for early clears.
    pub(crate) fn estimated_server_tick(&self, now_millis: u64) -> Option<u64> {
        let tick = self.last_server_tick?;
        let observed = self.last_tick_observed_millis?;
        Some(tick.saturating_add(now_millis.saturating_sub(observed) / MILLIS_PER_SERVER_TICK))
    }

    /// Drops effects that expired on the estimated session clock.
    pub(crate) fn expire_gameplay_effects(&mut self, now_millis: u64) {
        let now_tick = self.estimated_server_tick(now_millis);
        self.gameplay_hud.expire_effects(now_tick);
    }

    /// Observes the held state of the jump action for the mount jump-charge
    /// ramp. Holding jump while mounted starts the charge clock; releasing
    /// it, or losing the mount, resets the charge to empty.
    pub(crate) fn set_mount_jump_held(&mut self, held: bool, now_millis: u64) {
        if !held || self.gameplay_hud.mount_unique_id().is_none() {
            self.mount_jump_hold_started_millis = None;
            return;
        }
        if self.mount_jump_hold_started_millis.is_none() {
            self.mount_jump_hold_started_millis = Some(now_millis);
        }
    }

    /// The current jump charge in `0.0..=1.0`: a linear ramp over the pinned
    /// hold window, zero while jump is not held.
    pub(crate) fn mount_jump_charge(&self, now_millis: u64) -> f32 {
        let Some(started) = self.mount_jump_hold_started_millis else {
            return 0.0;
        };
        let elapsed = now_millis.saturating_sub(started);
        (elapsed as f32 / MOUNT_JUMP_CHARGE_FULL_MILLIS as f32).clamp(0.0, 1.0)
    }

    /// Publishes the armor bar derived from the authoritative equipped armor
    /// identifiers. `None` means armor equipment is unknown, which clears the
    /// row (fail closed) rather than retaining a stale value.
    pub(crate) fn set_derived_armor(&mut self, points: Option<u16>) {
        let armor = points.and_then(|points| BoundedStat::new(points.min(20), 20));
        self.hud
            .set_stats(self.hud.health(), self.hud.hunger(), armor, self.hud.air());
    }

    /// Millis timestamp of the last authoritative health decrease, for the
    /// Java-style damage heart blink.
    pub(crate) const fn last_health_drop_millis(&self) -> Option<u64> {
        self.last_health_drop_millis
    }

    /// Millis timestamp when the selected stack's item identity last changed,
    /// for the Java-style selected-item label fade.
    pub(crate) const fn selected_item_changed_millis(&self) -> Option<u64> {
        self.last_selected_identity_change_millis
    }

    /// Refreshes the selected-item identity clock. Runs before presentation so
    /// the label timer starts when the authoritative selection (slot or
    /// contents) changes, exactly like the Java reference behavior.
    pub(crate) fn observe_selected_item_identity(&mut self, now_millis: u64) {
        let identity = self
            .selected_stack()
            .map(|stack| (stack.network_id, stack.metadata));
        if identity != self.last_selected_identity {
            self.last_selected_identity = identity;
            self.last_selected_identity_change_millis = if identity.is_some() {
                Some(now_millis)
            } else {
                None
            };
        }
    }

    /// Applies every queued authoritative inventory event to the retained
    /// hotbar/offhand mirror. Runs once per frame before presentation; the
    /// queue would otherwise grow without a consumer until the Phase 5.5
    /// container store takes over this drain.
    pub(crate) fn drain_pending_inventory(&mut self) {
        while let Some(sequenced) = self.pending_inventory.pop_front() {
            if let InventoryEvent::SelectedSlot(selected) = &sequenced.event
                && selected.select_slot
                && selected.slot < protocol::HOTBAR_SLOT_COUNT
            {
                // A server-forced selection overrides the local prediction at
                // its FIFO position; later local input re-predicts as usual.
                self.server_selected_slot = Some(selected.slot);
                self.local_selected_slot = None;
            }
            self.gameplay_hud.apply_inventory(&sequenced.event);
        }
    }

    pub fn apply_local_attributes(
        &mut self,
        envelope: SequencedLocalAttributes,
    ) -> Result<(), UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            Some(envelope.server_tick),
        )?;
        let mut health = self.hud.health();
        let mut hunger = self.hud.hunger();
        let mut absorption = self.hud.absorption();
        let mut xp_level = self.hud.experience().map(|xp| xp.level);
        let mut xp_progress = self.hud.experience().map(|xp| xp.progress);
        for attribute in envelope.attributes.iter() {
            match attribute.name.as_ref() {
                // A semantically odd value (non-finite, inverted range) in a
                // well-formed attribute skips that field, counted, keeping the
                // previous authoritative value and the session alive.
                "minecraft:health" => match hud_adapter::attribute_stat(attribute) {
                    Some(stat) => health = Some(stat),
                    None => self.gameplay_hud.note_odd_attribute(),
                },
                "minecraft:player.hunger" => match hud_adapter::attribute_stat(attribute) {
                    Some(stat) => hunger = Some(stat),
                    None => self.gameplay_hud.note_odd_attribute(),
                },
                // Absorption is an ordinary bounded attribute; zero is common
                // and simply hides the golden hearts.
                "minecraft:absorption" => {
                    absorption = hud_adapter::attribute_stat(attribute);
                }
                // Bedrock sends experience as attributes, not a dedicated packet: progress in
                // 0.0..=1.0 and an integer level. `f32 as u32` saturates, so a stray value is bounded.
                "minecraft:player.experience" if attribute.current.is_finite() => {
                    xp_progress = Some(attribute.current);
                }
                "minecraft:player.level" if attribute.current.is_finite() => {
                    xp_level = Some(attribute.current.max(0.0) as u32);
                }
                _ => {}
            }
        }
        // An authoritative health decrease drives the Java-style damage blink.
        if let (Some(previous), Some(next)) = (self.hud.health(), health)
            && u32::from(next.current()) * u32::from(previous.scale())
                < u32::from(previous.current()) * u32::from(next.scale())
        {
            self.last_health_drop_millis = Some(envelope.local_millis);
        }
        self.hud
            .set_stats(health, hunger, self.hud.armor(), self.hud.air());
        self.hud.set_absorption(absorption);
        if xp_level.is_some() || xp_progress.is_some() {
            self.hud
                .set_experience(xp_level.unwrap_or(0), xp_progress.unwrap_or(0.0));
        }
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        self.last_server_tick = Some(envelope.server_tick);
        self.last_tick_observed_millis = Some(envelope.local_millis);
        Ok(())
    }

    /// Applies a committed local-player SetEntityData batch (air supply,
    /// freezing strength). Odd values are counted and skipped inside the
    /// gameplay-HUD state; they never fail the session.
    pub fn apply_local_metadata(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        metadata: &[ActorMetadata],
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        self.gameplay_hud.apply_metadata(metadata);
        if let Some((current, maximum)) = self.gameplay_hud.air_ticks() {
            self.hud.set_air(BoundedStat::new(current, maximum));
        }
        self.last_fifo_sequence = Some(fifo_sequence);
        Ok(())
    }

    /// Applies a committed local-player MobEffect change. `local_millis`
    /// anchors the event's server tick to the session clock so finite
    /// durations expire without further packets.
    pub fn apply_local_effect(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        event: ActorEffectEvent,
        local_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        let event_tick = event.tick;
        self.gameplay_hud.apply_effect(event);
        self.last_fifo_sequence = Some(fifo_sequence);
        if event_tick >= self.last_server_tick.unwrap_or(0) {
            self.last_server_tick = Some(event_tick);
            self.last_tick_observed_millis = Some(local_millis);
        }
        Ok(())
    }

    /// Applies the committed local-player MobArmorEquipment stacks.
    pub fn apply_local_armor(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        event: &ArmorEquipmentEvent,
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        self.gameplay_hud.apply_armor(event);
        self.last_fifo_sequence = Some(fifo_sequence);
        Ok(())
    }

    /// Applies the committed local mount change from SetActorLink.
    pub fn apply_local_mount(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        ridden_unique_id: Option<i64>,
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        self.gameplay_hud.set_mount(ridden_unique_id);
        self.last_fifo_sequence = Some(fifo_sequence);
        Ok(())
    }

    fn guard_local_apply(&self, session_id: u64, fifo_sequence: u64) -> Result<(), UiRuntimeError> {
        if session_id != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: session_id,
            });
        }
        if let Some(previous) = self.last_fifo_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        Ok(())
    }
}

impl UiRuntime {
    /// Installs the startup-loaded localization catalog used for rawtext
    /// translation and item display names.
    pub fn set_lang_catalog(&mut self, catalog: Arc<assets::RuntimeLangCatalog>) {
        self.lang_catalog = Some(catalog);
    }

    /// The localized display name for a vanilla item identifier: the pinned
    /// `item.<path>.name` / `tile.<path>.name` translation when present,
    /// otherwise the mechanical title-cased identifier.
    pub(crate) fn localized_item_name(&self, identifier: &str) -> String {
        if let Some(catalog) = self.lang_catalog.as_ref() {
            let path = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
            for key in [format!("item.{path}.name"), format!("tile.{path}.name")] {
                if let Some(value) = catalog.lookup(&key) {
                    return value.as_ref().to_owned();
                }
            }
        }
        super::item_facts::mechanical_display_name(identifier)
    }
}
