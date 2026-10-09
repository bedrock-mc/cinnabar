//! Local gameplay authority on [`UiRuntime`]: hotbar-slot precedence, the
//! retained gameplay-HUD event appliers, and the per-frame inventory drain.
//! Split from the runtime root to honor the production line budget.

use std::sync::Arc;

use protocol::{
    ActorEffectEvent, ActorMetadata, CanonicalCell, InventoryEvent, project_container_cell,
};
use ui::BoundedStat;

use super::{GameplayHudState, SequencedLocalAttributes, UiRuntime, UiRuntimeError, hud_adapter};

pub use inventory::SelectedStackSnapshot;

/// Bedrock's fixed wire cadence: 20 server ticks per second.
const MILLIS_PER_SERVER_TICK: u64 = 50;

/// The reference charges the mount jump bar from empty to full over half a
/// second of held jump input; pinned in milliseconds as a bounded recorded
/// approximation pending the native comparison gallery.
const MOUNT_JUMP_CHARGE_FULL_MILLIS: u64 = 500;

impl UiRuntime {
    pub fn set_hardcore(&mut self, hardcore: bool) {
        self.gameplay_hud.set_hardcore(hardcore);
    }

    pub fn apply_hud_rules(&mut self, rules: protocol::HudRules) {
        self.gameplay_hud.apply_hud_rules(rules);
    }

    pub(super) fn apply_game_mode_update(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        update: protocol::GameModeUpdate,
    ) -> super::UiApplyOutcome {
        if player_runtime.facts.apply_game_mode_update(update) {
            super::UiApplyOutcome::Applied
        } else {
            self.gameplay_hud.note_odd_hud_packet();
            super::UiApplyOutcome::IgnoredByReceiveStore
        }
    }

    pub(super) fn apply_default_game_mode_update(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        update: protocol::GameModeUpdate,
    ) -> super::UiApplyOutcome {
        if player_runtime.facts.apply_default_game_mode_update(update) {
            super::UiApplyOutcome::Applied
        } else {
            self.gameplay_hud.note_odd_hud_packet();
            super::UiApplyOutcome::IgnoredByReceiveStore
        }
    }

    pub const fn gameplay_hud(&self) -> &GameplayHudState {
        &self.gameplay_hud
    }

    /// The presentation tick at `now_millis`, advancing at 20 tps from its real-time anchor.
    /// Equal or lagging packet ticks never rewind this clock or discard fractional elapsed time.
    /// Effect durations start when received; a server Remove remains authoritative for early clears.
    pub fn estimated_server_tick(&self, now_millis: u64) -> Option<u64> {
        let (tick, observed) = self.presentation_tick_anchor?;
        Some(tick.saturating_add(now_millis.saturating_sub(observed) / MILLIS_PER_SERVER_TICK))
    }

    /// Accepts a forward clock observation while keeping packet-order fences independent.
    pub(super) fn observe_presentation_tick(&mut self, tick: u64, now_millis: u64) -> u64 {
        let estimated = self.estimated_server_tick(now_millis);
        if estimated.is_none_or(|estimated| tick > estimated) {
            self.presentation_tick_anchor = Some((tick, now_millis));
        }
        self.estimated_server_tick(now_millis).unwrap()
    }

    /// Drops effects that expired on the estimated session clock.
    pub fn expire_gameplay_effects(&mut self, now_millis: u64) {
        let now_tick = self.estimated_server_tick(now_millis);
        self.gameplay_hud.expire_effects(now_tick);
    }

