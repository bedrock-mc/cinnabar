//! Pure UI rasterization, layout and publication after input enqueue.
use super::*;

pub(super) struct PreviewCapture {
    pub(super) skin: Option<Arc<[u8]>>,
    pub(super) pose: player_preview::PlayerPreviewPose,
    pub(super) shown: bool,
    pub(super) hands: bool,
}

/// The authority and viewport captured before input is enqueued.
pub(super) struct PendingUiPublication {
    pub(super) inventory: crate::ui_runtime::presentation_snapshot::PresentationInventory,
    pub(super) preview: PreviewCapture,
    pub(super) item_icons: (Option<IconRef>, Option<IconRef>),
    pub(super) now_millis: u64,
    pub(super) physical_size: [u32; 2],
    pub(super) dpi_scale: DpiScale,
}

#[derive(Resource, Default)]
pub(crate) struct PreparedUiPublication(pub(super) Option<PendingUiPublication>);

/// Publishes the captured UI without observing mutations from this frame's outbound actions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut prepared: ResMut<PreparedUiPublication>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    mut client_world: ResMut<ClientWorld>,
    hand_rig: Res<render::HandRigScene>,
    nametag_scene: Option<ResMut<render::NametagScene>>,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let Some(prepared) = prepared.0.take() else {
        return;
    };
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::UiPublication));
    let preview = prepared.preview;
    presentation.sync_player_preview(
        preview.skin.as_deref(),
        preview.pose,
        preview.shown,
        preview.hands,
        prepared.now_millis as f64 / 1000.0,
    );
    let icon = presentation.player_preview_icon();
    let (left, right) = presentation.player_hand_icons();
    presentation.hud_frame.player_preview = icon;
    presentation.hud_frame.left_hand = left;
    presentation.hud_frame.right_hand = right;
    if let Some(menu) = presentation.menu_view.as_mut() {
        menu.profile_icon = icon;
    }
    publish_item_viewmodels(&mut presentation, prepared.item_icons);
    let input = match runtime.with_presentation_inventory(
        &mut player_runtime,
        prepared.inventory,
        |runtime, player_runtime| {
            presentation.build(
                player_runtime,
                runtime,
                prepared.now_millis,
                prepared.physical_size,
                prepared.dpi_scale,
            )
        },
    ) {
        Ok(input) => input,
        Err(error) => {
            hand.clear();
            record_fatal_error(&mut client_world.fatal_error, error.to_string());
            return;
        }
    };
    if let Some(mut scene) = nametag_scene {
        *scene = presentation.nametag_scene();
    }
    if !hand_rig.is_active() {
        hand.bind_cpu_fallback(
            &input,
            presentation.cpu_empty_hand_fallback(),
            presentation.hud_frame.held_item_icon,
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

/// Renders the captured held-item icons and updates the frame's raster references.
fn publish_item_viewmodels(
    presentation: &mut UiPresentationRuntime,
    (held, offhand): (Option<IconRef>, Option<IconRef>),
) {
    presentation.set_item_viewmodels(held, offhand);
    let (held, offhand) = presentation.item_viewmodel_icons();
    presentation.hud_frame.held_item_icon = held;
    presentation.hud_frame.offhand_viewmodel_icon = offhand;
}

/// Runs both HUD phases together for offline witnesses.
#[cfg(test)]
pub(crate) fn refresh_hud_frame(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: Option<&client_world::WorldStream>,
    camera_settings: &CameraSettingsAuthority,
    now_millis: u64,
) {
    let icons = capture_hud_frame(
        player_runtime,
        runtime,
        presentation,
        stream,
        camera_settings,
        now_millis,
        None,
    );
    publish_item_viewmodels(presentation, icons);
}
