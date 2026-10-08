use super::*;

/// Distinct colored variants let packing tests detect accidental key collapse.
fn sprite(metadata: u32) -> SessionIcon {
    SessionIcon {
        identifier: Arc::from("test:variant"),
        metadata,
        width: 16,
        height: 16,
        rgba8: vec![metadata as u8; 16 * 16 * 4].into(),
    }
}

#[test]
fn metadata_variants_get_distinct_uvs_and_large_catalogs_grow_the_page() {
    let icons = SessionIcons {
        icons: (0..600).map(sprite).collect(),
        ..Default::default()
    };
    let packed = pack(&icons, 7).unwrap();
    let variants = &packed.refs["test:variant"];
    assert_eq!(variants.len(), 600);
    assert_ne!(variants[&0].uv, variants[&599].uv);
    assert!(packed.page.pixels().len() > (MIN_PAGE_SIDE * MIN_PAGE_SIDE * 4) as usize);
    assert_eq!(variants[&599].page, 7);
}

// A custom block item with a cube sheet draws as the GUI cube vanilla block items draw, sampling
// the sheet rather than scaling its flat thumbnail; an item without one stays a flat sprite.
#[test]
fn block_sheet_items_draw_the_gui_cube_over_their_thumbnail() {
    use ui::{UiNode, UiNodeId, UiPoint, UiRect, UiVisual};
    let mut presentation = UiPresentationRuntime::new(super::super::tests::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    let thumbnail = |identifier: &str| SessionIcon {
        identifier: Arc::from(identifier),
        metadata: 0,
        width: 32,
        height: 32,
        rgba8: vec![9; 32 * 32 * 4].into(),
    };
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    let icons = Arc::new(SessionIcons {
        icons: vec![thumbnail("test:controller"), thumbnail("test:gem")],
        block_sheets: vec![SessionIcon {
            identifier: Arc::from("test:controller"),
            metadata: 0,
            width,
            height,
            rgba8: vec![255; (width * height * 4) as usize].into(),
        }],
        misses: HashMap::new(),
        ..Default::default()
    });
    observe(&mut presentation, Some(&icons));
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(16.0, 16.0).unwrap(),
    )
    .unwrap();
    let slot = |icon: IconRef| {
        UiNode::new(UiNodeId::new(1), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: icon.page,
            uv: icon.uv,
            color: [255; 4],
        })
    };
    let controller = presentation.item_icon("test:controller", 0).unwrap();
    let gem = presentation.item_icon("test:gem", 0).unwrap();
    let mut nodes = vec![slot(controller), slot(gem)];
    presentation.apply_gui_models(&mut nodes);
    let UiVisual::Mesh(mesh) = nodes[0].visual() else {
        panic!("a custom cube block item must draw GUI geometry");
    };
    let inside = |[u, v]: [f32; 2], uv: [u16; 4]| {
        u >= f32::from(uv[0])
            && u <= f32::from(uv[2])
            && v >= f32::from(uv[1])
            && v <= f32::from(uv[3])
    };
    assert!(
        mesh.vertices()
            .iter()
            .all(|vertex| !inside(vertex.uv, controller.uv)),
        "the cube samples the sheet, not the thumbnail"
    );
    assert!(matches!(nodes[1].visual(), UiVisual::Sprite { .. }));
}

