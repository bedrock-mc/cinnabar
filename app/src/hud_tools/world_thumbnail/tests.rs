use super::*;
use client_ui::ui_runtime::SequencedUiEvent;
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent};

/// Supplies a connected world without needing a renderer or installed UI carrier.
fn gameplay() -> (UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut ui = UiRuntime::new(1);
    ui.publish_local_runtime_id(&mut player, 1, 42).unwrap();
    ui.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    (ui, player)
}

#[test]
fn due_captures_wait_for_in_game_overlays_to_close() {
    let (mut ui, mut player) = gameplay();
    let mut state = WorldThumbnailCapture::default();
    assert!(
        state
            .due_world(Duration::ZERO, Some("world"), true)
            .is_none()
    );
    ui.toggle_inventory(&mut player, None);
    assert!(ui.inventory_open());
    assert!(
        state
            .due_world(
                FIRST_CAPTURE,
                Some("world"),
                gameplay_capture_allowed(&ui, &player, None, None)
            )
            .is_none()
    );
    assert_eq!(state.next_at, Some(FIRST_CAPTURE));
    ui.close_inventory(&mut player);
    ui.open_chat(&mut player);
    assert!(
        state
            .due_world(
                FIRST_CAPTURE,
                Some("world"),
                gameplay_capture_allowed(&ui, &player, None, None)
            )
            .is_none()
    );
    ui.close_chat();
    assert_eq!(
        state.due_world(
            FIRST_CAPTURE,
            Some("world"),
            gameplay_capture_allowed(&ui, &player, None, None)
        ),
        Some("world".into())
    );
    assert_eq!(state.next_at, Some(FIRST_CAPTURE + RECAPTURE));
}

#[test]
fn chest_and_server_form_scenes_block_thumbnail_capture() {
    let (mut ui, mut player) = gameplay();
    ui.enqueue_inventory_event(
        &mut player,
        1,
        1,
        protocol::InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: protocol::ContainerIdentity::window(4),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: 70,
        }),
    )
    .unwrap();
    ui.drain_pending_inventory(&mut player);
    assert!(ui.inventory_open());
    assert!(!gameplay_capture_allowed(&ui, &player, None, None));

    let (mut ui, mut player) = gameplay();
    ui.apply(
        &mut player,
        SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Form(FormRequestEvent {
                form_id: 1,
                kind: FormKind::Menu,
                title: Some(Arc::from("Choose")),
                json: Arc::from("{}"),
                model: ServerFormModel::TextMenu(TextMenuForm {
                    title: protocol::FormText::from("Choose"),
                    content: protocol::FormText::from("Choose a button"),
                    buttons: vec![protocol::FormText::from("OK")].into(),
                    button_images: [].into(),
                    omitted_images: 0,
                }),
            }),
        },
    )
    .unwrap();
    assert!(ui.server_forms().owns_input());
    assert!(!gameplay_capture_allowed(&ui, &player, None, None));
}

#[test]
fn launcher_and_personal_panels_block_capture_and_leaving_resets_the_deadline() {
    let (ui, player) = gameplay();
    let mut menu = MenuRuntime::new(false, 2, "Preview".into());
    menu.open_pause();
    assert!(!gameplay_capture_allowed(&ui, &player, Some(&menu), None));
    assert!(!gameplay_capture_allowed(
        &ui,
        &player,
        None,
        Some(&ScreenFocus {
            absorbs_input: true,
            ..Default::default()
        })
    ));
    let mut state = WorldThumbnailCapture::default();
    assert!(
        state
            .due_world(Duration::ZERO, Some("first"), true)
            .is_none()
    );
    assert!(state.due_world(FIRST_CAPTURE, None, true).is_none());
    assert!(
        state
            .due_world(FIRST_CAPTURE, Some("second"), true)
            .is_none()
    );
    assert_eq!(state.next_at, Some(FIRST_CAPTURE * 2));
    assert!(!gameplay_capture_allowed(
        &UiRuntime::new(0),
        &player,
        None,
        None
    ));
}

#[test]
fn thumbnail_crop_preserves_aspect_and_leaves_frame_edges_out() {
    for (width, height) in [(1920, 1080), (3840, 1080), (1080, 1920), (1280, 720)] {
        let [x, y, crop_width, crop_height] = thumbnail_crop(width, height).unwrap();
        assert_eq!(
            crop_width * THUMBNAIL_SIZE[1],
            crop_height * THUMBNAIL_SIZE[0]
        );
        assert!(x > 0 && y > 0);
        assert!(x + crop_width < width && y + crop_height < height);
        assert!(crop_width <= width * CROP_SHARE / 100);
        assert!(crop_height <= height * CROP_SHARE / 100);
    }
    assert!(thumbnail_crop(0, 1080).is_none());
    assert!(thumbnail_crop(1920, 0).is_none());
    assert!(thumbnail_crop(1, 1).is_none());
}

#[test]
fn ultrawide_thumbnail_excludes_the_top_and_bottom_hud() {
    let pixels = image::RgbImage::from_fn(3840, 1080, |_, y| {
        if !(80..1000).contains(&y) {
            image::Rgb([255, 0, 0])
        } else {
            image::Rgb([0, 0, 255])
        }
    });
    let frame = Image::from_dynamic(
        image::DynamicImage::ImageRgb8(pixels),
        true,
        Default::default(),
    );
    let jpeg = encode_thumbnail(frame).unwrap();
    let decoded = image::load_from_memory(&jpeg).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (THUMBNAIL_SIZE[0], THUMBNAIL_SIZE[1]));
    assert!(
        decoded
            .pixels()
            .all(|pixel| pixel[2] > 200 && pixel[0] < 20)
    );
}
