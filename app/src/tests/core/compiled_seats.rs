//! Original diagnostic projection of the resolved local rider seat.

use assets::entity_seats::{
    CompiledRideSeat, CompiledSeatLayout, EntitySeatAdmission, SeatRotation, SeatSelector,
};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::{UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor};
use std::sync::Arc;

/// Admits an original group-only layout whose activation is unavailable from the network.
fn checked_world() -> client_world::WorldAuthority {
    let mut compiled = assets::carriers::startup::test_carriers::synthetic_entity_assets(1);
    compiled.sources = Box::from([
        assets::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [2; 32],
        },
        compiled.sources[0].clone(),
    ]);
    compiled.symbols[0].source_index = 1;
    compiled.seat_layouts[0].admission = EntitySeatAdmission::Provisional {
        source: 0,
        layouts: Box::from([CompiledSeatLayout {
            selector: SeatSelector::UnresolvedComponentGroup { group: 0 },
            seats: Box::from([CompiledRideSeat {
                position: [0.0, 1.0, 0.0]
                    .map(|value| assets::ItemDisplayScalar::new(value).unwrap()),
                min_riders: 0,
                max_riders: u32::MAX,
                rotation: SeatRotation::Absent,
                lock_degrees: None,
            }]),
        }]),
    };
    let entities = Arc::new(
        assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
            .unwrap(),
    );
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
    for (sequence, runtime_id, unique_id, identifier) in [
        (1, 80, 800, "minecraft:allay"),
        (2, 99, 99, "fixture:rider"),
    ] {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id,
                    runtime_id,
                    kind: ActorKind::Entity {
                        identifier: identifier.into(),
                    },
                    position: [0.0; 3],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
                Some(sequence),
            )
            .unwrap();
    }
    world
        .apply_ordered_event(
            WorldEvent::ActorLink(protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 800,
                rider_unique_id: 99,
                link_type: protocol::ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
            Some(3),
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

/// Projects the actual resolved rider height through the offline UI rasterizer.
fn snapshot(height: f32) {
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
        [28.0, 96.0, 100.0, 110.0],
        [230, 160, 45, 255],
    );
    let feet = 96.0 - height * 24.0;
    rectangle(
        &mut vertices,
        &mut indices,
        [56.0, feet - 28.0, 72.0, feet],
        [80, 190, 230, 255],
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
    assert!(image.pixels().any(|pixel| pixel.0 == [80, 190, 230, 255]));
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "unadmitted-seat-group");
}

#[test]
fn unadmitted_seat_group_preserves_the_authoritative_rider_height() {
    let world = checked_world();
    let seat = world.local_rider_seat_pose();
    let height = seat.map_or_else(|| world.actor(99).unwrap().position[1], |pose| pose.0[1]);
    snapshot(height);
    assert_eq!(seat, None);
    assert_eq!(height, 0.0);
}
