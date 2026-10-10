use {super::*, ui::IconRef};

#[test]
fn source_atlas_keeps_original_pixels_and_deduplicates_sources() {
    let pixels: Vec<u8> = (0..16 * 16)
        .flat_map(|pixel| [pixel as u8, 20, 40, 255])
        .collect();
    let mut atlas = atlas::Atlas::new(8, MODEL_PAGES);
    let first = atlas.insert([16; 2], &pixels).unwrap();
    assert_eq!(atlas.insert([16; 2], &pixels).unwrap(), first);
    let (pages, _) = atlas.finish().unwrap();
    let [width, _] = pages[0].dimensions();
    for y in 0..16 {
        for x in 0..16 {
            let source = (y * 16 + x) * 4;
            let target =
                ((usize::from(first.uv[1]) + y) * width as usize + usize::from(first.uv[0]) + x)
                    * 4;
            assert_eq!(
                &pages[0].pixels()[target..target + 4],
                &pixels[source..source + 4]
            );
        }
    }
}

#[test]
fn carried_sheet_refs_preserve_face_order_and_pixel_edges() {
    let sheet = IconRef {
        page: 5,
        uv: [
            7,
            13,
            7 + assets::BLOCK_ITEM_SHEET_SIZE[0],
            13 + assets::BLOCK_ITEM_SHEET_SIZE[1],
        ],
        glint: false,
    };
    let faces = sheet_faces(sheet);
    let side = assets::BLOCK_ITEM_FACE_SIDE;
    assert_eq!(faces[0].uv, [7, 13, 7 + side, 13 + side]);
    assert_eq!(faces[3].uv, [7, 13 + side, 7 + side, 13 + 2 * side]);
    assert!(faces.iter().all(|icon| icon.page == sheet.page));
}

#[test]
fn atlas_rejects_invalid_sources_and_page_exhaustion() {
    let mut atlas = atlas::Atlas::new(1, 1);
    assert!(atlas.insert([0, 1], &[]).is_err());
    assert!(atlas.insert([16; 2], &[0; 4]).is_err());
    atlas.insert([510; 2], &vec![255; 510 * 510 * 4]).unwrap();
    assert!(atlas.insert([1; 2], &[20; 4]).is_err());
}

