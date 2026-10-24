//! Ordinary held models use the same routes and geometry in previews and world hands.

use super::*;
use assets::{ItemVisualKey, RuntimeIconCatalog};
use player_preview::PreviewHeldPlacement;

/// Compiles original player hand bones and binds one item to a carried pane model.
fn pane_fixture() -> (RuntimeAssets, RuntimeEntityAssets, Arc<RuntimeIconCatalog>) {
    let mut compiled = pack_entity_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","geometry":{"default":"geometry.player"},"render_controllers":["controller.render.player"]}}}"#.to_vec()),
        ("models/player.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.player","texture_width":64,"texture_height":64},"bones":[{"name":"body"},{"name":"rightItem","parent":"body","pivot":[-6,12,0]},{"name":"leftItem","parent":"body","pivot":[6,12,0]}]}]}"#.to_vec()),
        ("render_controllers/player.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.player":{"geometry":"Geometry.default"}}}"#.to_vec()),
    ]).unwrap().unwrap().assets;
    compiled.block_visual_count = 2;
    compiled.item_visuals = vec![assets::ItemVisualDefinition {
        key: ItemVisualKey {
            identifier: "minecraft:glass_pane".into(),
            metadata: 0,
        },
        source: 0,
        route: ItemVisualDefinitionRoute::BlockItem {
            block_visual: assets::BlockVisualId(1),
        },
        grip: assets::item::grip::ItemGrip::Unresolved,
        glint: assets::item::glint::ItemGlint::Unresolved,
        fuel: assets::item::fuel::ItemFuel::Unresolved,
        durability: assets::item::durability::ItemDurability::Unresolved,
        armour: assets::item::armour::ItemArmour::Unresolved,
        first_person: assets::ItemDisplayTransform::identity(),
        third_person: assets::ItemDisplayTransform::identity(),
        dropped: assets::ItemDisplayTransform::identity(),
    }]
    .into();
    let entities = RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap();
    let world = short_models::world_with_width(entities.source_manifest_sha256(), 256, 128, 32);
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_blocks(
            entities.source_manifest_sha256(),
            &[assets::IconSprite {
                width: 16,
                height: 16,
                rgba8: [50, 90, 130, 200].repeat(256).into(),
            }],
            &[assets::IconEntry {
                identifier: "minecraft:glass_pane".into(),
                metadata: 0,
                sprite: 0,
            }],
            &[],
            &[assets::IconBlockModel {
                sprite: 0,
                visual: assets::BlockVisualId(1),
                held_sprite: None,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    (world, entities, Arc::new(icons))
}

#[test]
fn player_preview_holds_the_carried_glass_plate_in_both_hands() {
    let (world, entities, icons) = pane_fixture();
    let mut preview = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::clone(&icons),
    )
    .unwrap();
    preview.set_gui_models(&world, &entities).unwrap();
    let held = &preview.gui_models.sources.held[&entities.item_visuals()[0].key];
    assert!(
        !held.is_sprite(),
        "the paper doll must not extrude the inventory thumbnail"
    );
    let model = held.model(&icons).unwrap();
    assert!(
        model
            .placements
            .iter()
            .all(|placement| matches!(placement, PreviewHeldPlacement::Block))
    );
    assert_eq!(model.vertices.len(), 36);
    for vertex in &*model.vertices {
        assert_eq!(vertex.position[0].abs(), 1.0 / 16.0);
        assert_eq!(vertex.position[1].abs(), 0.5);
        assert_eq!(vertex.position[2].abs(), 0.5);
    }
    assert_ordinary_routes(&preview, &world, &entities, &icons);
}

/// Compares actual prepared preview geometry and sprite artwork with every ordinary world route.
fn assert_ordinary_routes(
    preview: &UiPresentationRuntime,
    world: &RuntimeAssets,
    entities: &RuntimeEntityAssets,
    icons: &RuntimeIconCatalog,
) {
    let blocks = render_model::equipment::blocks::prepare_held(Some(world), entities, icons);
    for entry in icons.entries() {
        let key = ItemVisualKey {
            identifier: entry.identifier.clone(),
            metadata: entry.metadata,
        };
        let definition = entities
            .item_visuals()
            .iter()
            .find(|definition| definition.key == key);
        if definition.is_some_and(|definition| {
            matches!(definition.route, ItemVisualDefinitionRoute::EmptyHand)
        }) {
            assert!(!preview.gui_models.sources.held.contains_key(&key));
            continue;
        }
        let held = preview
            .gui_models
            .sources
            .held
            .get(&key)
            .unwrap_or_else(|| panic!("missing preview for {key:?}"));
        let model = held
            .model(icons)
            .unwrap_or_else(|| panic!("empty held geometry for {key:?}"));
        let visual = definition.and_then(|definition| match definition.route {
            ItemVisualDefinitionRoute::BlockItem { block_visual } => Some(block_visual.0),
            _ => None,
        });
        let expected = visual.filter(|visual| blocks.by_visual.contains_key(visual));
        assert_eq!(
            !held.is_sprite(),
            expected.is_some(),
            "held route differs for {key:?}"
        );
        if let Some(visual) = expected {
            let cube;
            let vertices = if let Some(vertices) = blocks.models.get(&visual) {
                vertices
            } else {
                cube = render_model::textured_cube_vertices(
                    render_model::equipment::blocks::face_rects([0.0, 0.0, 1.0, 1.0]),
                );
                &cube
            };
            assert_eq!(
                &*model.vertices, vertices,
                "held geometry differs for {key:?}"
            );
            assert!(
                model
                    .placements
                    .iter()
                    .all(|placement| matches!(placement, PreviewHeldPlacement::Block))
            );
        } else {
            let sprite = icons
                .lookup_held_index(&entry.identifier, entry.metadata)
                .unwrap();
            let source = preview.icon_refs.as_deref().unwrap()[sprite];
            assert_eq!(
                (model.source.page, model.source.uv),
                (source.page, source.uv)
            );
        }
    }
}

#[test]
fn every_installed_item_has_the_same_ordinary_held_route_in_preview_and_world() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(assets::carriers::COMPILED_DIR);
    let read = |carrier: &assets::carriers::Carrier| {
        let path = root.join(carrier.output);
        std::fs::read(&path).map_err(|error| {
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{}: {error}", path.display());
            eprintln!("skipping every_installed_item_has_the_same_ordinary_held_route_in_preview_and_world: missing {}; run make assets", path.display());
        })
    };
    let (Ok(world), Ok(entities), Ok(icons)) = (
        read(&assets::carriers::WORLD),
        read(&assets::carriers::ENTITY),
        read(&assets::carriers::ICON),
    ) else {
        return;
    };
    let world = RuntimeAssets::decode(&world).unwrap();
    let entities = RuntimeEntityAssets::decode(&entities).unwrap();
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let mut preview = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::clone(&icons),
    )
    .unwrap();
    preview.set_gui_models(&world, &entities).unwrap();
    assert_ordinary_routes(&preview, &world, &entities, &icons);
}

