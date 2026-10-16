use super::super::*;
use assets::{EquipmentTexture, RuntimeEquipmentCatalog, RuntimeIconCatalog};
use ui::{UiNodeId, UiPoint, UiRect};

mod alpha;

const ITEM: &str = "fixture:cutout_cube";
const VISUAL: assets::BlockVisualId = assets::BlockVisualId(1);

/// Creates two item icons sharing one cut-out cube sheet, without installed carriers.
fn catalog() -> (UiPresentationRuntime, RuntimeEntityAssets) {
    catalog_for(ITEM, 255, 255)
}

/// Gives the face sheet and baked thumbnail separate alpha values for material-mode assertions.
pub(super) fn catalog_for(
    item: &str,
    alpha: u8,
    thumbnail_alpha: u8,
) -> (UiPresentationRuntime, RuntimeEntityAssets) {
    catalog_for_visual(item, alpha, thumbnail_alpha, VISUAL)
}

/// Binds the shared original catalog to a startup or appended world visual.
pub(super) fn catalog_for_visual(
    item: &str,
    alpha: u8,
    thumbnail_alpha: u8,
    visual: assets::BlockVisualId,
) -> (UiPresentationRuntime, RuntimeEntityAssets) {
    let entity_path = "entity/fixture.json";
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:cube","geometry":{"default":"geometry.fixture"},"render_controllers":["controller.render.fixture"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":16,"texture_height":16},"bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#;
    let render = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default"}}}"#;
    let mut compiled = pack_entity_compiler::compile_entity_pack(vec![
        (entity_path.into(), entity.to_vec()),
        ("models/entity/fixture.geo.json".into(), geometry.to_vec()),
        ("render_controllers/fixture.json".into(), render.to_vec()),
    ])
    .unwrap()
    .unwrap()
    .assets;
    compiled.block_visual_count = visual.0 + 1;
    let source = compiled
        .sources
        .iter()
        .position(|source| source.path.as_ref() == entity_path)
        .unwrap() as u32;
    compiled.item_visuals = (0..2)
        .map(|metadata| assets::ItemVisualDefinition {
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            fuel: assets::item::fuel::ItemFuel::Unresolved,
            durability: assets::item::durability::ItemDurability::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            key: assets::ItemVisualKey {
                identifier: item.into(),
                metadata,
            },
            source,
            route: ItemVisualDefinitionRoute::BlockItem {
                block_visual: visual,
            },
            first_person: assets::ItemDisplayTransform::identity(),
            third_person: assets::ItemDisplayTransform::identity(),
            dropped: assets::ItemDisplayTransform::identity(),
        })
        .collect();
    let entities = RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap();
    let mut sprites: Vec<_> = (0..2)
        .map(|variant| assets::IconSprite {
            width: 16,
            height: 16,
            rgba8: [variant, 70, 80, thumbnail_alpha].repeat(16 * 16).into(),
        })
        .collect();
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let sheet = sprites.len() as u32;
    sprites.push(assets::IconSprite {
        width,
        height,
        rgba8: (0..usize::from(width) * usize::from(height))
            .flat_map(|pixel| [20, 40, 60, if pixel % 7 == 0 { 0 } else { alpha }])
            .collect::<Vec<_>>()
            .into(),
    });
    let entries: Vec<_> = entities
        .item_visuals()
        .iter()
        .map(|definition| assets::IconEntry {
            identifier: definition.key.identifier.clone(),
            metadata: definition.key.metadata,
            sprite: definition.key.metadata,
        })
        .collect();
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            entities.source_manifest_sha256(),
            &sprites,
            &entries,
            &[assets::IconBlockSheet {
                visual,
                sprite: sheet,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    (presentation, entities)
}

/// Fills the requested number of atlas pages with distinct required equipment textures.
fn equipment(entities: &RuntimeEntityAssets, pages: usize) -> Arc<RuntimeEquipmentCatalog> {
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            entities.source_manifest_sha256(),
            Vec::new(),
            (0..pages)
                .map(|page| EquipmentTexture {
                    identifier: format!("textures/fixture/page_{page:02}").into(),
                    width: side,
                    height: side,
                    rgba8: [page as u8, 90, 100, 255]
                        .repeat(usize::from(side) * usize::from(side))
                        .into(),
                })
                .collect(),
        )
        .unwrap(),
    )
}

/// Makes an icon control with the requested opacity and glint.
fn control(icon: IconRef, alpha: u8, glint: bool) -> UiNode {
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(32.0, 32.0).unwrap(),
    )
    .unwrap();
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
    UiNode::new(UiNodeId::new(1), None, bounds).with_visual(visual)
}

#[test]
fn cutout_cubes_show_outward_faces_and_fade_as_a_single_layer() {
    let (mut presentation, entities) = catalog();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    for entry in presentation.icon_catalog.as_deref().unwrap().entries() {
        let icon = presentation
            .item_icon(&entry.identifier, entry.metadata)
            .unwrap();
        let original = &presentation.gui_models.models[&icon_key(icon)];
        assert_eq!(
            original.indices().len(),
            assets::gui_item::CUBE_FACES.len() * 6
        );
        for alpha in [255, 254, 127, 0] {
            for glint in [false, true] {
                let control = control(icon, alpha, glint);
                let mut nodes = [control.clone()];
                presentation.apply_gui_models(&mut nodes);
                if alpha < 255 {
                    assert_eq!(
                        nodes[0], control,
                        "fading applies opacity to the composed icon once"
                    );
                    continue;
                }
                let UiVisual::Mesh(faded) = nodes[0].visual() else {
                    panic!("a cutout cube must draw its outward faces at full opacity");
                };
                assert_eq!(faded.indices(), original.indices());
                assert_eq!(faded.batches(), original.batches());
                assert_eq!(faded.vertices().len(), original.vertices().len());
                for (before, after) in original.vertices().iter().zip(faded.vertices()) {
                    let mut expected = *before;
                    expected.color[3] = alpha;
                    if glint {
                        expected.style_flags |= u16::from(ui::UI_STYLE_GLINT);
                    }
                    assert_eq!(*after, expected);
                }
            }
        }
    }
}

