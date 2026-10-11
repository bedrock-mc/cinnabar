//! Bevy item-use authority and presentation observations.
use bevy::prelude::Resource;
use chunk_pipeline::WorldStream;
use client_ui::ui_runtime::UiRuntime;
use client_world::LocalItemUse;
use gameplay::item_use::AirUse;
use inventory::{crossbow_animation_frame, ranged_animation_frame};
use selection::selected_air_use_with_projectile;

pub mod selection;

/// Bevy resource adapter for the gameplay item_use owner.
#[derive(Resource, Debug, Default, Clone)]
pub struct ItemUseRuntime(gameplay::item_use::ItemUseRuntime);
impl std::ops::Deref for ItemUseRuntime {
    type Target = gameplay::item_use::ItemUseRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ItemUseRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ItemUseRuntime {
    /// Exposes accepted-use state to the existing presentation adapters.
    pub fn is_using(&self) -> bool {
        self.0.is_using()
    }

    /// Native attachables read remaining ticks, not the actor animation's elapsed seconds.
    pub fn render_input(
        &self,
        player_runtime: &client_state::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
        tick: u64,
        frame_alpha: f32,
    ) -> client_world::AttachableAnimationInput<'static> {
        let max_use_ticks = self.active_timing().map_or_else(
            // A crossbow's maximum use duration remains its charge duration
            // when loaded; Instant describes the next action, not that query.
            || match selected_air_use_with_projectile(player_runtime, stream, Some(None)) {
                Some(AirUse::Hold { max_ticks, .. }) => max_ticks,
                _ => 0,
            },
            |(_, max_ticks)| max_ticks,
        );
        let use_elapsed_ticks = self.active_timing().map(|(started_tick, max_ticks)| {
            tick.saturating_sub(started_tick).min(u64::from(max_ticks)) as u32
        });
        let selected = player_runtime
            .selected_stack()
            .and_then(|stack| stream.authority().canonical_item_stack(stack));
        let projectile = self
            .selected_projectile(player_runtime, ui)
            .unwrap_or_else(|| {
                selected
                    .as_ref()
                    .and_then(|item| item.charged_projectile.as_deref())
            });
        let charged = projectile.is_some();
        let animation_frame = if selected
            .as_ref()
            .and_then(|item| item.identifier.as_deref())
            == Some("minecraft:crossbow")
        {
            let firework = ui
                .gameplay_hud()
                .offhand_stack()
                .and_then(|stack| stream.authority().canonical_item_stack(stack))
                .is_some_and(|item| {
                    item.identifier.as_deref() == Some("minecraft:firework_rocket")
                });
            crossbow_animation_frame(use_elapsed_ticks, max_use_ticks, projectile, firework)
        } else {
            ranged_animation_frame(
                selected
                    .as_ref()
                    .and_then(|item| item.identifier.as_deref()),
                use_elapsed_ticks,
                max_use_ticks,
            )
        };
        client_world::AttachableAnimationInput {
            first_person: true,
            is_local_player: true,
            frame_alpha,
            use_elapsed_ticks,
            max_use_ticks,
            hand_charged: charged,
            animation_frame,
            ..Default::default()
        }
    }

    /// The same native frame used by the attachable, for a player-inventory cell.
    pub fn inventory_animation_frame(
        &self,
        player_runtime: &client_state::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
        slot: u8,
        tick: u64,
    ) -> Option<u32> {
        let stack = ui.inventory_ledger(player_runtime).displayed_stack(slot)?;
        let canonical = stream.authority().canonical_item_stack(stack)?;
        if canonical.identifier.as_deref() != Some("minecraft:crossbow") {
            return None;
        }
        if player_runtime.selected_hotbar_slot() == Some(slot) {
            return Some(
                self.render_input(player_runtime, stream, ui, tick, 0.0)
                    .animation_frame,
            );
        }
        let projectile = self
            .slot_projectile(player_runtime, ui, slot)
            .unwrap_or(canonical.charged_projectile.as_deref());
        Some(crossbow_animation_frame(None, 0, projectile, false))
    }

    /// The local rig's use flag: set while a use runs, cleared while a held-use item idles.
    pub fn local_item_use(
        &self,
        player_runtime: &client_state::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
    ) -> LocalItemUse {
        if self.is_using() {
            return LocalItemUse::Using;
        }
        match selected_air_use_with_projectile(
            player_runtime,
            stream,
            self.selected_projectile(player_runtime, ui),
        ) {
            Some(AirUse::Hold { .. } | AirUse::Instant) => LocalItemUse::Idle,
            Some(AirUse::Throw { .. }) | None => LocalItemUse::Unpredicted,
        }
    }
    /// Resolves a charge prediction against the currently selected authoritative stack.
    pub fn selected_projectile(
        &self,
        player_runtime: &client_state::PlayerRuntime,
        ui: &UiRuntime,
    ) -> Option<Option<&'static str>> {
        let slot = player_runtime.selected_hotbar_slot()?;
        self.predicted_projectile(
            slot,
            ui.inventory_ledger(player_runtime)
                .authoritative_slot_revision(slot)?,
            player_runtime.selected_stack()?,
        )
    }
    /// Resolves a charge prediction against an authoritative inventory slot.
    fn slot_projectile(
        &self,
        player_runtime: &client_state::PlayerRuntime,
        ui: &UiRuntime,
        slot: u8,
    ) -> Option<Option<&'static str>> {
        self.predicted_projectile(
            slot,
            ui.inventory_ledger(player_runtime)
                .authoritative_slot_revision(slot)?,
            ui.inventory_ledger(player_runtime).displayed_stack(slot)?,
        )
    }
}

#[cfg(test)]
mod crossbow_presentation_tests;