/// Prepares a real player catalog whose carried model uses the requested alpha policy.
fn alpha_preview(flags: u32, alpha: u8) -> (UiPresentationRuntime, RuntimeEntityAssets) {
    let (_, entities, icons) = pane_fixture();
    let world =
        short_models::world_with_material(entities.source_manifest_sha256(), 256, alpha, 32, flags);
    let mut preview = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        icons,
    )
    .unwrap();
    preview.set_gui_models(&world, &entities).unwrap();
    (preview, entities)
}

#[test]
fn preview_held_cutout_texels_render_opaque() {
    let threshold = assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD;
    for alpha in [192, threshold - 1, threshold] {
        let (preview, entities) = alpha_preview(assets::MATERIAL_FLAG_ALPHA_CUTOUT, alpha);
        let model = preview.gui_models.sources.held[&entities.item_visuals()[0].key]
            .model(preview.icon_catalog.as_deref().unwrap())
            .unwrap();
        let page = &preview.textures.pages()[usize::from(model.source.page)];
        let offset = (usize::from(model.source.uv[1]) * page.dimensions()[0] as usize
            + usize::from(model.source.uv[0]))
            * 4;
        assert_eq!(
            page.pixels()[offset + 3],
            if alpha < threshold { 0 } else { 255 },
            "accepted cutout texels must be opaque in the preview"
        );
    }
}

