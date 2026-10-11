//! Base armor art keeps the item's own material in every player preview.

use super::*;
use assets::{
    ArmorSlot, DEFAULT_LEATHER_RGB, EntityDependencyResolution, EquipmentBinding,
    EquipmentCategory, EquipmentReference, EquipmentTexture, EquipmentTransform,
    RuntimeEquipmentCatalog,
};
use player_preview::{MenuPreviewConfig, PreviewView};

const DIAMOND: [u8; 4] = [40, 220, 210, 255];
const LEATHER: [u8; 4] = [160, 160, 160, 255];
/// A team colour a server writes into armor's `customColor`.
const TEAM_RED: u32 = 0x00b0_2e26;

/// Creates chest armor with the requested texture and material for dye-rule comparisons.
fn chestplate(identifier: &str, texture: &str, material: &str) -> EquipmentBinding {
    let reference = |identifier: &str| EquipmentReference {
        identifier: identifier.into(),
        resolution: EntityDependencyResolution::Catalog,
    };
    EquipmentBinding {
        identifier: identifier.into(),
        category: EquipmentCategory::Armor {
            slot: ArmorSlot::Chestplate,
        },
        geometry: reference("geometry.player.armor.chestplate"),
        texture: reference(texture),
        material: material.into(),
        render_controller: "controller.render.armor".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    }
}

/// Vanilla's diamond and leather chestplates as the base catalog binds them.
fn vanilla() -> Arc<RuntimeEquipmentCatalog> {
    let texture = |identifier: &str, color: [u8; 4]| EquipmentTexture {
        identifier: identifier.into(),
        width: 64,
        height: 32,
        rgba8: color.repeat(64 * 32).into(),
    };
    Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [6; 32],
            vec![
                chestplate(
                    "minecraft:diamond_chestplate",
                    "textures/models/armor/diamond_1",
                    "armor",
                ),
                chestplate(
                    "minecraft:leather_chestplate",
                    "textures/models/armor/leather_1",
                    "armor_leather",
                ),
            ],
            vec![
                texture("textures/models/armor/diamond_1", DIAMOND),
                texture("textures/models/armor/leather_1", LEATHER),
            ],
        )
        .unwrap(),
    )
}

/// A preview whose GUI model pages hold the base equipment art, as `set_gui_models` places it.
fn presentation(base: &Arc<RuntimeEquipmentCatalog>) -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
    let mut atlas = atlas::Atlas::new(first, MODEL_PAGES);
    for texture in base.textures() {
        atlas
            .insert([texture.width, texture.height], &texture.rgba8)
            .unwrap();
    }
    let (pages, textures) = atlas.finish().unwrap();
    presentation.gui_models.required_pages = pages.len();
    presentation.gui_models.pages = pages;
    presentation.gui_models.textures = textures;
    presentation.gui_models.enabled = true;
    presentation.set_equipment_catalog(Some(Arc::clone(base)));
    presentation.set_player_preview_skin(None, Default::default());
    presentation
}

/// The preview views that dress the local player: inventory, pause screen and HUD.
fn views() -> [PreviewView; 3] {
    let pause = MenuPreviewConfig::MENU;
    [
        PreviewView::Live { offset: [0.0; 2] },
        PreviewView::DollLook {
            yaw: pause.starting_rotation,
            tilt: pause.camera_tilt_degrees,
            offset: [0.0; 2],
        },
        PreviewView::Hud,
    ]
}

/// The armor batch's vertex colours and whether any vertex enables the dye mask.
fn armor_shading(presentation: &UiPresentationRuntime) -> (Vec<[u8; 4]>, bool) {
    let mesh = presentation.gui_player_mesh().unwrap();
    // The skin draws first; the chestplate is the only armor batch.
    let [_, armor] = mesh.batches() else {
        panic!("one skin and one armor batch");
    };
    let vertices =
        &mesh.vertices()[armor.index_range.start as usize..armor.index_range.end as usize];
    let mut colors = vertices
        .iter()
        .map(|vertex| vertex.color)
        .collect::<Vec<_>>();
    colors.dedup();
    let masked = vertices
        .iter()
        .any(|vertex| vertex.style_flags & render_model::UI_STYLE_COLOR_MASK as u8 != 0);
    (colors, masked)
}

#[test]
fn team_dyed_diamond_armor_keeps_its_untinted_diamond_art_in_every_preview() {
    let base = vanilla();
    let mut presentation = presentation(&base);
    for pack in [
        None,
        Some(Arc::new(
            RuntimeEquipmentCatalog::from_parts([7; 32], Vec::new(), Vec::new()).unwrap(),
        )),
    ] {
        presentation.set_preview_pack_equipment(pack);
        presentation.set_player_preview_gear(
            [
                None,
                Some(("minecraft:diamond_chestplate", Some(TEAM_RED))),
                None,
                None,
            ],
            None,
        );
        let armor = presentation.player_preview_gear.armor[1].as_ref().unwrap();
        assert_eq!(&armor.rgba[..4], &DIAMOND);
        assert_eq!(
            armor.tint, None,
            "only a leather material applies a stack's dye"
        );
        for view in views() {
            presentation.player_preview_view = view;
            assert_eq!(
                armor_shading(&presentation),
                (vec![[255; 4]], false),
                "{view:?}"
            );
        }
    }
}

#[test]
fn previews_resolve_the_world_players_armor_material_and_dye() {
    let base = vanilla();
    let mut presentation = presentation(&base);
    let rgb = |rgb: u32| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    for (item, dye) in [
        ("minecraft:diamond_chestplate", Some(TEAM_RED)),
        ("minecraft:diamond_chestplate", None),
        ("minecraft:leather_chestplate", Some(TEAM_RED)),
        ("minecraft:leather_chestplate", None),
    ] {
        presentation.set_player_preview_gear([None, Some((item, dye)), None, None], None);
        let binding = base.binding(item).unwrap();
        let world = binding.color_mask_rgb(dye).map(rgb);
        let armor = presentation.player_preview_gear.armor[1].as_ref().unwrap();
        assert_eq!(armor.tint, world, "{item} {dye:?}");
        let texture = base.texture(&binding.texture.identifier).unwrap();
        assert!(Arc::ptr_eq(&armor.rgba, &texture.rgba8));
        for view in views() {
            presentation.player_preview_view = view;
            let (colors, masked) = armor_shading(&presentation);
            assert_eq!(
                colors,
                vec![world.map_or([255; 4], |[r, g, b]| [r, g, b, 255])]
            );
            assert_eq!(masked, world.is_some(), "{item} {dye:?} {view:?}");
        }
    }
    assert_eq!(
        base.binding("minecraft:leather_chestplate")
            .unwrap()
            .color_mask_rgb(None),
        Some(DEFAULT_LEATHER_RGB)
    );
}
