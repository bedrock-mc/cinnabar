//! Pure UI rasterization, layout and publication after input enqueue.
use super::*;

pub struct PreviewCapture {
    pub skin: Option<Arc<[u8]>>,
    pub pose: player_preview::PlayerPreviewPose,
    pub shown: bool,
    pub hands: bool,
}

/// The authority and viewport captured before input is enqueued.
pub struct PendingUiPublication {
    pub inventory: crate::ui_runtime::presentation_snapshot::PresentationInventory,
    pub preview: PreviewCapture,
    pub item_icons: (Option<IconRef>, Option<IconRef>),
    pub now_millis: u64,
    pub physical_size: [u32; 2],
    pub dpi_scale: DpiScale,
}

#[derive(Resource, Default)]
pub struct PreparedUiPublication(pub Option<PendingUiPublication>);

/// Renders the captured UI without observing this frame's post-capture inventory changes.
pub fn render_prepared_ui(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    prepared: PendingUiPublication,
) -> Result<UiRenderInput, UiPresentationError> {
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
    publish_item_viewmodels(presentation, prepared.item_icons);
    runtime.with_presentation_inventory(
        player_runtime,
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
    )
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
#[cfg(any(test, feature = "test-support"))]
pub fn refresh_hud_frame(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: Option<&chunk_pipeline::WorldStream>,
    perspective: semantic_input::PerspectiveMode,
    now_millis: u64,
) {
    let icons = capture_hud_frame(
        player_runtime,
        runtime,
        presentation,
        stream,
        perspective,
        now_millis,
        ItemIconFrames::default(),
    );
    publish_item_viewmodels(presentation, icons);
}