#[test]
fn preview_held_blend_keeps_low_alpha_texels() {
    let (preview, entities) = alpha_preview(assets::MATERIAL_FLAG_ALPHA_BLEND, 64);
    let model = preview.gui_models.sources.held[&entities.item_visuals()[0].key]
        .model(preview.icon_catalog.as_deref().unwrap())
        .unwrap();
    let mesh = player_preview::geometry::mesh(
        Default::default(),
        Default::default(),
        0.0,
        model.source,
        &Default::default(),
        [None; 4],
        [Some(&model), None],
        true,
    )
    .unwrap();
    let batch = mesh.batches().last().unwrap();
    assert!(batch.depth_write, "held models retain their depth policy");
    assert_eq!(
        batch.alpha_cutoff, None,
        "blended held texels must not be thresholded"
    );
}

#[test]
fn preview_held_opaque_material_ignores_texture_alpha() {
    let (preview, entities) = alpha_preview(0, 64);
    let model = preview.gui_models.sources.held[&entities.item_visuals()[0].key]
        .model(preview.icon_catalog.as_deref().unwrap())
        .unwrap();
    let page = &preview.textures.pages()[usize::from(model.source.page)];
    let offset = (usize::from(model.source.uv[1]) * page.dimensions()[0] as usize
        + usize::from(model.source.uv[0]))
        * 4;
    assert_eq!(page.pixels()[offset + 3], 255);
}

#[test]
fn full_model_atlas_keeps_the_unadmitted_held_model_on_its_sprite_route() {
    let (world, entities, icons) = pane_fixture();
    let mut preview = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        icons,
    )
    .unwrap();
    preview.set_equipment_catalog(Some(cubes::equipment(&entities, MODEL_PAGES)));
    preview
        .set_gui_models(&world, &entities)
        .expect("one optional held texture must not abort the player catalog");
    assert!(preview.gui_models.sources.held[&entities.item_visuals()[0].key].is_sprite());
    assert_eq!(preview.gui_models.sources.held_texture_fallbacks(), 1);
}

#[test]
fn full_model_atlas_reuses_resident_held_face_sheets() {
    let (_, entities, original) = pane_fixture();
    let world = RuntimeAssets::diagnostic();
    let mut sprites = original.sprites().to_vec();
    let sheet = sprites.len() as u32;
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    sprites.push(assets::IconSprite {
        width,
        height,
        rgba8: (0..usize::from(width) * usize::from(height))
            .flat_map(|pixel| [31, 47, 59, if pixel % 7 == 0 { 0 } else { 255 }])
            .collect::<Vec<_>>()
            .into(),
    });
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_blocks(
            entities.source_manifest_sha256(),
            &sprites,
            original.entries(),
            &[assets::IconBlockSheet {
                visual: assets::BlockVisualId(1),
                sprite: sheet,
            }],
            &[assets::IconBlockModel {
                sprite: 0,
                visual: assets::BlockVisualId(1),
                held_sprite: None,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let mut preview = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let resident = preview.icon_refs.as_deref().unwrap()[sheet as usize];
    preview.set_equipment_catalog(Some(cubes::equipment(&entities, MODEL_PAGES)));
    preview.set_gui_models(&world, &entities).unwrap();
    let held = &preview.gui_models.sources.held[&entities.item_visuals()[0].key];
    assert!(
        !held.is_sprite(),
        "resident artwork must preserve the model even when the atlas is full"
    );
    assert_eq!(
        held.model(preview.icon_catalog.as_deref().unwrap())
            .unwrap()
            .source,
        resident
    );
    assert_eq!(preview.gui_models.sources.held_texture_fallbacks(), 0);
}
