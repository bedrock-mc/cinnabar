//! A custom wearable can name a native external model without shipping its geometry.

use super::*;
use crate::presentation::equipment::tests::held_animation_tests::{owner, owner_rig};

/// Compiles original armor documents and pixels without installed game assets.
fn compile(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> (Arc<RuntimeEntityAssets>, Arc<RuntimeEquipmentCatalog>) {
    let pack = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(pack.entities).unwrap()),
        Arc::new(
            RuntimeEquipmentCatalog::from_parts(
                pack.identity,
                pack.equipment_bindings,
                pack.equipment_textures,
            )
            .unwrap(),
        ),
    )
}

/// An ordinary external definition or an owner-mapped variant of the same armor item.
fn definition(id: &str, item: Option<&str>, geometry: &str) -> Vec<u8> {
    let mut description = serde_json::json!({
        "identifier":id,"materials":{"default":"armor"},
        "textures":{"default":"textures/armor"},"geometry":{"default":geometry},
        "render_controllers":["controller.render.fixture"]
    });
    if let Some(item) = item {
        description["item"] =
            serde_json::json!({(item):"query.owner_identifier == 'minecraft:skeleton'"});
    }
    serde_json::to_vec(&serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":description}})).unwrap()
}

#[test]
fn custom_content_wearables_resolve_external_models_for_their_owner() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([210, 30, 50, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let shared = vec![
        (Box::<str>::from("textures/armor.png"), png.into_inner()),
        ("render_controllers/fixture.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
    ];
    let mut base = shared.clone();
    base.extend([
        ("attachables/ordinary.json".into(), definition("minecraft:iron_helmet",None,"geometry.fixture.external")),
        ("attachables/owner.json".into(), definition("fixture:owner",Some("minecraft:iron_helmet"),"geometry.fixture.owner")),
        ("models/entity/armor.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture.owner","texture_width":16,"texture_height":16},"bones":[{"name":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec()),
    ]);
    let (base, catalog) = compile(base);
    let (fixture, _, _) = block_fixture();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        base,
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let body = crate::presentation::equipment::tests::player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names);
    for geometry in ["geometry.fixture.external", "geometry.fixture.missing"] {
        let mut files = shared.clone();
        files.push((
            "attachables/custom.json".into(),
            definition("fixture:custom", None, geometry),
        ));
        let (pack, catalog) = compile(files);
        let (_, locations) = pages
            .clone()
            .with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((pack, Some(catalog), locations)));
        crate::presentation::equipment::tests::declare_wearable(
            &mut runtime,
            "fixture:custom",
            "slot.armor.head",
        );
        let input = ActorEquipmentInput {
            armor: [
                Some(WornItem {
                    identifier: "fixture:custom".into(),
                    kind: HeldKind::Other,
                    ..fixture_item()
                }),
                None,
                None,
                None,
            ],
            ..Default::default()
        };
        let layers = runtime.layers_for(
            &body,
            &input,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: 0.05,
            }),
        );
        assert_eq!(
            layers.len(),
            usize::from(geometry == "geometry.fixture.external")
        );
        if let Some(layer) = layers.first() {
            assert!(
                layer
                    .submission
                    .input
                    .current_bones
                    .iter()
                    .any(|bone| bone.translation_scale[3] != 0.0)
            );
        }
    }
}

/// Reuses a synthetic stack with no dependencies on vanilla carriers.
fn fixture_item() -> WornItem {
    block_fixture().2
}