    /// Observes the held state of the jump action for the mount jump-charge
    /// ramp. Holding jump while mounted starts the charge clock; releasing
    /// it, or losing the mount, resets the charge to empty.
    pub fn set_mount_jump_held(&mut self, held: bool, now_millis: u64) {
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
    pub fn mount_jump_charge(&self, now_millis: u64) -> f32 {
        let Some(started) = self.mount_jump_hold_started_millis else {
            return 0.0;
        };
        let elapsed = now_millis.saturating_sub(started);
        (elapsed as f32 / MOUNT_JUMP_CHARGE_FULL_MILLIS as f32).clamp(0.0, 1.0)
    }

    /// Publishes the armor bar derived from the authoritative equipped armor
    /// identifiers. `None` means armor equipment is unknown, which clears the
    /// row (fail closed) rather than retaining a stale value.
    pub fn set_derived_armor(&mut self, points: Option<u16>) {
        let armor = points.and_then(|points| BoundedStat::new(points.min(20), 20));
        self.hud
            .set_stats(self.hud.health(), self.hud.hunger(), armor, self.hud.air());
    }

    /// Current local actor damage state, absent when its session has no player actor.
    pub const fn local_actor_damage(&self) -> Option<client_world::ActorDamageState> {
        self.local_actor_damage
    }

    /// Publishes the current local actor's damage snapshot after its completed ticks.
    pub fn publish_local_actor_damage(&mut self, damage: Option<client_world::ActorDamageState>) {
        self.local_actor_damage = damage;
    }

    /// Projects health from its actor attribute range alongside the completed damage countdown.
    pub fn publish_local_actor_health(&mut self, actor: Option<&client_world::ActorSnapshot>) {
        self.publish_local_actor_damage(actor.map(|actor| actor.status.damage));
        if let Some((attribute, health)) = actor
            .and_then(|actor| actor.attributes.get("minecraft:health"))
            .and_then(|attribute| {
                hud_adapter::attribute_stat(attribute).map(|health| (attribute, health))
            })
        {
            self.publish_local_player_alive(attribute.current > 0.0);
            self.hud.set_health(Some(health));
        } else if actor.is_none() {
            self.local_player_alive = None;
        }
    }

    /// Millis timestamp when the selected slot or item identity last changed,
    /// for the selected-item label fade.
    pub const fn selected_item_changed_millis(&self) -> Option<u64> {
        self.last_selected_identity_change_millis
    }

    /// Refreshes the selected-item identity clock. Runs before presentation so
    /// the label timer starts when the selection or item identity changes.
    /// Bedrock's HUD tick notices slot changes even between identical items.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn observe_selected_item_identity(
        &mut self,
        player_runtime: &player_state::PlayerState,
        now_millis: u64,
    ) {
        let identity = player_runtime
            .selected_stack_snapshot()
            .and_then(|snapshot| match snapshot.state {
                super::inventory_ledger::PlayerInventorySlot::Present(stack) => {
                    Some((snapshot.slot, stack.network_id, stack.metadata))
                }
                _ => None,
            });
        self.observe_selected_item_identity_value(identity, now_millis);
    }

