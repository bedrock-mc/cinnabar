use {super::*, launcher::menu::MenuScreen};

fn point(x: f32, y: f32) -> Option<UiPoint> {
    Some(UiPoint::new(x, y).unwrap())
}

fn preview() -> MenuPreview {
    MenuPreview {
        screen: Some(MenuScreen::Home),
        control: Some(PreviewControl {
            bounds: rect(100.0, 100.0, 300.0, 400.0).unwrap(),
            gui_pixel: 2.0,
        }),
        ..Default::default()
    }
}

#[test]
fn drag_capture_persists_outside_the_character_and_release_keeps_the_rotation() {
    let mut preview = preview();
    assert!(preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, true));
    assert!(preview.update(Some(MenuScreen::Home), point(450.0, 20.0), true, false));
    let rotated = preview.rotation();
    assert_ne!(rotated, 0.0);
    assert!(preview.update(Some(MenuScreen::Home), point(450.0, 20.0), false, false));
    assert_eq!(preview.rotation(), rotated);
    assert!(!preview.update(Some(MenuScreen::Home), point(250.0, 200.0), false, false));
    assert_eq!(preview.rotation(), rotated, "hover never rotates the body");
}

#[test]
fn a_press_outside_the_character_never_captures_when_moved_inside() {
    let mut preview = preview();
    assert!(!preview.update(Some(MenuScreen::Home), point(20.0, 20.0), true, true));
    assert!(!preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, false));
    assert_eq!(preview.rotation(), 0.0);
}

#[test]
fn entering_another_screen_revokes_character_capture() {
    let mut preview = preview();
    assert!(preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, true));
    assert!(!preview.update(Some(MenuScreen::Settings), point(250.0, 200.0), true, false));
    assert!(preview.control.is_none());
    assert!(!preview.captured);
}

#[test]
fn stationary_held_pointer_does_not_apply_the_same_delta_twice() {
    let mut preview = preview();
    preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, true);
    preview.update(Some(MenuScreen::Home), point(240.0, 200.0), true, false);
    let rotated = preview.rotation();
    preview.update(Some(MenuScreen::Home), point(240.0, 200.0), true, false);
    assert_eq!(preview.rotation(), rotated);
}

#[test]
fn dragging_right_turns_the_character_to_the_left() {
    let mut preview = preview();
    preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, true);
    preview.update(Some(MenuScreen::Home), point(240.0, 200.0), true, false);
    assert!(preview.rotation() > 180.0, "a rightward drag decreases yaw");
    preview.update(Some(MenuScreen::Home), point(200.0, 200.0), true, false);
    assert_eq!(
        preview.rotation(),
        0.0,
        "the reverse drag restores its turn"
    );
}