#[test]
fn mesh_modulation_preserves_geometry_and_adds_glint_without_baking() {
    let icon = IconRef {
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let mesh = item_gui::cube([icon; 6]).unwrap();
    assert!(Arc::ptr_eq(
        &mesh,
        &modulated(&mesh, [255; 4], false).unwrap()
    ));
    let faded = modulated(&mesh, [255, 255, 255, 127], true).unwrap();
    for (original, result) in mesh.vertices().iter().zip(faded.vertices()) {
        assert_eq!(original.position, result.position);
        assert_eq!(result.color[3], 127);
        assert_ne!(result.style_flags & ui::UI_STYLE_GLINT, 0);
    }
}

#[test]
fn geometry_replacement_preserves_json_control_state_and_flat_sprites() {
    use ui::{UiNodeId, UiPoint, UiRect};
    let mut presentation = UiPresentationRuntime::new(super::super::tests::fixture_font()).unwrap();
    let icon = IconRef {
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let mesh = item_gui::cube([icon; 6]).unwrap();
    presentation.gui_models.enabled = true;
    presentation.gui_models.models.insert(icon_key(icon), mesh);
    let bounds = UiRect::new(
        UiPoint::new(20.0, 30.0).unwrap(),
        UiPoint::new(36.0, 46.0).unwrap(),
    )
    .unwrap();
    let model = UiNode::new(UiNodeId::new(2), Some(UiNodeId::new(1)), bounds)
        .with_focusable(true)
        .with_navigation_order(7)
        .with_clip_children(true)
        .with_visual(UiVisual::Sprite {
            texture_page: icon.page,
            uv: icon.uv,
            color: [255; 4],
        });
    let flat = UiNode::new(UiNodeId::new(3), None, bounds).with_visual(UiVisual::Sprite {
        texture_page: 1,
        uv: [16, 0, 32, 16],
        color: [255; 4],
    });
    let mut nodes = vec![model.clone(), flat.clone()];
    presentation.apply_gui_models(&mut nodes);
    let UiVisual::Mesh(mesh) = nodes[0].visual() else {
        panic!("the JSON model icon must draw geometry, not its baked thumbnail");
    };
    assert!(mesh.indices().len() > 6);
    assert_eq!(
        nodes[0],
        model.with_visual(UiVisual::Mesh(Arc::clone(mesh)))
    );
    assert_eq!(nodes[1], flat);
}

/// Most frames show no player preview, so they must not pay for its mesh.
#[test]
fn the_player_preview_mesh_is_built_only_for_a_shown_preview() {
    use std::cell::Cell;
    use ui::{UiNodeId, UiPoint, UiRect};
    let preview = IconRef {
        page: 2,
        uv: [0, 0, 32, 32],
        glint: false,
    };
    let other = IconRef {
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let mesh = item_gui::cube([other; 6]).unwrap();
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(16.0, 16.0).unwrap(),
    )
    .unwrap();
    let sprite = |id, icon: IconRef| {
        UiNode::new(UiNodeId::new(id), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: icon.page,
            uv: icon.uv,
            color: [255; 4],
        })
    };
    let builds = Cell::new(0);
    let player = || {
        builds.set(builds.get() + 1);
        Some(Arc::clone(&mesh))
    };
    let models = |key: &IconKey| (*key == icon_key(other)).then_some(&mesh);

    let mut hud = vec![sprite(1, other), sprite(2, other)];
    replace_model_icons(
        &mut hud,
        Some(icon_key(preview)),
        &BTreeSet::new(),
        player,
        models,
    );
    assert_eq!(builds.get(), 0, "no node shows the preview");
    assert!(
        hud.iter()
            .all(|node| matches!(node.visual(), UiVisual::Mesh(_)))
    );

    let mut shown = vec![sprite(1, preview), sprite(2, preview)];
    replace_model_icons(
        &mut shown,
        Some(icon_key(preview)),
        &BTreeSet::new(),
        player,
        models,
    );
    assert_eq!(builds.get(), 1, "one build serves every preview node");
    assert!(
        shown
            .iter()
            .all(|node| matches!(node.visual(), UiVisual::Mesh(_)))
    );
}

#[test]
fn live_pose_changes_only_geometry_and_original_skin_keeps_its_density() {
    let mut presentation = UiPresentationRuntime::new(super::super::tests::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    let mut skin = vec![255; 128 * 128 * 4];
    skin[4..8].copy_from_slice(&[12, 34, 56, 255]);
    presentation.set_player_preview_skin(Some(&skin), Default::default());
    let textures = Arc::clone(&presentation.textures);
    let source = &textures.pages()[textures.dynamic_start() + SKIN_PAGE];
    assert_eq!(source.dimensions(), [128; 2]);
    assert_eq!(source.pixels(), skin);
    presentation.player_preview_view = player_preview::PreviewView::Live {
        offset: [13.123, -7.321],
    };
    presentation.player_preview_bob = 2.345;
    presentation.set_player_preview_skin(Some(&skin), Default::default());
    assert!(Arc::ptr_eq(&textures, &presentation.textures));
    let mesh = presentation.gui_player_mesh().unwrap();
    assert!(mesh.indices().len() > 6);
    assert!(
        mesh.batches()
            .iter()
            .all(|batch| { batch.texture_page == (textures.dynamic_start() + SKIN_PAGE) as u16 })
    );
    // Equal allocations reuse the frame; changed bytes at the same address refresh it.
    presentation.set_player_preview_skin(Some(&skin.clone()), Default::default());
    assert!(Arc::ptr_eq(&textures, &presentation.textures));
    skin[4..8].copy_from_slice(&[65, 43, 21, 255]);
    presentation.set_player_preview_skin(Some(&skin), Default::default());
    assert!(!Arc::ptr_eq(&textures, &presentation.textures));
    assert_eq!(
        presentation.textures.pages()[presentation.textures.dynamic_start() + SKIN_PAGE].pixels(),
        skin
    );
    super::super::dynamic_textures::observe_session(&mut presentation, 1);
    super::super::dynamic_textures::observe_session(&mut presentation, 2);
    assert!(presentation.gui_models.skin.is_none());
}

#[test]
fn review_faded_block_models_keep_single_layer_thumbnail_opacity() {
    use assets::gui_item::{GuiBlockQuad, cube_face};
    use ui::{UiNodeId, UiPoint, UiRect};
    let mut presentation = UiPresentationRuntime::new(super::super::tests::fixture_font()).unwrap();
    let icon = IconRef {
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let quads: Vec<_> = [assets::BlockFace::Down, assets::BlockFace::Up]
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            (
                GuiBlockQuad {
                    corners,
                    uvs,
                    material: 0,
                },
                icon,
            )
        })
        .into();
    presentation.gui_models.enabled = true;
    presentation
        .gui_models
        .models
        .insert(icon_key(icon), item_gui::block_model(&quads).unwrap());
    presentation
        .gui_models
        .optional_models
        .insert(icon_key(icon));
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(32.0, 32.0).unwrap(),
    )
    .unwrap();
    for alpha in [0, 127, 254, 255] {
        for glint in [false, true] {
            let visual = if glint {
                UiVisual::GlintSprite {
                    texture_page: icon.page,
                    uv: icon.uv,
                    color: [255, 255, 255, alpha],
                }
            } else {
                UiVisual::Sprite {
                    texture_page: icon.page,
                    uv: icon.uv,
                    color: [255, 255, 255, alpha],
                }
            };
            let original = UiNode::new(UiNodeId::new(1), None, bounds).with_visual(visual);
            let mut nodes = [original.clone()];
            presentation.apply_gui_models(&mut nodes);
            if alpha == 255 {
                assert!(matches!(nodes[0].visual(), UiVisual::Mesh(_)));
            } else {
                assert_eq!(
                    nodes[0], original,
                    "faded block controls keep one thumbnail layer"
                );
            }
        }
    }
}
