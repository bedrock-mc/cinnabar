use super::*;
use assets::{RuntimeEntityAssets, entity_seats::EntitySeatAdmission};
use pack_compiler::{compile_actor_assets_for_assets, compile_entity_assets_with_behavior_report};

#[test]
fn actor_artwork_decodes_against_behavior_seats_and_resource_only_entities() {
    let resource = pack(0, "entity_alphatest", false);
    let behavior = tempfile::tempdir().unwrap();
    write(
        behavior.path(),
        "entities/example.json",
        br#"{"format_version":"1.21.0","minecraft:entity":{"description":{"identifier":"minecraft:example"},"components":{"minecraft:rideable":{"seats":{"position":[0,1,0],"rotate_rider_by":"query.is_on_ground ? -90 : 90"}}}}}"#,
    );
    for with_behavior in [true, false] {
        let entities = if with_behavior {
            compile_entity_assets_with_behavior_report(resource.path(), behavior.path(), MANIFEST)
                .unwrap()
                .assets
        } else {
            compile_entity_assets(resource.path(), MANIFEST).unwrap()
        };
        let blob = encode_entity_blob(&entities).unwrap();
        let runtime_entities = RuntimeEntityAssets::decode(&blob).unwrap();
        let actor = compile_actor_assets_for_assets(resource.path(), &entities).unwrap();
        let artwork = RuntimeActorCatalog::decode(&actor.bytes, &runtime_entities).unwrap();
        assert_eq!(artwork.bindings().len(), 1);
        assert_eq!(artwork.textures().len(), 1);
        let texture = &artwork.textures()[0];
        assert_eq!(
            entities.sources[texture.source as usize].path.as_ref(),
            "textures/entity/example.png"
        );
        assert_eq!(texture.rgba8[PROBE_ALPHA], 0);
        assert_eq!(&texture.rgba8[..4], &[17, 31, 47, 255]);
        assert_eq!(
            actor.report.entity_carrier_sha256,
            Sha256::digest(&blob).as_slice()
        );
        assert_eq!(
            matches!(
                entities.seat_layouts[0].admission,
                EntitySeatAdmission::Provisional { .. }
            ),
            with_behavior
        );
        if with_behavior {
            let resource_actor = compile_actor_assets(resource.path(), MANIFEST).unwrap();
            assert!(RuntimeActorCatalog::decode(&resource_actor.bytes, &runtime_entities).is_err());
        } else {
            assert_eq!(
                actor.bytes,
                compile_actor_assets(resource.path(), MANIFEST)
                    .unwrap()
                    .bytes
            );
        }
    }
}
