use super::*;

/// Builds a minimal original binding for catalog validation.
fn binding(identifier: &str) -> EquipmentBinding {
    let reference = |identifier: &str| EquipmentReference {
        identifier: identifier.into(),
        resolution: EntityDependencyResolution::Catalog,
    };
    EquipmentBinding {
        identifier: identifier.into(),
        category: EquipmentCategory::Held,
        geometry: reference("geometry.a"),
        texture: reference("textures/entity/a"),
        resources: Default::default(),
        material: "entity".into(),
        color_mask: EquipmentColorMask::NoMask,
        render_controller: "controller.render.a".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    }
}

// Bindings sort and dedupe by identifier; a zero identity is refused.
#[test]
fn from_parts_sorts_dedupes_and_requires_an_identity() {
    let catalog = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        vec![binding("b:item"), binding("a:item"), binding("b:item")],
        Vec::new(),
    )
    .unwrap();
    let identifiers = catalog
        .bindings()
        .iter()
        .map(|binding| binding.identifier.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(identifiers, ["a:item", "b:item"]);
    assert!(catalog.binding("b:item").is_some());
    assert!(
        RuntimeEquipmentCatalog::from_parts([0; 32], vec![binding("a:item")], Vec::new()).is_err()
    );
}

#[test]
fn compiled_mask_capability_applies_dye_without_inspecting_the_material_name() {
    let red = Some(0x00b0_2e26);
    let worn = |material: &str, color_mask| EquipmentBinding {
        material: material.into(),
        color_mask,
        ..binding("custom:chestplate")
    };
    assert_eq!(
        worn("renamed", EquipmentColorMask::Dye).color_mask_rgb(red),
        red
    );
    assert_eq!(
        worn("renamed", EquipmentColorMask::Dye).color_mask_rgb(None),
        Some(DEFAULT_LEATHER_RGB)
    );
    assert_eq!(
        worn("leather_decoy", EquipmentColorMask::NoMask).color_mask_rgb(red),
        None
    );
    assert_eq!(
        worn("unknown", EquipmentColorMask::Unresolved).color_mask_rgb(red),
        None
    );
}
