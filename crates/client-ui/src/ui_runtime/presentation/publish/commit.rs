//! Pure UI rasterization, layout and publication after input enqueue.
use {super::*, ui::IconRef};

pub struct PreviewCapture {
    pub skin: Option<Arc<[u8]>>,
    /// Model and texels publish together once the menu worker has completed.
    pub ready: bool,
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
    if preview.ready {
        presentation.sync_player_preview(
            preview.skin.as_deref(),
            preview.pose,
            preview.shown,
            preview.hands,
            prepared.now_millis as f64 / 1000.0,
        );
    }
    let icon = presentation.player_preview_icon();
    let (left, right) = presentation.player_hand_icons();
    presentation.hud_frame.player_preview = icon;
    presentation.hud_frame.left_hand = left.filter(|_| preview.hands);
    presentation.hud_frame.right_hand = right.filter(|_| preview.hands);
    if let Some(menu) = presentation.menu_view.as_mut()
        && menu.profile_icon != icon
    {
        Arc::make_mut(menu).profile_icon = icon;
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
    if presentation.hud_frame.hand_rig_active || !presentation.hud_frame.first_person {
        presentation.hud_frame.held_item_icon = None;
        presentation.hud_frame.offhand_viewmodel_icon = None;
        return;
    }
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
    use {super::*, ui::IconRef};

    #[test]
    fn switching_hotbar_icons_does_not_rasterize_or_replace_textures_under_the_hand_rig() {
        let mut presentation =
            UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
        presentation.hud_frame.first_person = true;
        presentation.hud_frame.hand_rig_active = true;
        let textures = Arc::clone(&presentation.textures);
        for slot in 0..9 {
            publish_item_viewmodels(
                &mut presentation,
                (
                    Some(IconRef {
                        page: 0,
                        uv: [slot * 8, 0, slot * 8 + 8, 8],
                        glint: false,
                    }),
                    None,
                ),
            );
        }
        assert!(Arc::ptr_eq(&textures, &presentation.textures));
        assert!(presentation.held_viewmodel_source.is_none());
        assert!(presentation.hud_frame.held_item_icon.is_none());

        presentation.hud_frame.hand_rig_active = false;
        publish_item_viewmodels(
            &mut presentation,
            (
                Some(IconRef {
                    page: 0,
                    uv: [0, 0, 8, 8],
                    glint: false,
                }),
                None,
            ),
        );
        assert!(presentation.held_viewmodel_source.is_some());
    }
}