/// Builds an external family, its conditional owner model, and an optional direct startup fallback.
fn conditional_fixture(
    condition: &str,
    startup_fallback: bool,
    alternate: bool,
    pack_fallback: bool,
) -> EquipmentRuntime {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([210, 30, 50, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let shared = vec![
        (Box::<str>::from("textures/armor.png"), png.into_inner()),
        ("render_controllers/fixture.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
    ];
    let mut mapped: serde_json::Value = serde_json::from_slice(&definition(
        "fixture:owner",
        Some("minecraft:iron_helmet"),
        "geometry.fixture.owner",
    ))
    .unwrap();
    mapped["minecraft:attachable"]["description"]["item"]["minecraft:iron_helmet"] =
        condition.into();
    if alternate {
        mapped["minecraft:attachable"]["description"]["geometry"]["alternate"] =
            "geometry.fixture.alternate".into();
        mapped["minecraft:attachable"]["description"]["render_controllers"] =
            serde_json::json!(["controller.render.alternate"]);
    }
    let mut base = shared.clone();
    if alternate {
        base.extend([
            ("render_controllers/alternate.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.alternate":{"geometry":"Geometry.alternate","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
            ("models/entity/alternate.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture.alternate","texture_width":16,"texture_height":16},"bones":[{"name":"alternate_shell","pivot":[0,24,0],"cubes":[{"origin":[-3,24,-3],"size":[6,6,6],"uv":[0,0]}]}]}]}"#.to_vec()),
        ]);
    }
    base.extend([
        ("attachables/ordinary.json".into(), definition("minecraft:iron_helmet", None, "geometry.fixture.external")),
        ("attachables/owner.json".into(), serde_json::to_vec(&mapped).unwrap()),
        ("models/entity/armor.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture.owner","texture_width":16,"texture_height":16},"bones":[{"name":"stand_shell","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec()),
    ]);
    if startup_fallback {
        base.push((
            "attachables/fallback.json".into(),
            definition("fixture:custom", None, "geometry.fixture.owner"),
        ));
    }
    let (base, catalog) = compile(base);
    let (fixture, _, _) = block_fixture();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        base,
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let mut pack = shared;
    pack.push((
        "attachables/custom.json".into(),
        definition("fixture:custom", None, "geometry.fixture.external"),
    ));
    if pack_fallback {
        pack.retain(|(path, _)| path.as_ref() != "attachables/custom.json");
        pack.extend([
            ("attachables/custom.json".into(), definition("fixture:custom", None, "geometry.fixture.pack")),
            ("attachables/custom_owner.json".into(), definition("fixture:mapped_custom", Some("fixture:custom"), "geometry.fixture.external")),
            ("models/entity/pack.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture.pack","texture_width":16,"texture_height":16},"bones":[{"name":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec()),
        ]);
    }
    let (assets, catalog) = compile(pack);
    let (_, locations) = pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((assets, Some(catalog), locations)));
    crate::presentation::equipment::tests::declare_wearable(
        &mut runtime,
        "fixture:custom",
        "slot.armor.head",
    );
    runtime
}

#[test]
fn custom_content_external_armor_uses_stack_enchantment_and_unmatched_authored_poses() {
    let mut runtime = conditional_fixture("v.is_enchanted", false, false, false);
    let body = crate::presentation::equipment::tests::player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names);
    for enchanted in [false, true, false] {
        let input = ActorEquipmentInput {
            armor: [
                Some(WornItem {
                    identifier: "fixture:custom".into(),
                    enchanted,
                    ..fixture_item()
                }),
                None,
                None,
                None,
            ],
            ..Default::default()
        };
        let layers = runtime.layers_for(
            &body,
            &input,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: 0.05,
            }),
        );
        assert_eq!(layers.len(), usize::from(enchanted));
        if let Some(layer) = layers.first() {
            assert_eq!(layer.submission.input.current_bones.len(), 1);
            assert_ne!(
                layer.submission.input.current_bones[0].translation_scale[3], 0.0,
                "the owner lacks stand_shell; its authored pose must remain visible"
            );
        }
    }
}

#[test]
fn custom_content_external_armor_keeps_startup_draw_when_family_rejects_owner() {
    let mut runtime = conditional_fixture(
        "query.owner_identifier == 'minecraft:player'",
        true,
        false,
        false,
    );
    let body = crate::presentation::equipment::tests::player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names);
    let input = ActorEquipmentInput {
        armor: [
            Some(WornItem {
                identifier: "fixture:custom".into(),
                ..fixture_item()
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    };
    let animation = Some(EquipmentAnimation {
        owner: &owner,
        rig: &rig,
        frame_alpha: 1.0,
        delta_seconds: 0.05,
    });
    let with_pack: Vec<_> = runtime
        .layers_for(&body, &input, animation)
        .iter()
        .map(|layer| (layer.submission.input.rig, layer.submission.texture_layer))
        .collect();
    runtime.set_pack_layer(None);
    let startup: Vec<_> = runtime
        .layers_for(&body, &input, animation)
        .iter()
        .map(|layer| (layer.submission.input.rig, layer.submission.texture_layer))
        .collect();
    assert_eq!(startup.len(), 1);
    assert_eq!(with_pack, startup);
}

#[test]
fn custom_content_external_armor_keeps_ordinary_pack_model_when_family_rejects_owner() {
    let mut runtime = conditional_fixture(
        "query.owner_identifier == 'minecraft:player'",
        false,
        false,
        true,
    );
    let body = crate::presentation::equipment::tests::player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names);
    let input = ActorEquipmentInput {
        armor: [
            Some(WornItem {
                identifier: "fixture:custom".into(),
                ..fixture_item()
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    };
    let animation = EquipmentAnimation {
        owner: &owner,
        rig: &rig,
        frame_alpha: 1.0,
        delta_seconds: 0.05,
    };
    let source = runtime
        .worn_binding_source(
            input.armor[0].as_ref().unwrap(),
            &input,
            Some(animation),
            ArmorSlot::Helmet,
        )
        .unwrap();
    assert!(source.from_pack);
    assert!(source.catalog.bindings()[source.index].attachable.is_none());
    assert_eq!(runtime.layers_for(&body, &input, Some(animation)).len(), 1);
}

#[test]
fn custom_content_external_armor_uses_controller_geometry_and_registers_its_mesh() {
    let mut runtime = conditional_fixture("1.0", false, true, false);
    let body = crate::presentation::equipment::tests::player_body(&mut runtime);
    runtime.take_pending_geometries();
    let expected = runtime
        .armor_geometry_for("geometry.fixture.alternate", false)
        .unwrap()
        .rig;
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names);
    let input = ActorEquipmentInput {
        armor: [
            Some(WornItem {
                identifier: "fixture:custom".into(),
                ..fixture_item()
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    };
    let layers = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 1.0,
            delta_seconds: 0.05,
        }),
    );
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].submission.input.rig, expected);
    assert_ne!(
        layers[0].submission.input.current_bones[0].translation_scale[3],
        0.0
    );
    assert!(
        runtime
            .take_pending_geometries()
            .iter()
            .any(|mesh| mesh.id == expected)
    );
}