#[test]
fn dressing_room_character_fits_its_stage_through_rotation_head_look_and_idle() {
    use crate::ui_runtime::presentation::player_preview::geometry;
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    presentation
        .menu_preview
        .begin_frame(Some(MenuScreen::DressingRoom));
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    for control in [[100.0, 100.0, 300.0, 490.0], [100.0, 100.0, 180.0, 250.0]] {
        let mut first_frame = None;
        for rotation in [0.0, 90.0, 180.0, 270.0] {
            for pointer in [
                point(200.0, 295.0),
                point(-10_000.0, -10_000.0),
                point(10_000.0, 10_000.0),
            ] {
                for seconds in [0.0, 1.0] {
                    presentation.menu_preview.rotations[2] = rotation;
                    presentation.menu_preview.pointer = pointer;
                    presentation.menu_seconds = seconds;
                    let mut nodes = Vec::new();
                    let mut next = 1;
                    presentation
                        .append_menu_player_preview(
                            &mut nodes,
                            &mut next,
                            metrics,
                            control,
                            control,
                            MenuPreviewConfig::DRESSING_ROOM,
                        )
                        .unwrap();
                    let mut tree = ui::UiTree::new(nodes).unwrap();
                    let layout = tree
                        .layout(
                            rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
                            ui::UiScale::default(),
                            ui::SafeArea::default(),
                        )
                        .unwrap();
                    let frame = layout.bounds(UiNodeId::new(next - 1)).unwrap();
                    if let Some(first) = first_frame {
                        assert_eq!(
                            frame, first,
                            "idle and pointer look never resize the character"
                        );
                    } else {
                        first_frame = Some(frame);
                    }
                    let model = geometry::mesh(
                        Default::default(),
                        presentation.player_preview_view,
                        presentation.player_preview_bob,
                        presentation.player_preview_icon.unwrap(),
                        &Default::default(),
                        [None; 4],
                        [None; 2],
                        false,
                    )
                    .unwrap();
                    for vertex in model.vertices() {
                        let x = frame.min().x() + vertex.position[0] * frame.width();
                        let y = frame.min().y() + vertex.position[1] * frame.height();
                        assert!(
                            x >= control[0] - 0.001
                                && x <= control[2] + 0.001
                                && y >= control[1] - 0.001
                                && y <= control[3] + 0.001,
                            "character pixel ({x},{y}) escapes {control:?} at yaw={rotation}, t={seconds}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dressing_room_uses_native_idle_arm_sway_without_changing_menu_paper_dolls() {
    use crate::ui_runtime::presentation::player_preview::{bob_degrees, geometry};
    let source = render_model::standard_biped_vertices();
    let icon = ui::IconRef {
        page: 0,
        uv: [0, 0, 64, 64],
        glint: false,
    };
    let mesh = |config: MenuPreviewConfig, seconds| {
        geometry::mesh(
            Default::default(),
            config.view([0.0; 2]),
            bob_degrees(seconds),
            icon,
            &Default::default(),
            [None; 4],
            [None; 2],
            false,
        )
        .unwrap()
    };
    let first = mesh(MenuPreviewConfig::DRESSING_ROOM, 0.0);
    let later = mesh(MenuPreviewConfig::DRESSING_ROOM, 1.0);
    let mut moving_arms = 0;
    for ((original, first), later) in source.iter().zip(first.vertices()).zip(later.vertices()) {
        if matches!(original.part, 2 | 3) {
            moving_arms += usize::from(first.position != later.position);
        } else {
            assert_eq!(
                first.position, later.position,
                "idle leaves body, head and legs stationary"
            );
        }
    }
    assert!(moving_arms > 0, "idle animates the arms continuously");
    let first = mesh(MenuPreviewConfig::MENU, 0.0);
    let later = mesh(MenuPreviewConfig::MENU, 1.0);
    for (first, later) in first.vertices().iter().zip(later.vertices()) {
        assert_eq!(
            first.position, later.position,
            "Home/Pause paper dolls retain their native pose"
        );
    }
}

#[test]
fn head_tracking_is_bounded_and_reverses_yaw_when_facing_backwards() {
    let front = MenuPreviewConfig::MENU.view([1_000_000.0, -1_000_000.0]);
    let back = front.with_menu_rotation(180.0);
    let [body, head, pitch, tilt] = front.angles();
    let [back_body, back_head, back_pitch, back_tilt] = back.angles();
    let bound = std::f32::consts::FRAC_PI_2 * super::super::POINTER_ANGLE_FACTOR;
    assert!((head - body).abs() < bound);
    assert!(pitch.abs() < bound);
    assert!(((head - body) + (back_head - back_body)).abs() < 0.0001);
    assert_eq!(pitch, back_pitch);
    assert_eq!(tilt, back_tilt);
}

#[test]
fn a_scrolled_character_only_captures_visible_pixels_and_keeps_its_full_centre() {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    presentation
        .menu_preview
        .begin_frame(Some(MenuScreen::DressingRoom));
    presentation.menu_player_preview_pointer(
        Some(MenuScreen::DressingRoom),
        point(200.0, 60.0),
        false,
        false,
    );
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    let mut next = 0;
    presentation
        .append_menu_player_preview(
            &mut nodes,
            &mut next,
            metrics,
            [100.0, -100.0, 300.0, 100.0],
            [50.0, 50.0, 350.0, 300.0],
            MenuPreviewConfig::MENU,
        )
        .unwrap();
    assert!(!presentation.menu_player_preview_pointer(
        Some(MenuScreen::DressingRoom),
        point(200.0, 10.0),
        true,
        true
    ));
    assert!(presentation.menu_player_preview_pointer(
        Some(MenuScreen::DressingRoom),
        point(200.0, 75.0),
        true,
        true
    ));
    assert!(
        presentation.menu_player_preview_angles()[2] > 0.0,
        "head tracking uses the model's full centre, including the scrolled-away portion"
    );

    presentation
        .menu_preview
        .begin_frame(Some(MenuScreen::DressingRoom));
    presentation
        .append_menu_player_preview(
            &mut nodes,
            &mut next,
            metrics,
            [100.0, -200.0, 300.0, -50.0],
            [50.0, 50.0, 350.0, 300.0],
            MenuPreviewConfig::MENU,
        )
        .unwrap();
    assert!(
        presentation.menu_player_preview_bounds().is_none(),
        "a fully clipped character has no input region"
    );
}

#[test]
fn safe_area_and_dpi_keep_the_pointer_aligned_with_the_character_centre() {
    for dpi in [1.0, 2.0] {
        let mut presentation =
            UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
        presentation.set_safe_area(ui::SafeArea::new(17.0, 19.0, 0.0, 0.0).unwrap());
        presentation.set_player_preview_skin(None, Default::default());
        presentation
            .menu_preview
            .begin_frame(Some(MenuScreen::DressingRoom));
        presentation.menu_player_preview_pointer(
            Some(MenuScreen::DressingRoom),
            point(217.0, 269.0),
            false,
            false,
        );
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(dpi).unwrap(), Some(2));
        presentation
            .append_menu_player_preview(
                &mut Vec::new(),
                &mut 0,
                metrics,
                [100.0, 100.0, 300.0, 400.0],
                [100.0, 100.0, 300.0, 400.0],
                MenuPreviewConfig::MENU,
            )
            .unwrap();
        let [body, head, pitch, _] = presentation.menu_player_preview_angles();
        assert_eq!(
            head, body,
            "the visible centre has no horizontal head turn at DPI{dpi}"
        );
        assert_eq!(
            pitch, 0.0,
            "the visible centre has no vertical head turn at DPI{dpi}"
        );
        let bounds = presentation.menu_player_preview_bounds().unwrap();
        assert_eq!([bounds.min().x(), bounds.min().y()], [117.0, 119.0]);
    }
}

#[test]
fn fitted_dressing_room_look_origin_tracks_the_displayed_head_pivot() {
    use crate::ui_runtime::presentation::player_preview::{
        HEAD_PIVOT, PREVIEW_HEIGHT, PREVIEW_WIDTH, Rig,
    };
    for dpi in [1.0, 2.0] {
        let mut presentation =
            UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
        let safe_area = ui::SafeArea::new(17.0, 19.0, 0.0, 0.0).unwrap();
        presentation.set_safe_area(safe_area);
        presentation.set_player_preview_skin(None, Default::default());
        presentation
            .menu_preview
            .begin_frame(Some(MenuScreen::DressingRoom));
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(dpi).unwrap(), Some(2));
        let control = [100.0, 100.0, 300.0, 490.0];
        let mut nodes = Vec::new();
        let mut next = 1;
        presentation
            .append_menu_player_preview(
                &mut nodes,
                &mut next,
                metrics,
                control,
                control,
                MenuPreviewConfig::DRESSING_ROOM,
            )
            .unwrap();
        let mut tree = ui::UiTree::new(nodes).unwrap();
        let layout = tree
            .layout(
                rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
                ui::UiScale::default(),
                safe_area,
            )
            .unwrap();
        let frame = layout.bounds(UiNodeId::new(next - 1)).unwrap();
        let pivot = Rig::new(
            Default::default(),
            MenuPreviewConfig::DRESSING_ROOM.view([0.0; 2]),
            0.0,
            [false; 2],
        )
        .project(render_model::ActorVertex {
            position: HEAD_PIVOT,
            uv: [0.0; 2],
            part: 0,
        })
        .screen;
        let target = point(
            frame.min().x() + pivot[0] / PREVIEW_WIDTH as f32 * frame.width(),
            frame.min().y() + pivot[1] / PREVIEW_HEIGHT as f32 * frame.height(),
        );
        presentation.menu_player_preview_pointer(
            Some(MenuScreen::DressingRoom),
            target,
            false,
            false,
        );
        presentation
            .append_menu_player_preview(
                &mut Vec::new(),
                &mut 1,
                metrics,
                control,
                control,
                MenuPreviewConfig::DRESSING_ROOM,
            )
            .unwrap();
        let [body, head, pitch, _] = presentation.menu_player_preview_angles();
        assert!(
            (head - body).abs() < 0.0001,
            "neutral yaw follows the displayed head at DPI{dpi}"
        );
        assert!(
            pitch.abs() < 0.0001,
            "neutral pitch follows the displayed head at DPI{dpi}"
        );
        let target = target.unwrap();
        presentation.menu_player_preview_pointer(
            Some(MenuScreen::DressingRoom),
            point(target.x(), target.y() + 20.0),
            false,
            false,
        );
        presentation
            .append_menu_player_preview(
                &mut Vec::new(),
                &mut 1,
                metrics,
                control,
                control,
                MenuPreviewConfig::DRESSING_ROOM,
            )
            .unwrap();
        assert!(
            presentation.menu_player_preview_angles()[2] > 0.0,
            "the head follows a pointer below its displayed pivot"
        );
    }
}

#[test]
fn an_attached_cape_stays_inside_the_fitted_stage_when_the_character_turns() {
    use crate::ui_runtime::presentation::player_preview::{
        PREVIEW_HEIGHT, PREVIEW_WIDTH, Rig, cape,
    };
    let side = render_api::CLASSIC_SKIN_SIDE as u32;
    let (width, height) = render_api::CAPE_DIMENSIONS[0];
    let skin = render_api::StandardSkin {
        width: side,
        height: side,
        rgba8: vec![255; (side * side * 4) as usize].into(),
        geometry: None,
        cape: Some(render_api::CapeImage {
            width,
            height,
            rgba8: vec![255; (width * height * 4) as usize].into(),
        }),
    };
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    assert!(presentation.set_menu_preview_skin(&skin));
    presentation
        .menu_preview
        .begin_frame(Some(MenuScreen::DressingRoom));
    let control = [100.0, 100.0, 300.0, 490.0];
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    for rotation in [0.0, 90.0, 180.0, 270.0] {
        presentation.menu_preview.rotations[2] = rotation;
        let mut nodes = Vec::new();
        let mut next = 1;
        presentation
            .append_menu_player_preview(
                &mut nodes,
                &mut next,
                metrics,
                control,
                control,
                MenuPreviewConfig::DRESSING_ROOM,
            )
            .unwrap();
        let mut tree = ui::UiTree::new(nodes).unwrap();
        let layout = tree
            .layout(
                rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
                ui::UiScale::default(),
                ui::SafeArea::default(),
            )
            .unwrap();
        let frame = layout.bounds(UiNodeId::new(next - 1)).unwrap();
        let rig = Rig::new(
            Default::default(),
            presentation.player_preview_view,
            presentation.player_preview_bob,
            [false; 2],
        );
        for original in cape::rest_vertices() {
            let vertex = rig.project(*original).screen;
            let x = frame.min().x() + vertex[0] / PREVIEW_WIDTH as f32 * frame.width();
            let y = frame.min().y() + vertex[1] / PREVIEW_HEIGHT as f32 * frame.height();
            assert!(
                x >= control[0] - 0.001
                    && x <= control[2] + 0.001
                    && y >= control[1] - 0.001
                    && y <= control[3] + 0.001,
                "attached cape escapes the stage at yaw={rotation}: ({x},{y})"
            );
        }
    }
}