#[test]
fn large_session_icons_install_without_blocking_later_server_ui_textures() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use super::super::forms::{ServerUiPack, pack_harness, tests::mini_engine_presentation};

    let mut presentation = mini_engine_presentation();
    let static_identity = presentation.textures.static_identity();
    let icons = Arc::new(SessionIcons {
        icons: (0..600).map(sprite).collect(),
        block_sheets: Vec::new(),
        misses: HashMap::new(),
        ..Default::default()
    });
    observe(&mut presentation, Some(&icons));
    let icon = presentation.item_icon("test:variant", 599).unwrap();
    let page = &presentation.textures.pages()[usize::from(icon.page)];
    assert_eq!(
        page.dimensions(),
        [MIN_PAGE_SIDE * 2; 2],
        "session icon references must address the installed enlarged page"
    );
    let pixel =
        ((u32::from(icon.uv[1]) * page.dimensions()[0] + u32::from(icon.uv[0])) * 4) as usize;
    assert_eq!(&page.pixels()[pixel..pixel + 4], &[599u32 as u8; 4]);
    assert_eq!(presentation.textures.static_identity(), static_identity);

    let color = [17, 91, 203, 255];
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = "textures/ui/session_icon_upload_witness";
    presentation.set_server_ui_pack(&ServerUiPack {
        textures: vec![(format!("{path}.png"), png.into_inner())],
        ..Default::default()
    });
    let runtime = pack_harness::image_form(
        &mut player_runtime,
        "Image",
        &["Image"],
        vec![Some(protocol::FormButtonImage::Path(path.into()))],
    );
    let nodes = pack_harness::render(&mut presentation, &runtime, [1280, 720], 1.0);
    let server_page = presentation.textures.dynamic_start() + dynamic_textures::SERVER_UI_PAGE;
    let uv = nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Sprite {
                texture_page, uv, ..
            } if usize::from(*texture_page) == server_page => Some(*uv),
            _ => None,
        })
        .expect("the form draws the server texture");
    let page = &presentation.textures.pages()[server_page];
    let pixel = ((u32::from(uv[1]) * page.dimensions()[0] + u32::from(uv[0])) * 4) as usize;
    assert_eq!(
        &page.pixels()[pixel..pixel + 4],
        &color,
        "a large session icon page must not block later server UI uploads"
    );
    assert_eq!(presentation.textures.static_identity(), static_identity);
}

#[test]
fn stack_icon_identity_retains_loaded_projectile_and_local_frame_override() {
    for projectile in ["minecraft:arrow", "minecraft:firework_rocket"] {
        let frame = inventory::crossbow_animation_frame(None, 0, Some(projectile), false);
        assert_eq!(
            UiPresentationRuntime::item_icon_key("minecraft:crossbow", 73, Some(projectile), None),
            ("minecraft:crossbow_pulling", frame - 1),
        );
        assert_eq!(
            UiPresentationRuntime::item_icon_key(
                "minecraft:crossbow",
                73,
                Some(projectile),
                Some(0)
            ),
            ("minecraft:crossbow", 73),
        );
    }
    assert_eq!(
        UiPresentationRuntime::item_icon_key("minecraft:crossbow", 73, None, None),
        ("minecraft:crossbow", 73),
    );
    assert_eq!(
        UiPresentationRuntime::item_icon_key(
            "minecraft:stone",
            2,
            Some("minecraft:arrow"),
            Some(1)
        ),
        ("minecraft:stone", 2),
    );
}

#[test]
fn review_duplicate_icon_selection_precedes_height_sorting() {
    let first = sprite(0);
    let mut second = sprite(0);
    second.height = 32;
    second.rgba8 = vec![9; 16 * 32 * 4].into();
    let icons = SessionIcons {
        icons: vec![first, second],
        ..Default::default()
    };
    let refs = pack(&icons, 7).unwrap().refs;
    let uv = refs["test:variant"][&0].uv;
    assert_eq!(uv[3] - uv[1], 16);
}

#[test]
fn review_oversized_icons_are_rejected_before_gutter_arithmetic() {
    let invalid = SessionIcon {
        width: u32::MAX,
        height: u32::MAX,
        rgba8: Box::new([]),
        ..sprite(1)
    };
    let icons = SessionIcons {
        icons: vec![invalid, sprite(0)],
        ..Default::default()
    };
    let refs = pack(&icons, 7).unwrap().refs;
    assert!(refs["test:variant"].contains_key(&0));
    assert!(!refs["test:variant"].contains_key(&1));
}
