//! Server stacks whose attachable art outgrows the bounded GUI model atlas.

use super::*;
use assets::{
    ArmorSlot, EntityDependencyResolution, EquipmentBinding, EquipmentCategory, EquipmentReference,
    EquipmentTexture, EquipmentTransform, RuntimeEquipmentCatalog,
};

/// Distinct armor texture sizes and counts of a reported nine-pack server stack: 52 textures,
/// about 4.7 million texels, against 1.8 million in the seven model pages.
const STACK_ARMOR: [([u16; 2], usize); 9] = [
    ([1024, 1024], 2),
    ([654, 576], 1),
    ([512, 512], 2),
    ([432, 410], 2),
    ([337, 336], 2),
    ([256, 256], 11),
    ([256, 128], 4),
    ([128, 64], 26),
    ([96, 96], 2),
];

fn binding(identifier: &str, slot: ArmorSlot, texture: &str, material: &str) -> EquipmentBinding {
    let reference = |identifier: &str| EquipmentReference {
        identifier: identifier.into(),
        resolution: EntityDependencyResolution::Catalog,
    };
    EquipmentBinding {
        identifier: identifier.into(),
        category: EquipmentCategory::Armor { slot },
        geometry: reference("geometry.fixture"),
        texture: reference(texture),
        material: material.into(),
        render_controller: "controller.render.armor".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    }
}

fn texture(identifier: &str, [width, height]: [u16; 2], color: [u8; 4]) -> EquipmentTexture {
    EquipmentTexture {
        identifier: identifier.into(),
        width,
        height,
        rgba8: color
            .repeat(usize::from(width) * usize::from(height))
            .into(),
    }
}

/// A distinct opaque colour per stack texture, so no two share atlas texels.
fn stack_color(index: usize) -> [u8; 4] {
    [index as u8, (index * 37) as u8, 200, 255]
}

/// The stack's armor catalog: item `fixture:armor_N` wears texture `N`, in `STACK_ARMOR` order.
fn stack_catalog() -> Arc<RuntimeEquipmentCatalog> {
    let sizes = STACK_ARMOR
        .iter()
        .flat_map(|(size, count)| std::iter::repeat_n(*size, *count))
        .collect::<Vec<_>>();
    let (bindings, textures) = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            let path = format!("textures/fixture/armor_{index}");
            (
                binding(
                    &format!("fixture:armor_{index}"),
                    ArmorSlot::Chestplate,
                    &path,
                    "entity_alphatest",
                ),
                texture(&path, *size, stack_color(index)),
            )
        })
        .unzip();
    Arc::new(RuntimeEquipmentCatalog::from_parts([3; 32], bindings, textures).unwrap())
}

/// The first stack texture of `size`.
fn stack_index(size: [u16; 2]) -> usize {
    STACK_ARMOR
        .iter()
        .take_while(|(stack, _)| *stack != size)
        .map(|(_, count)| count)
        .sum()
}

fn presentation() -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(None, Default::default());
    presentation
}

/// Texels each drawn armor slot samples from the GUI pages, at its batch's first UV corner.
fn sampled_armor(presentation: &UiPresentationRuntime) -> Vec<[u8; 4]> {
    let mesh = presentation.gui_player_mesh().unwrap();
    // The skin is the first batch; armor follows in slot order without a cape or held item.
    mesh.batches()[1..]
        .iter()
        .map(|batch| {
            let vertices =
                &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
            let corner = [0, 1].map(|axis| {
                vertices
                    .iter()
                    .map(|vertex| vertex.uv[axis])
                    .fold(f32::INFINITY, f32::min) as usize
            });
            let page = &presentation.textures.pages()[usize::from(batch.texture_page)];
            let [width, _] = page.dimensions();
            let at = (corner[1] * width as usize + corner[0]) * 4;
            page.pixels()[at..at + 4].try_into().unwrap()
        })
        .collect()
}

#[test]
fn stack_armor_beyond_the_model_atlas_still_dresses_every_slot_with_pack_art() {
    let mut presentation = presentation();
    presentation.set_preview_pack_equipment(Some(stack_catalog()));
    assert!(
        presentation.gui_models.pack_equipment.source.is_some(),
        "art the preview does not wear cannot disable the pack"
    );
    let worn = [[1024, 1024], [654, 576], [512, 512], [128, 64]].map(stack_index);
    let identifiers = worn.map(|index| format!("fixture:armor_{index}"));
    presentation.set_player_preview_gear(
        std::array::from_fn(|slot| Some((identifiers[slot].as_str(), None))),
        None,
    );
    for (slot, index) in worn.into_iter().enumerate() {
        let armor = presentation.player_preview_gear.armor[slot]
            .as_ref()
            .unwrap();
        assert_eq!(&armor.rgba[..4], &stack_color(index), "slot {slot}");
    }
    assert_eq!(sampled_armor(&presentation), worn.map(stack_color));
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
    // Art up to a full model page keeps every texel; longer art halves until a page holds it.
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    for (slot, size) in [[side; 2], [327, 288], [side; 2], [128, 64]]
        .into_iter()
        .enumerate()
    {
        let region = presentation.gui_models.pack_equipment.region(slot).unwrap();
        assert_eq!(
            [region.uv[2] - region.uv[0], region.uv[3] - region.uv[1]],
            size,
            "slot {slot}"
        );
    }
}

#[test]
fn one_pack_texture_without_room_leaves_the_rest_of_the_pack_usable() {
    let mut presentation = presentation();
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let full =
        UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap();
    presentation.gui_models.pages = vec![full; MODEL_PAGES - 1];
    presentation.gui_models.required_pages = MODEL_PAGES - 1;
    let diamond = [40, 220, 210, 255];
    let base = RuntimeEquipmentCatalog::from_parts(
        [4; 32],
        vec![binding(
            "minecraft:diamond_boots",
            ArmorSlot::Boots,
            "textures/models/armor/diamond_1",
            "armor",
        )],
        vec![texture(
            "textures/models/armor/diamond_1",
            [64, 32],
            diamond,
        )],
    )
    .unwrap();
    presentation.set_equipment_catalog(Some(Arc::new(base)));
    let (chest, boots) = ([90, 10, 10, 255], [10, 90, 10, 255]);
    let pack = RuntimeEquipmentCatalog::from_parts(
        [5; 32],
        vec![
            binding(
                "fixture:chestplate",
                ArmorSlot::Chestplate,
                "pack/chest",
                "armor",
            ),
            binding(
                "minecraft:diamond_boots",
                ArmorSlot::Boots,
                "pack/boots",
                "armor",
            ),
        ],
        vec![
            texture("pack/chest", [512, 512], chest),
            texture("pack/boots", [128, 64], boots),
        ],
    )
    .unwrap();
    presentation.set_preview_pack_equipment(Some(Arc::new(pack)));
    presentation.set_player_preview_gear(
        [
            None,
            Some(("fixture:chestplate", None)),
            None,
            Some(("minecraft:diamond_boots", None)),
        ],
        None,
    );
    let gear = &presentation.player_preview_gear.armor;
    assert_eq!(&gear[1].as_ref().unwrap().rgba[..4], &chest);
    // The one free page holds the page-sized chestplate; the boots keep their vanilla diamond.
    assert_eq!(&gear[3].as_ref().unwrap().rgba[..4], &diamond);
    assert_eq!(gear[3].as_ref().unwrap().tint, None);
    assert!(presentation.gui_models.pack_equipment.source.is_some());
}
