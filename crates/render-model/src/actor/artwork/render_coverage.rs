use assets::{
    EntityAssetKind, EntityRigFallback, RuntimeActorCatalog, RuntimeEntityAssets,
    facts::entity::{EntityFacts, NativeEntityClass},
};
use std::collections::BTreeSet;

use super::ActorArtworkPages;
use crate::EntityRigId;

#[path = "render_coverage/fixture.rs"]
mod fixture;

// These missing body routes stay explicit until their models can be rendered.
const INCOMPLETE: &[&str] = &[
    "minecraft:agent",
    "minecraft:balloon",
    "minecraft:chalkboard",
    "minecraft:elder_guardian_ghost",
    "minecraft:ice_bomb",
    "minecraft:moving_block",
    "minecraft:ominous_item_spawner",
    "minecraft:thrown_trident",
    "minecraft:shield",
];

/// Reads local carrier fixtures without replacing unavailable assets with diagnostic artwork.
fn fixture(variable: &str) -> Option<Vec<u8>> {
    let Some(path) = std::env::var_os(variable) else {
        eprintln!("missing fixture {variable}: skipping entity render coverage");
        return None;
    };
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("missing fixture {variable}={path:?}: skipping entity render coverage");
            None
        }
        Err(error) => panic!("read {variable}={path:?}: {error}"),
    }
}

/// Checks drawable geometry and runtime artwork lookups, including conditional overlay layers.
fn has_body_route(
    entities: &RuntimeEntityAssets,
    artwork: &ActorArtworkPages,
    identifier: &str,
    parents: &[Option<usize>],
) -> bool {
    let Some((binding, rig)) = entities.rig_bindings().iter().enumerate().find(|(_, rig)| {
        let symbol = &entities.symbols()[rig.entity_symbol as usize];
        symbol.kind == EntityAssetKind::Entity && symbol.identifier.as_ref() == identifier
    }) else {
        return false;
    };
    if rig.geometry_count == 0
        || rig.render_controller.is_none()
        || rig.fallback == EntityRigFallback::Diagnostic
    {
        return false;
    }
    let drawable = |geometry: usize| {
        std::iter::successors(Some(geometry), |&index| parents[index]).any(|index| {
            entities.geometries()[index]
                .bones
                .iter()
                .any(|bone| !bone.cubes.is_empty() || !bone.texture_meshes.is_empty())
        })
    };
    let rig_candidates = rig.first_geometry..rig.first_geometry + u32::from(rig.geometry_count);
    if !rig_candidates.clone().all(|candidate| {
        drawable(entities.rig_geometries()[candidate as usize].geometry as usize)
            && artwork.route(EntityRigId(candidate)).is_some()
    }) {
        return false;
    }
    let layers = entities.render_layers(binding);
    !layers.is_empty()
        && layers.iter().all(|layer| {
            !layer.unresolved_textures
                && layer.slot_count > 0
                && entities.render_data().geometries[layer.first_geometry as usize..]
                    [..usize::from(layer.geometry_count)]
                    .iter()
                    .all(|candidate| drawable(candidate.geometry as usize))
                && entities.render_data().slots[layer.first_slot as usize..]
                    [..usize::from(layer.slot_count)]
                    .iter()
                    .all(|slot| {
                        slot.candidate_count > 0
                            && entities.render_data().candidates[slot.first_candidate as usize..]
                                [..usize::from(slot.candidate_count)]
                                .iter()
                                .all(|candidate| {
                                    rig_candidates.clone().all(|geometry| {
                                        artwork
                                            .variant_location(
                                                EntityRigId(geometry),
                                                candidate.source,
                                            )
                                            .is_some()
                                    })
                                })
                    })
        })
}

#[test]
fn incomplete_entity_routes_cannot_outlive_the_native_registry() {
    let registry: BTreeSet<_> = EntityFacts::pinned()
        .records
        .iter()
        .map(|record| record.identifier.as_ref())
        .collect();
    assert_eq!(
        INCOMPLETE.iter().copied().collect::<BTreeSet<_>>().len(),
        INCOMPLETE.len()
    );
    for identifier in INCOMPLETE {
        assert!(
            registry.contains(identifier),
            "stale entity exception {identifier}"
        );
    }
}

#[test]
fn removing_one_artwork_binding_fails_body_coverage_with_textures_intact() {
    let (entity_bytes, textures, bindings) = fixture::carrier();
    let entities = RuntimeEntityAssets::decode(&entity_bytes).unwrap();
    let parents = assets::validate_entity_geometry_inheritance(entities.geometries()).unwrap();
    let actor_bytes = assets::encode_actor_catalog(&entity_bytes, &textures, &bindings).unwrap();
    let complete = RuntimeActorCatalog::decode(&actor_bytes, &entities).unwrap();
    assert!(has_body_route(
        &entities,
        &ActorArtworkPages::new(&complete),
        "test:coverage",
        &parents
    ));
    for removed in 0..bindings.len() {
        let mut incomplete = bindings.clone();
        incomplete.remove(removed);
        let actor_bytes =
            assets::encode_actor_catalog(&entity_bytes, &textures, &incomplete).unwrap();
        let artwork = RuntimeActorCatalog::decode(&actor_bytes, &entities).unwrap();
        assert_eq!(artwork.textures(), complete.textures());
        assert!(
            !has_body_route(
                &entities,
                &ActorArtworkPages::new(&artwork),
                "test:coverage",
                &parents
            ),
            "missing binding for geometry candidate {removed} must fail coverage"
        );
    }
}

#[test]
fn native_entities_resolve_complete_render_resources() {
    let Some(entity_bytes) = fixture("CINNABAR_ENTITY_CARRIER") else {
        return;
    };
    let Some(actor_bytes) = fixture("CINNABAR_ACTOR_CARRIER") else {
        return;
    };
    let entities = RuntimeEntityAssets::decode(&entity_bytes).expect("valid entity carrier");
    let artwork =
        RuntimeActorCatalog::decode(&actor_bytes, &entities).expect("matching actor carrier");
    let artwork = ActorArtworkPages::new(&artwork);
    let parents = assets::validate_entity_geometry_inheritance(entities.geometries())
        .expect("valid geometry inheritance");
    let mut failures = Vec::new();
    for record in &EntityFacts::pinned().records {
        let identifier = record.identifier.as_ref();
        // These classes publish block, item, painting, weather or particle geometry elsewhere.
        let special = matches!(
            EntityFacts::pinned().get(identifier).native_class,
            NativeEntityClass::Tnt
                | NativeEntityClass::FallingBlock
                | NativeEntityClass::Painting
                | NativeEntityClass::Item
                | NativeEntityClass::LightningBolt
                | NativeEntityClass::AreaEffectCloud
        );
        let resolved = special || has_body_route(&entities, &artwork, identifier, &parents);
        if INCOMPLETE.contains(&identifier) {
            assert!(
                !resolved,
                "remove the resolved entity exception {identifier}"
            );
        } else if !resolved {
            failures.push(identifier);
        }
    }
    assert!(
        failures.is_empty(),
        "unresolved entity render resources: {failures:?}"
    );
}
