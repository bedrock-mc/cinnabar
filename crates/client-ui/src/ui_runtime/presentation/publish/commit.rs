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
    let timed = trace_ui_frame(presentation, runtime);
    let started = timed.then(std::time::Instant::now);
    let source_changed = timed && presentation.server_ui_changed(runtime.server_ui());
    let preview = prepared.preview;
    presentation.sync_player_preview(
        preview.skin.as_deref(),
        preview.pose,
        preview.shown,
        preview.hands,
        prepared.now_millis as f64 / 1000.0,
    );
    let preview_ms = started.map(|started| started.elapsed().as_secs_f64() * 1_000.0);
    let icon = presentation.player_preview_icon();
    let (left, right) = presentation.player_hand_icons();
    presentation.hud_frame.player_preview = icon;
    presentation.hud_frame.left_hand = left;
    presentation.hud_frame.right_hand = right;
    if let Some(menu) = presentation.menu_view.as_mut() {
        menu.profile_icon = icon;
    }
    publish_item_viewmodels(presentation, prepared.item_icons);
    let viewmodel_ms = started
        .map(|started| started.elapsed().as_secs_f64() * 1_000.0 - preview_ms.unwrap_or_default());
    let input = runtime.with_presentation_inventory(
        player_runtime,
        prepared.inventory,
        |runtime, player_runtime| {
            presentation.build_profiled(
                player_runtime,
                runtime,
                prepared.now_millis,
                prepared.physical_size,
                prepared.dpi_scale,
                timed,
            )
        },
    );
    if let Some(started) = started {
        bevy::log::info!(
            session_generation = runtime.session_id(),
            source_changed,
            preview_ms,
            viewmodel_ms,
            build_ms = started.elapsed().as_secs_f64() * 1_000.0
                - preview_ms.unwrap_or_default()
                - viewmodel_ms.unwrap_or_default(),
            total_ms = started.elapsed().as_secs_f64() * 1_000.0,
            "session UI frame prepared",
        );
    }
    input
}

fn trace_ui_frame(presentation: &UiPresentationRuntime, runtime: &UiRuntime) -> bool {
    presentation.texture_session != Some(runtime.session_id())
        || presentation.server_ui_changed(runtime.server_ui())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_publication_is_observed_after_a_prebootstrap_frame_in_the_same_session() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        let player = player_state::PlayerState::new(2);
        let mut runtime = UiRuntime::new(2);
        let viewport = [640, 480];
        let scale = DpiScale::new(1.0).unwrap();
        assert!(trace_ui_frame(&presentation, &runtime));
        presentation
            .build(&player, &runtime, 0, viewport, scale)
            .unwrap();
        assert!(!trace_ui_frame(&presentation, &runtime));
        let base = presentation.pack_catalog_base().unwrap();
        runtime.set_server_ui(Some(forms::ServerUiPack::default().prepare_catalog(&base)));
        assert!(trace_ui_frame(&presentation, &runtime));
        presentation
            .build(&player, &runtime, 1, viewport, scale)
            .unwrap();
        assert!(!trace_ui_frame(&presentation, &runtime));
        runtime.set_server_ui(None);
        assert!(trace_ui_frame(&presentation, &runtime));
        presentation
            .build(&player, &runtime, 2, viewport, scale)
            .unwrap();
        assert!(!trace_ui_frame(&presentation, &runtime));
    }
}
