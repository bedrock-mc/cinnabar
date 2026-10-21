//! The screens the engine is allowed to draw, and the generic renderer for them;
//! [`render_screen`] refuses anything not on the allow-list.

use crate::bind::{BindState, DataSource, bind_stateful};
use crate::catalog::Catalog;
use crate::form::{CatalogLibrary, FormRender, finish};
use crate::layout::LayoutEnv;
use crate::state::ViewState;
use crate::vanilla::Reference;
use crate::{Context, ResolvedControl, resolve};

/// A rendered engine screen: bound tree, draw nodes, hit regions, scroll report.
pub type ScreenRender = FormRender;

/// Every `namespace.name` screen the engine renders.
pub const ENGINE_SCREENS: &[&str] = &[
    Reference::TradeTradeScreen.as_str(),
    Reference::Trade2TradeScreen.as_str(),
    crate::vanilla::Reference::HudHudScreen.as_str(),
    crate::vanilla::Reference::HudCrosshairHudCrosshairScreen.as_str(),
    Reference::ServerFormThirdPartyServerScreen.as_str(),
    Reference::ServerFormLongForm.as_str(),
    Reference::ServerFormCustomForm.as_str(),
    Reference::PopupDialogModalDialogPopup.as_str(),
    Reference::RatingPromptRatingPromptScreen.as_str(),
    Reference::CraftingInventoryScreen.as_str(),
    Reference::CraftingCraftingScreen.as_str(),
    Reference::ChestSmallChestScreen.as_str(),
    Reference::ChestLargeChestScreen.as_str(),
    Reference::ChestEnderChestScreen.as_str(),
    Reference::ChestBarrelScreen.as_str(),
    Reference::ChestShulkerBoxScreen.as_str(),
    Reference::FurnaceFurnaceScreen.as_str(),
    Reference::BlastFurnaceBlastFurnaceScreen.as_str(),
    Reference::SmokerSmokerScreen.as_str(),
    Reference::AnvilAnvilScreen.as_str(),
    Reference::EnchantingEnchantingScreen.as_str(),
    Reference::BrewingStandBrewingStandScreen.as_str(),
    Reference::GrindstoneGrindstoneScreen.as_str(),
    Reference::LoomLoomScreen.as_str(),
    Reference::SmithingTableSmithingTableScreen.as_str(),
    Reference::CartographyCartographyScreen.as_str(),
    Reference::StonecutterStonecutterScreen.as_str(),
    Reference::BeaconBeaconScreen.as_str(),
    Reference::RedstoneHopperScreen.as_str(),
    Reference::RedstoneDispenserScreen.as_str(),
    Reference::RedstoneDropperScreen.as_str(),
    Reference::RedstoneCrafterScreen.as_str(),
    Reference::HorseHorseScreen.as_str(),
    Reference::BookBookScreen.as_str(),
    Reference::NpcInteractNpcScreen.as_str(),
    Reference::PausePauseScreen.as_str(),
    Reference::InviteInviteScreen.as_str(),
    Reference::ChatChatScreen.as_str(),
    Reference::StartStartScreen.as_str(),
    Reference::PlayPlayScreen.as_str(),
    Reference::AddExternalServerAddExternalServerScreenNew.as_str(),
    Reference::SettingsScreenControlsAndSettings.as_str(),
    Reference::PackSettingsScreen.as_str(),
    Reference::DeathDeathScreen.as_str(),
    Reference::ProgressProgressScreen.as_str(),
    Reference::ProgressWorldConvertModalProgressScreen.as_str(),
    Reference::ProgressWorldLoadingProgressScreen.as_str(),
    Reference::ProgressRealmsStoriesLoadingProgressScreen.as_str(),
    Reference::DisconnectDisconnectScreen.as_str(),
    Reference::XblConsoleSigninXblConsoleSignin.as_str(),
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

/// [`bind_screen`] that keeps what the bind built in `state`, so [`rebind_screen`] can rebuild
/// only the controls a later refresh changes. A `library` handing out the same templates
/// across binds (a [`crate::CachedLibrary`]) lets unchanged factory and grid instances stand.
pub fn bind_screen_incremental(
    root: &std::sync::Arc<ResolvedControl>,
    library: &dyn crate::ControlLibrary,
    data: &std::sync::Arc<DataSource>,
    state: &mut BindState,
) -> ResolvedControl {
    crate::bind_incremental(root, data, library, state)
}

/// Applies a refresh in place onto `bound`, the tree `state`'s last incremental screen bind
/// produced, rebuilding only changed controls and keeping `measures` for the rest.
pub fn rebind_screen(
    root: &std::sync::Arc<ResolvedControl>,
    library: &dyn crate::ControlLibrary,
    data: &std::sync::Arc<DataSource>,
    state: &mut BindState,
    bound: &mut ResolvedControl,
    measures: &mut crate::MeasureCache,
) {
    crate::rebind(root, data, library, state, bound, measures);
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
        let (small, small_allocations) =
            test_alloc::count(|| screen_settings("settings.screen", &small, &context).unwrap());
        let (large, large_allocations) =
            test_alloc::count(|| screen_settings("settings.screen", &large, &context).unwrap());
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
        assert!(is_engine_screen(Reference::HudHudScreen.as_str()));
        assert!(is_engine_screen(
            Reference::HudCrosshairHudCrosshairScreen.as_str()
        ));
        assert!(!is_engine_screen("hud.hud_content"));
    }

    #[test]
    fn vanilla_join_progress_screens_are_engine_screens() {
        assert!(is_engine_screen(
            Reference::ProgressWorldLoadingProgressScreen.as_str()
        ));
        assert!(is_engine_screen(
            Reference::ProgressRealmsStoriesLoadingProgressScreen.as_str()
        ));
    }
}