#[test]
fn actor_flames_reclaim_cutout_cube_pages_after_required_equipment() {
    let (mut presentation, entities) = catalog();
    let required = MODEL_PAGES - 2;
    let equipment = equipment(&entities, required);
    presentation.set_equipment_catalog(Some(Arc::clone(&equipment)));
    let side = render_model::UI_MODEL_ATLAS_SIDE - 2;
    let flame = assets::ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: side,
        height: side * 2,
        rgba8: (0..2)
            .flat_map(|frame| [frame, 110, 120, 255].repeat((side * side) as usize))
            .collect::<Vec<_>>()
            .into(),
    };
    presentation.set_gui_fire_texture(Some(&flame)).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .expect("required equipment and flames must fit after optional cubes are discarded");
    assert_eq!(presentation.gui_models.pages.len(), MODEL_PAGES);
    for (page, texture) in presentation
        .gui_models
        .pages
        .iter()
        .zip(equipment.textures())
    {
        assert_eq!(page.pixels(), texture.rgba8.as_ref());
    }
    let frames = &presentation.gui_models.fire.frames;
    assert_eq!(frames.len(), 2);
    let first = presentation.textures.dynamic_start() + MODEL_PAGE;
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(usize::from(frame.page), first + required + index);
        let page = &presentation.gui_models.pages[required + index];
        let offset = (usize::from(frame.uv[1]) * page.dimensions()[0] as usize
            + usize::from(frame.uv[0]))
            * 4;
        assert_eq!(
            &page.pixels()[offset..offset + 4],
            &[index as u8, 110, 120, 255]
        );
    }
    for entry in presentation.icon_catalog.as_deref().unwrap().entries() {
        let icon = presentation
            .item_icon(&entry.identifier, entry.metadata)
            .unwrap();
        let original = control(icon, 255, false);
        let mut nodes = [original.clone()];
        presentation.apply_gui_models(&mut nodes);
        assert_eq!(
            nodes[0], original,
            "discarded cubes retain their thumbnails"
        );
    }
}

#[test]
fn a_full_required_atlas_keeps_cutout_cube_thumbnails() {
    let (mut presentation, entities) = catalog();
    presentation.set_equipment_catalog(Some(equipment(&entities, MODEL_PAGES)));
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .expect("optional cube sources must not make required equipment fail");
    assert_eq!(presentation.gui_models.pages.len(), MODEL_PAGES);
    for entry in presentation.icon_catalog.as_deref().unwrap().entries() {
        let icon = presentation
            .item_icon(&entry.identifier, entry.metadata)
            .unwrap();
        let original = control(icon, 254, true);
        let mut nodes = [original.clone()];
        presentation.apply_gui_models(&mut nodes);
        assert_eq!(nodes[0], original, "a full atlas retains the icon sprite");
    }
}

/// Uses original unit-cube faces with a shortened vertical extent, like a portal frame.
fn carried_geometry(height: f32) -> Vec<render_model::ActorRigVertex> {
    assets::BlockFace::ALL
        .into_iter()
        .flat_map(|face| {
            let (positions, _) = assets::gui_item::cube_face(face);
            positions.map(|position| render_model::ActorRigVertex {
                position: [
                    position[0] - 0.5,
                    position[1] * height - height * 0.5,
                    position[2] - 0.5,
                ],
                normal: [0.0, 1.0, 0.0],
                uv: [0.0; 2],
                back_uv: [0.0; 2],
                bone_index: 0,
                surface: render_model::ActorRigSurface::SINGLE_FACE,
                glint_origin: [0.0; 2],
            })
        })
        .collect()
}

#[test]
fn transparent_carried_sheets_preserve_shortened_models() {
    let mut ordinary = render_model::equipment::blocks::BlockSheets {
        sheets: Vec::new(),
        by_visual: std::collections::BTreeMap::from([(VISUAL.0, 0)]),
        models: std::collections::BTreeMap::from([(VISUAL.0, carried_geometry(13.0 / 16.0))]),
    };
    for pixels in [
        [30, 60, 90, 0, 30, 60, 90, 255],
        [30, 60, 90, 127, 30, 60, 90, 255],
    ] {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
        let sprite = assets::IconSprite {
            width,
            height,
            rgba8: pixels
                .repeat(usize::from(width) * usize::from(height) / 2)
                .into(),
        };
        assert!(
            gui_cube_sheet(
                &alpha::world([assets::MATERIAL_FLAG_ALPHA_CUTOUT; 6]),
                VISUAL,
                &sprite,
            )
            .is_some(),
            "these source alpha modes can draw normalized cube faces"
        );
        assert!(
            !cube_shape_admitted(&ordinary, VISUAL.0),
            "short models must reach the shaped fallback"
        );
        let short = ordinary.models.remove(&VISUAL.0).unwrap();
        ordinary.models.insert(VISUAL.0, carried_geometry(1.0));
        assert!(
            cube_shape_admitted(&ordinary, VISUAL.0),
            "full-height cubes retain transparent faces"
        );
        ordinary.models.insert(VISUAL.0, short);
    }
}
