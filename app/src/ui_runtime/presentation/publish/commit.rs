//! Pure UI rasterization, layout and publication after input enqueue.
use super::*;

/// Publishes the UI captured during preparation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    player_runtime: Res<crate::player_runtime::PlayerRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut prepared: ResMut<PreparedUiPublication>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<render::UiRenderSceneResource>,
    stats: Res<render::UiRenderStatsResource>,
    mut client_world: ResMut<ClientWorld>,
    hand_rig: Res<render::HandRigScene>,
    nametag_scene: Option<ResMut<render::NametagSceneResource>>,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let Some(prepared) = prepared.0.take() else {
        return;
    };
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::UiPublication));
    let input = match client_ui::ui_runtime::presentation::render_prepared_ui(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        prepared,
    ) {
        Ok(input) => input,
        Err(error) => {
            hand.clear();
            record_fatal_error(&mut client_world.fatal_error, error.to_string());
            return;
        }
    };
    if let Some(mut scene) = nametag_scene {
        scene.0 = presentation.nametag_scene();
    }
    if !hand_rig.is_active() {
        hand.bind_cpu_fallback(
            &input,
            presentation.cpu_empty_hand_fallback(),
            presentation.hud_frame().held_item_icon,
        );
    }
    if let Err(error) = scene.publish(input, &stats) {
        hand.clear();
        record_fatal_error(
            &mut client_world.fatal_error,
            UiPresentationError::Render(error).to_string(),
        );
    }
}
