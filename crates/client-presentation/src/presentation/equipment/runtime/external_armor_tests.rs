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
