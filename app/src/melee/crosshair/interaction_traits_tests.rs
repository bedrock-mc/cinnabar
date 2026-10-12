//! Original diagnostic projection of the admitted actor-pick result.

use std::sync::Arc;

use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::{UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor};

/// Loads a checked original entity inventory before admitting a custom projectile base.
fn checked_world() -> client_world::WorldAuthority {
    let compiled = assets::carriers::startup::test_carriers::synthetic_interaction_assets(
        "fixture:projectile",
    );
    let blob = assets::encode_entity_blob(&compiled).unwrap();
    let entities = Arc::new(assets::RuntimeEntityAssets::decode(&blob).unwrap());
    let mut world = client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 99,
            local_player_unique_id: 99,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Some(entities),
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Identifiers(protocol::ActorIdentifierRegistry {
                entries: Arc::from([protocol::ActorIdentifier {
                    identifier: "fixture:projectile".into(),
                    base_identifier: "minecraft:arrow".into(),
                }]),
                skipped: 0,
            })),
            Some(1),
        )
        .unwrap();
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 800,
                runtime_id: 80,
                kind: ActorKind::Entity {
                    identifier: "fixture:projectile".into(),
                },
                position: [0.0, 0.0, -2.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([
                    protocol::ActorMetadata {
                        key: client_world::BOUNDING_BOX_WIDTH_METADATA_KEY,
                        value: protocol::ActorMetadataValue::Float(0.6),
                    },
                    protocol::ActorMetadata {
                        key: client_world::BOUNDING_BOX_HEIGHT_METADATA_KEY,
                        value: protocol::ActorMetadataValue::Float(2.0),
                    },
                ]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(2),
        )
        .unwrap();
    world
}

/// Adds one original diagnostic rectangle to the offline render input.
fn rectangle(
    vertices: &mut Vec<UiRenderVertex>,
    indices: &mut Vec<u32>,
    bounds: [f32; 4],
    color: [u8; 4],
) {
    let first = vertices.len() as u32;
    for position in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
        [bounds[0], bounds[3]],
    ] {
        vertices.push(UiRenderVertex {
            position,
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color,
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        });
    }
    indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// Shows the real pick result as a red diagnostic selection frame around an original actor marker.
fn snapshot(selected: bool) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    rectangle(
        &mut vertices,
        &mut indices,
        [0.0, 0.0, 128.0, 128.0],
        [22, 27, 34, 255],
    );
    rectangle(
        &mut vertices,
        &mut indices,
        [48.0, 24.0, 80.0, 104.0],
        [230, 160, 45, 255],
    );
    if selected {
        for bounds in [
            [44.0, 20.0, 84.0, 23.0],
            [44.0, 105.0, 84.0, 108.0],
            [44.0, 23.0, 47.0, 105.0],
            [81.0, 23.0, 84.0, 105.0],
        ] {
            rectangle(&mut vertices, &mut indices, bounds, [220, 60, 70, 255]);
        }
    }
    rectangle(
        &mut vertices,
        &mut indices,
        [57.0, 63.0, 71.0, 65.0],
        [255; 4],
    );
    rectangle(
        &mut vertices,
        &mut indices,
        [63.0, 57.0, 65.0, 71.0],
        [255; 4],
    );
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [128; 2],
        safe_area: [0; 4],
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 128, 128),
            0,
            indices.len() as u32,
            0,
        )]),
        vertices: vertices.into(),
        indices: indices.into(),
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![render_model::UiTexturePage::owned([1; 2], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    input.validate().unwrap();
    let image = client_ui::ui_runtime::presentation::forms::snapshot::rasterize(&input);
    assert_eq!(
        image.pixels().any(|pixel| pixel.0 == [220, 60, 70, 255]),
        selected
    );
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "admitted-projectile-pick");
}

#[test]
fn interaction_traits_do_not_pick_an_advertised_projectile() {
    let world = checked_world();
    let hit = gameplay::melee::pick_actor(
        world.remote_actors(),
        None,
        [0.0, 1.4, 0.0],
        [0.0, 0.0, -1.0],
        3.0,
    );
    snapshot(hit.is_some());
    assert_eq!(hit, None);
    assert!(!gameplay::melee::obstructs_placement(
        world.actor(80).unwrap()
    ));
}