    /// Updates the selected-item clock from an already-sampled frame identity.
    pub fn observe_selected_item_identity_value(
        &mut self,
        identity: Option<(u8, i32, u32)>,
        now_millis: u64,
    ) {
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
    pub fn drain_pending_inventory(&mut self, player_runtime: &mut player_state::PlayerState) {
        while let Some(sequenced) = player_runtime.inventory.apply_next() {
            let event = match sequenced.event {
                super::InventoryAuthorityEvent::Inventory(event) => event,
                super::InventoryAuthorityEvent::Registry(registry) => {
                    self.observe_use_on_identity(
                        player_runtime,
                        sequenced.session_generation,
                        sequenced.fifo_sequence,
                        &super::InventoryAuthorityEvent::Registry(registry),
                    );
                    continue;
                }
            };
            match &event {
                InventoryEvent::Authority(_) => {
                    self.inventory_open = player_runtime
                        .inventory
                        .ledger()
                        .personal_inventory_desired_open()
                        || player_runtime
                            .inventory
                            .ledger()
                            .storage_generation()
                            .is_some();
                }
                InventoryEvent::Open(_) => {
                    self.inventory_open = player_runtime
                        .inventory
                        .ledger()
                        .personal_inventory_desired_open()
                        || player_runtime
                            .inventory
                            .ledger()
                            .storage_generation()
                            .is_some();
                    if self.inventory_open {
                        self.chat_focused = false;
                    }
                }
                InventoryEvent::Content(content)
                    if matches!(
                        project_container_cell(&content.container, 0),
                        Some(CanonicalCell::GenericStorage { .. })
                    ) =>
                {
                    self.inventory_open = player_runtime
                        .inventory
                        .ledger()
                        .personal_inventory_desired_open()
                        || player_runtime
                            .inventory
                            .ledger()
                            .storage_slot_count()
                            .is_some();
                    if self.inventory_open {
                        self.chat_focused = false;
                    }
                }
                InventoryEvent::Close(_) => {
                    self.inventory_open = player_runtime
                        .inventory
                        .ledger()
                        .personal_inventory_desired_open()
                        || player_runtime
                            .inventory
                            .ledger()
                            .storage_generation()
                            .is_some();
                }
                _ => {}
            }
            self.gameplay_hud.apply_inventory(&event);
            self.observe_use_on_identity(
                player_runtime,
                sequenced.session_generation,
                sequenced.fifo_sequence,
                &super::InventoryAuthorityEvent::Inventory(event),
            );
        }
        player_runtime.inventory.finish_drain();
        self.sample_crafting_observation(player_runtime);
    }

    pub fn apply_local_attributes(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
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
                    Some(stat) => {
                        self.publish_local_player_alive(attribute.current > 0.0);
                        health = Some(stat);
                    }
                    None => self.gameplay_hud.note_odd_attribute(),
                },
                "minecraft:player.hunger" => {
                    if player_runtime.facts.apply_hunger_attribute(attribute) {
                        hunger = player_runtime.facts.hunger().and_then(|stat| {
                            BoundedStat::new_scaled(stat.current(), stat.maximum(), stat.scale())
                        });
                    } else {
                        self.gameplay_hud.note_odd_attribute();
                    }
                }
                "minecraft:player.saturation" => {
                    self.gameplay_hud.set_saturation(attribute.current);
                }
                // The native heart renderer reads current absorption, independently
                // of the attribute range; zero clears the golden hearts.
                "minecraft:absorption" => match hud_adapter::attribute_stat(attribute) {
                    Some(stat) => absorption = Some(stat),
                    None => self.gameplay_hud.note_odd_attribute(),
                },
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
        // An authoritative health decrease can dismiss screens that close when hurt.
        if let (Some(previous), Some(next)) = (self.hud.health(), health)
            && u32::from(next.current()) * u32::from(previous.scale())
                < u32::from(previous.current()) * u32::from(next.scale())
        {
            self.note_player_hurt();
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
        self.observe_presentation_tick(envelope.server_tick, envelope.local_millis);
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

    /// Applies a committed local-player MobEffect change. Its duration starts at the
    /// receive-time presentation tick, including packets with absent or lagging wire ticks.
    pub fn apply_local_effect(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        mut event: ActorEffectEvent,
        local_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        let event_tick = event.tick;
        event.tick = self.observe_presentation_tick(event_tick, local_millis);
        self.gameplay_hud.apply_effect(event);
        self.last_fifo_sequence = Some(fifo_sequence);
        if event_tick >= self.last_server_tick.unwrap_or(0) {
            self.last_server_tick = Some(event_tick);
        }
        Ok(())
    }

    /// The local player's worn armor, helmet to boots, from its armor
    /// container as the inventory shows it.
    #[must_use]
    pub fn local_armor(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> super::gameplay_hud::ArmorSlots {
        let [helmet, chestplate, leggings, boots] = player_runtime.inventory.local_armor();
        super::gameplay_hud::ArmorSlots {
            helmet,
            chestplate,
            leggings,
            boots,
        }
    }

    /// Applies the committed local mount change from SetActorLink.
    pub fn apply_local_mount(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_id: u64,
        fifo_sequence: u64,
        ridden_unique_id: Option<i64>,
    ) -> Result<(), UiRuntimeError> {
        self.guard_local_apply(session_id, fifo_sequence)?;
        player_runtime.facts.set_mount(ridden_unique_id);
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

    /// The UI language's table, consulted before en_US.
    pub fn set_active_language(&mut self, catalog: Option<Arc<assets::RuntimeLangCatalog>>) {
        self.active_lang = catalog;
    }

    pub fn set_server_lang(&mut self, overlay: Option<Arc<assets::ServerLangOverlay>>) {
        self.server_lang = overlay;
    }

    pub fn set_session_icons(&mut self, icons: Option<Arc<super::presentation::SessionIcons>>) {
        self.session_icons = icons;
    }

    pub fn set_session_glyphs(
        &mut self,
        glyphs: Option<Arc<super::presentation::SessionGlyphSheets>>,
    ) {
        self.session_glyphs = glyphs;
    }

    pub fn session_glyphs(&self) -> Option<&Arc<super::presentation::SessionGlyphSheets>> {
        self.session_glyphs.as_ref()
    }

    pub fn session_icons(&self) -> Option<&Arc<super::presentation::SessionIcons>> {
        self.session_icons.as_ref()
    }

    pub fn set_session_items(
        &mut self,
        items: Option<Arc<super::item_facts::SessionItemComponents>>,
    ) {
        self.session_items = items;
    }

    /// The server's components for `identifier` this session.
    pub fn item_components(&self, identifier: &str) -> Option<&protocol::ItemComponents> {
        self.session_items.as_ref()?.get(identifier)
    }

    pub fn item_glint(&self, stack: &protocol::NetworkItemStack, identifier: &str) -> bool {
        super::item_facts::is_glint(stack, identifier, self.item_components(identifier))
    }

    /// A damageable item's maximum: the server's durability component, else the vanilla table.
    pub fn item_max_durability(&self, identifier: Option<&str>) -> Option<u32> {
        let identifier = identifier?;
        self.item_components(identifier)
            .and_then(|components| components.max_durability)
            .or_else(|| client_world::vanilla_max_durability(identifier))
    }

    pub fn set_server_ui(&mut self, pack: Option<Arc<super::presentation::ServerUiPack>>) {
        self.server_ui = pack;
    }

    pub fn server_ui(&self) -> Option<&Arc<super::presentation::ServerUiPack>> {
        self.server_ui.as_ref()
    }

    /// Identifies the language tables [`Self::translation`] reads; any
    /// replacement changes it, so text laid out before is measured again.
    pub fn text_generation(&self) -> [usize; 3] {
        fn address<T: ?Sized>(table: Option<&Arc<T>>) -> usize {
            table.map_or(0, |table| Arc::as_ptr(table).cast::<()>().addr())
        }
        [
            address(self.lang_catalog.as_ref()),
            address(self.active_lang.as_ref()),
            address(self.server_lang.as_ref()),
        ]
    }

    pub(super) fn translation(&self, key: &str) -> Option<Arc<str>> {
        translate(
            self.server_lang.as_deref(),
            self.active_lang.as_deref(),
            self.lang_catalog.as_deref(),
            key,
        )
    }

    /// The language tables [`Self::translation`] reads, detached for another thread.
    pub fn translator(&self) -> Translator {
        Translator {
            server: self.server_lang.clone(),
            active: self.active_lang.clone(),
            base: self.lang_catalog.clone(),
        }
    }

    /// The localized display name for a vanilla item identifier: the pinned
    /// `item.<path>.name` / `tile.<path>.name` translation when present,
    /// otherwise the mechanical title-cased identifier.
    pub fn localized_item_name(&self, identifier: &str) -> String {
        // A display_name component is a language key, shown literally when untranslated.
        if let Some(name) = self
            .item_components(identifier)
            .and_then(|components| components.display_name.as_deref())
        {
            return self
                .translation(name)
                .map_or_else(|| name.to_owned(), |text| text.as_ref().to_owned());
        }
        if self.server_lang.is_some() || self.lang_catalog.is_some() {
            let path = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
            for key in [format!("item.{path}.name"), format!("tile.{path}.name")] {
                if let Some(value) = self.translation(&key) {
                    return value.as_ref().to_owned();
                }
            }
        }
        super::item_facts::mechanical_display_name(identifier)
    }
}

/// A snapshot of the language tables translations read.
#[derive(Clone, Default)]
pub struct Translator {
    server: Option<Arc<assets::ServerLangOverlay>>,
    active: Option<Arc<assets::RuntimeLangCatalog>>,
    base: Option<Arc<assets::RuntimeLangCatalog>>,
}

impl Translator {
    pub fn lookup(&self, key: &str) -> Option<Arc<str>> {
        translate(
            self.server.as_deref(),
            self.active.as_deref(),
            self.base.as_deref(),
            key,
        )
    }
}

/// `key` from the server's overlay, else the active language, else the base catalog.
fn translate(
    server: Option<&assets::ServerLangOverlay>,
    active: Option<&assets::RuntimeLangCatalog>,
    base: Option<&assets::RuntimeLangCatalog>,
    key: &str,
) -> Option<Arc<str>> {
    server
        .and_then(|overlay| overlay.lookup(key))
        .map(Arc::from)
        .or_else(|| active.and_then(|active| active.lookup(key)))
        .or_else(|| base.and_then(|base| base.lookup(key)))
}
