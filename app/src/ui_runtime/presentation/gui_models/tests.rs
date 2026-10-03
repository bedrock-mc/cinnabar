use super::*;

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
#[ignore = "requires installed local carriers (make assets)"]
fn installed_carriers_admit_geometry_and_keep_diagnostic_world_fallback() {
    use crate::asset_startup::equipment_carrier::equipment_asset_path;
    use crate::asset_startup::{DEFAULT_ASSET_PATH, ENTITY_ASSETS_FILENAME, icon_asset_path};
    use assets::RuntimeIconCatalog;
    let world_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(DEFAULT_ASSET_PATH);
    let world = std::fs::read(&world_path).expect("make assets: world carrier");
    let entities = std::fs::read(world_path.with_file_name(ENTITY_ASSETS_FILENAME))
        .expect("make assets: entity carrier");
    let icons = std::fs::read(icon_asset_path(&world_path)).expect("make assets: icon carrier");
    let world = RuntimeAssets::decode(&world).unwrap();
    let entities = RuntimeEntityAssets::decode(&entities).unwrap();
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        super::super::tests::fixture_font(),
        super::super::tests::fixture_hud(),
        icons,
    )
    .unwrap();
    if let Ok(bytes) = std::fs::read(equipment_asset_path(&world_path)) {
        presentation.set_equipment_catalog(Some(Arc::new(
            assets::RuntimeEquipmentCatalog::decode(&bytes).unwrap(),
        )));
    }
    presentation.set_gui_models(&world, &entities).unwrap();
    for identifier in ["minecraft:dirt", "minecraft:grass_block"] {
        let icon = presentation.item_icon(identifier, 0).unwrap();
        assert!(presentation.gui_models.models.contains_key(&icon_key(icon)));
        let model = presentation
            .gui_models
            .held
            .get(&assets::ItemVisualKey {
                identifier: identifier.into(),
                metadata: 0,
            })
            .unwrap();
        assert_eq!(model.vertices.len(), 36);
        assert!(matches!(
            model.placements[0],
            player_preview::PreviewHeldPlacement::Block
        ));
    }
    if presentation.equipment_catalog.is_some() {
        let shield = presentation
            .gui_models
            .held
            .get(&assets::ItemVisualKey {
                identifier: "minecraft:shield".into(),
                metadata: 0,
            })
            .unwrap();
        assert!(matches!(
            shield.placements[0],
            player_preview::PreviewHeldPlacement::Authored { .. }
        ));
        assert!(shield.vertices.len() > 36);
        let mesh = player_preview::geometry::mesh(
            Default::default(),
            Default::default(),
            0.0,
            presentation.item_icon("minecraft:dirt", 0).unwrap(),
            &Default::default(),
            [None; 4],
            [Some(shield); 2],
            true,
        )
        .unwrap();
        for batch in mesh.batches().iter().skip(1) {
            let high = mesh.vertices()
                [batch.index_range.start as usize..batch.index_range.end as usize]
                .iter()
                .map(|vertex| {
                    (player_preview::PREVIEW_FEET_Y
                        - vertex.position[1] * player_preview::PREVIEW_HEIGHT as f32)
                        / player_preview::PREVIEW_PIXELS_PER_BLOCK
                })
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                high < 2.3,
                "factory Shield must remain at the hand, not above head"
            );
        }
    }
    let beacon = presentation.item_icon("minecraft:beacon", 0).unwrap();
    assert!(
        !presentation
            .gui_models
            .models
            .contains_key(&icon_key(beacon)),
        "the translucent special model must not take the opaque cube route"
    );
    let dynamic = presentation.textures.dynamic_start();
    assert_eq!(
        presentation.textures.pages()[dynamic + MODEL_PAGE].dimensions(),
        [render::UI_MODEL_ATLAS_SIDE; 2]
    );
    assert!(presentation.textures.plan().bytes() <= render::MAX_UI_TEXTURE_BYTES);
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    let grass = presentation.item_icon("minecraft:grass_block", 0).unwrap();
    assert!(
        presentation
            .gui_models
            .models
            .contains_key(&icon_key(grass))
    );
}
