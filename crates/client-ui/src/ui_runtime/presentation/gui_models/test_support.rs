//! Assertions shared with app-owned installed-carrier integration tests.
use super::*;
use crate::test_support::{fixture_font, fixture_hud};

/// Checks installed geometry admission and diagnostic fallback without owning asset paths.
pub fn assert_installed_geometry(
    world: &RuntimeAssets,
    entities: &RuntimeEntityAssets,
    icons: Arc<assets::RuntimeIconCatalog>,
    equipment: Option<Arc<assets::RuntimeEquipmentCatalog>>,
) {
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    presentation.set_equipment_catalog(equipment);
    presentation.set_gui_models(world, entities).unwrap();
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
        [render_model::UI_MODEL_ATLAS_SIDE; 2]
    );
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), entities)
        .unwrap();
    let grass = presentation.item_icon("minecraft:grass_block", 0).unwrap();
    assert!(
        presentation
            .gui_models
            .models
            .contains_key(&icon_key(grass))
    );
}
