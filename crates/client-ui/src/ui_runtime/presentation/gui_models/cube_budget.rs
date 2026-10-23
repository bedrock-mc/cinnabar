//! See-through cube sources remain optional under the real GUI atlas budget.

use super::*;
use assets::{IconBlockSheet, IconEntry, IconSprite, ItemVisualKey, RuntimeIconCatalog};

/// Fills a bounded shelf with distinct opaque sheets and keeps the carrier's sheet-count limit.
/// Required equipment textures fill any remaining atlas pages in larger budget fixtures.
fn sheets_at_capacity(pages: usize) -> Vec<IconSprite> {
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let mut cutout = vec![255; usize::from(width) * usize::from(height) * 4];
    cutout[3] = 0;
    let mut sheets = vec![IconSprite {
        width,
        height,
        rgba8: cutout.into(),
    }];
    let mut atlas = atlas::Atlas::new(1, pages);
    let limit = assets::MAX_ICON_BLOCK_SHEETS.min(assets::MAX_ICON_SPRITES);
    for index in 1..limit {
        let [red, green, blue, _] = (index as u32).to_le_bytes();
        let rgba8 = [red, green, blue, 255].repeat(usize::from(width) * usize::from(height));
        if atlas.insert([width, height], &rgba8).is_err() {
            assert_eq!(atlas.page_count(), pages);
            return sheets;
        }
        sheets.push(IconSprite {
            width,
            height,
            rgba8: rgba8.into(),
        });
    }
    panic!("the requested shelf capacity exceeds the carrier's sheet limit; use equipment pages")
}

/// Builds original compiled item routes and admitted sheets without needing game carriers.
fn catalog(sheets: Vec<IconSprite>) -> (RuntimeEntityAssets, RuntimeIconCatalog) {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:budget"}}}"#;
    let mut compiled = pack_entity_compiler::compile_entity_pack(vec![(
        "entity/budget.json".into(),
        entity.to_vec(),
    )])
    .unwrap()
    .unwrap()
    .assets;
    compiled.block_visual_count = sheets.len() as u32;
    compiled.item_visuals = (0..sheets.len())
        .map(|index| assets::ItemVisualDefinition {
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            fuel: assets::item::fuel::ItemFuel::Unresolved,
            durability: assets::item::durability::ItemDurability::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            key: ItemVisualKey {
                identifier: format!("fixture:cube_{index:05}").into(),
                metadata: 0,
            },
            source: 0,
            route: ItemVisualDefinitionRoute::BlockItem {
                block_visual: assets::BlockVisualId(index as u32),
            },
            first_person: assets::ItemDisplayTransform::identity(),
            third_person: assets::ItemDisplayTransform::identity(),
            dropped: assets::ItemDisplayTransform::identity(),
        })
        .collect();
    let entities = RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap();
    let entries: Vec<_> = entities
        .item_visuals()
        .iter()
        .enumerate()
        .map(|(index, definition)| IconEntry {
            identifier: definition.key.identifier.clone(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect();
    let blocks: Vec<_> = (0..sheets.len())
        .map(|index| IconBlockSheet {
            visual: assets::BlockVisualId(index as u32),
            sprite: index as u32,
        })
        .collect();
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            entities.source_manifest_sha256(),
            &sheets,
            &entries,
            &blocks,
        )
        .unwrap(),
    )
    .unwrap();
    (entities, icons)
}

/// Keeps the same thumbnail control before and after optional model installation or reclamation.
fn node(icon: IconRef) -> UiNode {
    let bounds = ui::UiRect::new(
        ui::UiPoint::new(0.0, 0.0).unwrap(),
        ui::UiPoint::new(32.0, 32.0).unwrap(),
    )
    .unwrap();
    UiNode::new(ui::UiNodeId::new(1), None, bounds).with_visual(UiVisual::Sprite {
        texture_page: icon.page,
        uv: icon.uv,
        color: [255; 4],
    })
}

#[test]
fn full_required_cube_atlas_keeps_cutout_thumbnail_without_failing_installation() {
    let cube_pages = 1;
    let (entities, icons) = catalog(sheets_at_capacity(cube_pages));
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    presentation.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            entities.source_manifest_sha256(),
            Vec::new(),
            (cube_pages..MODEL_PAGES)
                .map(|page| assets::EquipmentTexture {
                    identifier: format!("textures/fixture/budget_{page:02}").into(),
                    width: side,
                    height: side,
                    rgba8: [page as u8, 17, 23, 255]
                        .repeat(usize::from(side) * usize::from(side))
                        .into(),
                })
                .collect(),
        )
        .unwrap(),
    )));
    let cutout = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    let opaque = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    assert_eq!(presentation.gui_models.sources.required_pages, MODEL_PAGES);
    assert_eq!(presentation.gui_models.sources.pages.len(), MODEL_PAGES);
    assert!(presentation.gui_models.sources.optional_models.is_empty());
    let mut nodes = [node(cutout), node(opaque)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(
        nodes[0], thumbnail,
        "budget pressure retains the cutout thumbnail"
    );
    assert!(
        matches!(nodes[1].visual(), UiVisual::Mesh(_)),
        "opaque cubes remain geometry"
    );
}

#[test]
fn cutout_cube_pages_are_reclaimed_without_changing_required_cube_texels() {
    let (entities, icons) = catalog(sheets_at_capacity(1));
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let cutout = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    let opaque = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    assert_eq!(presentation.gui_models.sources.required_pages, 1);
    assert_eq!(presentation.gui_models.sources.pages.len(), 2);
    let required = presentation.gui_models.sources.pages[0].pixels().to_vec();
    let mut drawn = [node(cutout)];
    presentation.apply_gui_models(&mut drawn);
    assert!(matches!(drawn[0].visual(), UiVisual::Mesh(_)));
    let first = presentation.textures.dynamic_start() + MODEL_PAGE;
    assert!(presentation.gui_models.discard_optional_models(first));
    assert_eq!(presentation.gui_models.sources.pages.len(), 1);
    assert_eq!(presentation.gui_models.sources.pages[0].pixels(), required);
    assert!(
        !presentation
            .gui_models
            .sources
            .textures
            .contains_key(&atlas::key(
                assets::BLOCK_ITEM_SHEET_SIZE,
                &presentation.icon_catalog.as_deref().unwrap().sprites()[0].rgba8,
            )),
        "reclaimed optional-page textures are no longer addressable"
    );
    let mut nodes = [node(cutout), node(opaque)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(nodes[0], thumbnail);
    assert!(matches!(nodes[1].visual(), UiVisual::Mesh(_)));
}
