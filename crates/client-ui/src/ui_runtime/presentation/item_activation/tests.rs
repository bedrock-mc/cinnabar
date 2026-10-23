use super::*;
use crate::ui_runtime::item_activation::ItemActivation;
use assets::facts::TOTEM_IDENTIFIER;

#[test]
fn activation_projects_the_full_rotating_quad_and_preserves_icon_uvs() {
    let icon = IconRef {
        color_mask: None,
        page: 2,
        uv: [11, 13, 27, 29],
        glint: true,
    };
    let beginning = activation_mesh(icon, 0.0, [0; 2], [1280.0, 720.0]).unwrap();
    let middle = activation_mesh(icon, 0.5, [0; 2], [1280.0, 720.0]).unwrap();
    assert_eq!(beginning.vertices().len(), 4);
    assert_eq!(beginning.vertices()[0].uv, [11.0, 13.0]);
    assert_eq!(beginning.vertices()[2].uv, [27.0, 29.0]);
    assert_ne!(
        beginning.vertices()[0].position,
        middle.vertices()[0].position
    );
    assert!(beginning.vertices()[0].position[0] > beginning.vertices()[3].position[0]);
    assert_eq!(beginning.vertices()[0].style_flags, 0);
    for millis in 0..2_000 {
        let mesh = activation_mesh(icon, millis as f32 / 2_000.0, [0; 2], [1280.0, 720.0]).unwrap();
        assert!(
            mesh.vertices()
                .iter()
                .all(|vertex| vertex.position.iter().all(|value| value.is_finite()))
        );
    }
}

#[test]
fn activation_draws_the_session_texture_through_all_phases_and_then_disappears() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut runtime = UiRuntime::new(1);
    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: TOTEM_IDENTIFIER.into(),
            metadata: 7,
            width: 2,
            height: 1,
            rgba8: vec![255, 0, 255, 255, 0, 255, 255, 255].into(),
        }],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    let icon = presentation.item_icon(TOTEM_IDENTIFIER, 7).unwrap();
    runtime.item_activation = Some(ItemActivation {
        identifier: TOTEM_IDENTIFIER.into(),
        metadata: 7,
        elapsed_millis: 0,
        started_clock_millis: 0,
        offsets: [0; 2],
    });
    for age in [0, 100, 500, 1_000, 1_500, 1_999] {
        runtime.item_activation.as_mut().unwrap().elapsed_millis = age;
        let mut nodes = Vec::new();
        presentation
            .append_item_activation(&runtime, &mut nodes, &mut 1, [1280.0, 720.0], [0.0; 2])
            .unwrap();
        let UiVisual::Mesh(mesh) = nodes[0].visual() else {
            panic!("activation must be textured geometry");
        };
        assert_eq!(mesh.batches()[0].texture_page, icon.page);
        assert_eq!(
            mesh.vertices()[0].uv,
            [f32::from(icon.uv[0]), f32::from(icon.uv[1])]
        );
    }
    runtime.item_activation.as_mut().unwrap().elapsed_millis = 2_000;
    let mut nodes = Vec::new();
    presentation
        .append_item_activation(&runtime, &mut nodes, &mut 1, [1280.0, 720.0], [0.0; 2])
        .unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn active_totem_follows_pack_replacement_and_removal() {
    let base_pixels = vec![20, 40, 220, 255];
    let catalog = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [5; 32],
                &[assets::IconSprite {
                    width: 1,
                    height: 1,
                    rgba8: base_pixels.clone().into(),
                }],
                &[assets::IconEntry {
                    identifier: TOTEM_IDENTIFIER.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        super::super::tests::fixture_hud(),
        catalog,
    )
    .unwrap();
    presentation.hud_frame_mut().first_person = true;
    let mut runtime = UiRuntime::new(1);
    runtime.item_activation = Some(ItemActivation {
        identifier: TOTEM_IDENTIFIER.into(),
        metadata: 7,
        elapsed_millis: 0,
        started_clock_millis: 0,
        offsets: [0; 2],
    });
    for pixels in [vec![255, 0, 255, 255], vec![0, 255, 0, 255]] {
        let icons = Arc::new(SessionIcons {
            icons: vec![SessionIcon {
                identifier: TOTEM_IDENTIFIER.into(),
                metadata: 7,
                width: 1,
                height: 1,
                rgba8: pixels.clone().into(),
            }],
            ..Default::default()
        });
        session_icons::observe(&mut presentation, Some(&icons));
        assert_eq!(
            presentation
                .item_sprite(TOTEM_IDENTIFIER, 7, 64)
                .unwrap()
                .rgba8,
            pixels
        );
        let mut nodes = Vec::new();
        presentation
            .append_item_activation(&runtime, &mut nodes, &mut 1, [1280.0, 720.0], [0.0; 2])
            .unwrap();
        let UiVisual::Mesh(mesh) = nodes[0].visual() else {
            panic!("missing active item");
        };
        assert_eq!(
            mesh.batches()[0].texture_page,
            presentation.item_icon(TOTEM_IDENTIFIER, 7).unwrap().page
        );
    }
    session_icons::observe(&mut presentation, None);
    assert_eq!(
        presentation
            .item_sprite(TOTEM_IDENTIFIER, 7, 64)
            .unwrap()
            .rgba8,
        base_pixels
    );
}

#[test]
fn camera_activation_keeps_full_viewport_coordinates_below_screen_overlays() {
    let icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [0; 32],
                &[assets::IconSprite {
                    width: 1,
                    height: 1,
                    rgba8: vec![20, 40, 220, 255].into(),
                }],
                &[assets::IconEntry {
                    identifier: TOTEM_IDENTIFIER.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        super::super::tests::fixture_hud(),
        icons,
    )
    .unwrap();
    presentation.hud_frame_mut().first_person = true;
    presentation.hud_frame_mut().sleep.observe(true, 0);
    let mut runtime = UiRuntime::new(1);
    runtime.item_activation = Some(ItemActivation {
        identifier: TOTEM_IDENTIFIER.into(),
        metadata: 0,
        elapsed_millis: 1_000,
        started_clock_millis: 0,
        offsets: [0; 2],
    });
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let icon = presentation.item_icon(TOTEM_IDENTIFIER, 0).unwrap();
    let plain = presentation
        .build(
            &player,
            &runtime,
            1_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(plain.batches[0].texture_page, u32::from(icon.page));
    assert!(
        plain
            .batches
            .iter()
            .skip(1)
            .any(|batch| batch.texture_page != u32::from(icon.page))
    );
    presentation.set_safe_area(ui::SafeArea::new(70.0, 40.0, 30.0, 20.0).unwrap());
    let inset = presentation
        .build(
            &player,
            &runtime,
            1_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let activation = |input: &render_model::UiRenderInput| {
        input.indices[..6]
            .iter()
            .map(|index| input.vertices[*index as usize])
            .collect::<Vec<_>>()
    };
    assert_eq!(activation(&plain), activation(&inset));
    runtime.item_activation.as_mut().unwrap().elapsed_millis = 1_050;
    let advanced = presentation
        .build(
            &player,
            &runtime,
            1_050,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    for input in [&plain, &inset, &advanced] {
        let clip = input.batches[0].scissor;
        assert_eq!([clip.x, clip.y, clip.width, clip.height], [0, 0, 1280, 720]);
    }
    let small = presentation
        .build(
            &player,
            &runtime,
            1_050,
            [320, 200],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let clip = small.batches[0].scissor;
    assert_eq!([clip.x, clip.y, clip.width, clip.height], [0, 0, 320, 200]);
}
