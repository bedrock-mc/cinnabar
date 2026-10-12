use assets::{
    RuntimeEntityAssets,
    entity_seats::{EntitySeatAdmission, SeatRotation, SeatSelector},
};
use pack_compiler::{compile_entity_assets, compile_entity_assets_with_behavior_report};
use serde_json::json;

/// Writes an original JSON fixture into its declared pack family.
fn write(root: &std::path::Path, name: &str, value: serde_json::Value) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn collected_behavior_seats_bind_the_resource_inventory_and_global_expression_table() {
    let resource = tempfile::tempdir().unwrap();
    let behavior = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(resource.path().join(family)).unwrap();
    }
    for identifier in ["fixture:mount", "fixture:no_behavior"] {
        write(
            resource.path(),
            &format!("entity/{}.json", identifier.replace(':', "_")),
            json!({
                "format_version":"1.8.0", "minecraft:client_entity":{"description":{"identifier":identifier}}
            }),
        );
    }
    write(
        behavior.path(),
        "entities/mount.json",
        json!({
            "format_version":"1.21.0", "minecraft:entity": {
                "description":{"identifier":"fixture:mount"},
                "components":{"minecraft:rideable":{"seats":{"position":[0,1,0],"rotate_rider_by":"query.is_on_ground ? -90 : 90"}}},
                "component_groups":{"fixture:saddled":{"minecraft:rideable":{"seats":{"position":[0,2,0],"rotate_rider_by":45}}}}
            }
        }),
    );
    let manifest = include_bytes!("../../../../assets/vanilla-source.json");
    let compiled =
        compile_entity_assets_with_behavior_report(resource.path(), behavior.path(), manifest)
            .unwrap()
            .assets;
    assert_eq!(compiled.seat_layouts.len(), 2);
    let EntitySeatAdmission::Provisional { source, layouts } = &compiled.seat_layouts[0].admission
    else {
        panic!("authored seat")
    };
    assert_eq!(
        compiled.sources[*source as usize].path.as_ref(),
        "behavior/entities/mount.json"
    );
    assert_eq!(layouts[0].selector, SeatSelector::Base);
    let SeatRotation::Expression { expression } = layouts[0].seats[0].rotation else {
        panic!("retained expression")
    };
    assert!(compiled.molang_expressions[expression as usize].op_count > 0);
    assert!(matches!(
        layouts[1].selector,
        SeatSelector::UnresolvedComponentGroup { .. }
    ));
    assert_eq!(
        compiled.seat_layouts[1].admission,
        EntitySeatAdmission::Unresolved
    );
    RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let missing = compile_entity_assets(resource.path(), manifest).unwrap();
    assert!(
        missing
            .seat_layouts
            .iter()
            .all(|entry| entry.admission == EntitySeatAdmission::Unresolved)
    );
}
