//! Player mods' committed inventory requests, made as the vanilla creative catalog and recipe
//! book make them; the server checks each one.

use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use mod_host::InventoryRequest;

use super::ModRuntime;

pub(super) fn apply_inventory_requests(
    extension: Option<ResMut<ModRuntime>>,
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
    mut ui: ResMut<UiRuntimeResource>,
) {
    let Some(mut extension) = extension else {
        return;
    };
    for index in 0..extension.hosts.host_count() {
        for request in extension.hosts.host_mut(index).take_inventory_requests() {
            let result = match &request {
                InventoryRequest::CreativeTake {
                    identifier,
                    aux,
                    count,
                } => ui.mod_creative_take(&mut player_runtime, identifier, *aux, *count),
                InventoryRequest::FillRecipe { network_id } => {
                    ui.mod_fill_recipe(&mut player_runtime, *network_id)
                }
            };
            if let Err(error) = result {
                debug!(?request, ?error, "mod inventory request refused");
            }
        }
    }
}
