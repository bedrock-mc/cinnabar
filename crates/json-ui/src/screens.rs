//! The screens the engine is allowed to draw, and the generic renderer for them;
//! [`render_screen`] refuses anything not on the allow-list.

use crate::bind::{BindState, DataSource, bind_stateful};
use crate::catalog::Catalog;
use crate::form::{CatalogLibrary, FormRender, finish};
use crate::layout::LayoutEnv;
use crate::state::ViewState;
use crate::{Context, ResolvedControl, resolve};

/// A rendered engine screen: bound tree, draw nodes, hit regions, scroll report.
pub type ScreenRender = FormRender;

/// Every `namespace.name` screen the engine renders.
pub const ENGINE_SCREENS: &[&str] = &[
    crate::hud::HUD_SCREEN,
    crate::hud::CROSSHAIR_SCREEN,
    "server_form.third_party_server_screen",
    "server_form.long_form",
    "server_form.custom_form",
    "popup_dialog.modal_dialog_popup",
    "rating_prompt.rating_prompt_screen",
    "crafting.inventory_screen",
    "crafting.crafting_screen",
    "chest.small_chest_screen",
    "chest.large_chest_screen",
    "chest.ender_chest_screen",
    "chest.barrel_screen",
    "chest.shulker_box_screen",
    "furnace.furnace_screen",
    "blast_furnace.blast_furnace_screen",
    "smoker.smoker_screen",
    "anvil.anvil_screen",
    "enchanting.enchanting_screen",
    "brewing_stand.brewing_stand_screen",
    "grindstone.grindstone_screen",
    "loom.loom_screen",
    "smithing_table.smithing_table_screen",
    "cartography.cartography_screen",
    "stonecutter.stonecutter_screen",
    "beacon.beacon_screen",
    "redstone.hopper_screen",
    "redstone.dispenser_screen",
    "redstone.dropper_screen",
    "redstone.crafter_screen",
    "horse.horse_screen",
    "book.book_screen",
    "npc_interact.npc_screen",
    "pause.pause_screen",
    "invite.invite_screen",
    "chat.chat_screen",
    "start.start_screen",
    "play.play_screen",
    "add_external_server.add_external_server_screen_new",
    "settings.screen_controls_and_settings",
    "pack_settings.screen",
    "death.death_screen",
    "progress.progress_screen",
    "progress.world_convert_modal_progress_screen",
    "progress.world_loading_progress_screen",
    "progress.realms_stories_loading_progress_screen",
    "disconnect.disconnect_screen",
    "xbl_console_signin.xbl_console_signin",
    "store_layout.store_data_driven_screen",
    "store_inventory.store_inventory_screen",
    "store_progress.store_progress_screen",
];

pub fn is_engine_screen(reference: &str) -> bool {
    ENGINE_SCREENS.contains(&reference)
}

/// Resolve, bind, lay out, and emit an allow-listed screen against `data`.
/// `None` for a screen off the allow-list or a reference the catalog lacks.
pub fn render_screen(
    reference: &str,
    catalog: &Catalog,
    context: &Context,
    data: &DataSource,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
) -> Option<ScreenRender> {
    let root = resolve_screen(reference, catalog, context)?;
    let bound = bind_screen(&root, catalog, context, data, &mut BindState::new());
    Some(finish(bound, root_size, env, state))
}

/// Resolve an allow-listed screen; `None` off the allow-list or when the
/// catalog lacks it. The result depends only on its inputs, so callers may
/// keep it while those stay the same.
pub fn resolve_screen(
    reference: &str,
    catalog: &Catalog,
    context: &Context,
) -> Option<ResolvedControl> {
    if !is_engine_screen(reference) {
        return None;
    }
    resolve(catalog, reference, context).control
}

/// The `ScreenSettings` of a `type: "screen"` definition; `None` when the
/// reference is unknown, ignored or not a screen.
pub fn screen_settings(
    reference: &str,
    catalog: &Catalog,
    context: &Context,
) -> Option<crate::ScreenSettings> {
    let (namespace, name) = reference.split_once('.')?;
    let root = context.root_env(catalog);
    let (control_type, properties) =
        crate::Resolver::new(catalog).resolve_root_properties(namespace, name, &root)?;
    (control_type.as_deref() == Some("screen"))
        .then(|| crate::ScreenSettings::from_properties(&properties))
}

/// One data refresh of a resolved screen whose live bindings `state` keeps,
/// ready for [`crate::render_bound`].
pub fn bind_screen(
    root: &ResolvedControl,
    catalog: &Catalog,
    context: &Context,
    data: &DataSource,
    state: &mut BindState,
) -> ResolvedControl {
    let library = CatalogLibrary { catalog, context };
    bind_stateful(&std::sync::Arc::new(root.clone()), data, &library, state).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_settings_do_not_allocate_for_unused_child_trees() {
        let catalog = |children: usize| {
            let controls: Vec<_> = (0..children)
                .map(|index| {
                    serde_json::json!({
                        format!("child{index}"): {
                            "type":"panel", "controls":[{"label":{"type":"label","text":"unused"}}]
                        }
                    })
                })
                .collect();
            let bytes = serde_json::to_vec(&serde_json::json!({
                "namespace":"settings",
                "base":{"type":"screen", "$capture":true, "absorbs_input":"$capture", "controls":controls},
                "screen@base":{"should_steal_mouse":false}
            }))
            .unwrap();
            Catalog::from_files([
                ("ui/_global_variables.json", b"{}".as_slice()),
                (
                    "ui/_ui_defs.json",
                    br#"{"ui_defs":["ui/settings.json"]}"#.as_slice(),
                ),
                ("ui/settings.json", bytes.as_slice()),
            ])
            .unwrap()
        };
        let small = catalog(0);
        let large = catalog(1_024);
        let context = Context::empty();
        let (small, small_allocations) = crate::allocation_count::count(|| {
            screen_settings("settings.screen", &small, &context).unwrap()
        });
        let (large, large_allocations) = crate::allocation_count::count(|| {
            screen_settings("settings.screen", &large, &context).unwrap()
        });
        assert_eq!(small, large);
        assert!(large.absorbs_input);
        assert!(!large.should_steal_mouse);
        assert_eq!(
            large_allocations, small_allocations,
            "screen policies consume root properties independently of descendant count"
        );
    }

    #[test]
    fn the_gameplay_hud_is_an_engine_screen() {
        assert!(is_engine_screen("hud.hud_screen"));
        assert!(is_engine_screen("hud_crosshair.hud_crosshair_screen"));
        assert!(!is_engine_screen("hud.hud_content"));
    }

    #[test]
    fn vanilla_join_progress_screens_are_engine_screens() {
        assert!(is_engine_screen("progress.world_loading_progress_screen"));
        assert!(is_engine_screen(
            "progress.realms_stories_loading_progress_screen"
        ));
    }
}
