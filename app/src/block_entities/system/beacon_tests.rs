use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::{Quat, World};
use protocol::WorldEvent;
use sha2::Digest;
use world::BlockEntityKey;

/// Beacons sit above all other loaded blocks in the synthetic sub-chunk.
const BEACON_Y: i32 = SUB_CHUNK_SIDE as i32 - 1;

/// Encodes an unsigned network varint for the synthetic palette and NBT.
fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            return;
        }
    }
}

/// Encodes a network NBT string, including its length.
fn string(value: &str, out: &mut Vec<u8>) {
    varint(value.len() as u32, out);
    out.extend_from_slice(value.as_bytes());
}

/// An active beacon with a revision field that makes NBT replacements distinct.
fn beacon_nbt(revision: i32) -> Vec<u8> {
    let mut out = vec![10, 0, 8];
    string("id", &mut out);
    string("Beacon", &mut out);
    for (name, value) in [("Levels", 1), ("revision", revision)] {
        out.push(3);
        string(name, &mut out);
        varint(((value << 1) ^ (value >> 31)) as u32, &mut out);
    }
    out.push(0);
    out
}

/// Commits a fixture event through the production stream before returning.
fn commit(stream: &mut chunk_pipeline::WorldStream, event: WorldEvent) {
    let sequence = stream.committed_sequence() + 1;
    stream.submit(sequence, event).unwrap();
    test_time::eventually("beacon fixture event committed", || {
        stream.poll([8.0, 8.0, 8.0], 0);
        stream.committed_sequence() == sequence
    });
}

/// A single loaded column with active beacons and an authored beam texture.
fn fixture(beacon_count: usize, eye: Vec3, fill_portals: bool) -> World {
    let bytes = assets::pinned_block_registry_bytes();
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(bytes, version).unwrap();
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let collisions = PhysicsCollisionRegistries::from_assets(
        bytes,
        &records,
        include_bytes!("../../../../crates/assets/data/block-physics-v2193.bin"),
        version,
    )
    .unwrap();
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: eye.to_array(),
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let range = protocol::vanilla_dimension_range(0).unwrap();
    let mut payload = Vec::new();
    for y in range.base_sub_chunk_y..0 {
        if fill_portals && y == -1 {
            payload.extend_from_slice(&[8, 1, 1]);
            varint(id("minecraft:end_portal") * 2, &mut payload);
        } else {
            payload.extend_from_slice(&[8, 0]);
        }
    }
    payload.extend_from_slice(&[8, 1, 1]);
    varint(id("minecraft:beacon") * 2, &mut payload);
    commit(
        &mut stream,
        WorldEvent::LevelChunk(protocol::LevelChunkEvent {
            dimension: 0,
            x: 0,
            z: 0,
            mode: protocol::LevelChunkMode::Inline {
                count: (1 - range.base_sub_chunk_y) as usize,
            },
            payload,
        }),
    );
    for index in 0..beacon_count {
        commit(
            &mut stream,
            WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
                dimension: 0,
                position: [
                    (index / SUB_CHUNK_SIDE) as i32,
                    BEACON_Y,
                    (index % SUB_CHUNK_SIDE) as i32,
                ],
                nbt: beacon_nbt(0),
            }),
        );
    }
    let beam_assets = assets::RuntimeBlockEntityAssets::decode(
        &assets::encode_block_entity_catalog(
            b"{}",
            render_model::MIN_DYNAMIC_ATLAS_WIDTH,
            16,
            &vec![255; render_model::MIN_DYNAMIC_ATLAS_WIDTH as usize * 16 * 4],
            &[assets::BlockEntityPlacement {
                name: "textures/entity/beacon_beam".into(),
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let page = assets::FontTexturePage {
        source_path: "font/fixture.png".into(),
        source_bytes: 4,
        source_sha256: [1; 32],
        pixels_sha256: sha2::Sha256::digest([255u8; 4]).into(),
        width: 1,
        height: 1,
        pixels: assets::FontPixels::Rgba8(std::sync::Arc::new([255; 4])),
    };
    let glyph = assets::GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0; 2],
        advance_64: 64,
    };
    let font = RuntimeFontCatalog::decode(
        &assets::encode_font_catalog([1; 32], &[glyph], &[page]).unwrap(),
        [1; 32],
    )
    .unwrap();
    let mut world = World::new();
    world.insert_resource(ClientWorld {
        stream: Some(stream),
        ..Default::default()
    });
    world.insert_resource(collisions);
    world.insert_resource(LocalViewPose::new(eye, Quat::IDENTITY));
    world.insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)));
    world.insert_resource(BlockEntityFont(Arc::new(font)));
    world.insert_resource(BlockEntityRuntime::new());
    world.insert_resource(render_scene::BlockEntitySceneResource(block_entity_scene(
        Some(&beam_assets),
    )));
    world.init_resource::<render_scene::BlockEntityFrameResource>();
    world.init_resource::<render_scene::StaticItemPlacementsResource>();
    world.init_resource::<AtmosphereFrame>();
    world.init_resource::<ActorFramePartialTick>();
    world.init_resource::<Time<Real>>();
    world
}

#[test]
fn beacon_cache_waits_for_distance_admission_even_after_column_edits() {
    let mut world = fixture(
        SUB_CHUNK_SIDE * SUB_CHUNK_SIDE,
        Vec3::new(8.0, 200.0, 8.0),
        false,
    );
    for revision in 0..2 {
        if revision != 0 {
            let mut client_world = world.resource_mut::<ClientWorld>();
            commit(
                client_world.stream.as_mut().unwrap(),
                WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
                    dimension: 0,
                    position: [0, BEACON_Y, 0],
                    nbt: beacon_nbt(revision),
                }),
            );
        }
        world.run_system_once(update_block_entity_scene).unwrap();
        let scan = &world.resource::<BlockEntityRuntime>().columns[&ChunkKey::new(0, 0, 0)];
        assert_eq!(scan.entities.len(), SUB_CHUNK_SIDE * SUB_CHUNK_SIDE);
        assert!(
            scan.beacons.is_empty(),
            "distant beacons must not scan their beams"
        );
        assert!(
            world
                .resource::<render_scene::BlockEntityFrameResource>()
                .overlay
                .is_empty()
        );
    }
    world.insert_resource(LocalViewPose::new(Vec3::new(8.0, 8.0, 8.0), Quat::IDENTITY));
    world.run_system_once(update_block_entity_scene).unwrap();
    let key = BlockEntityKey::new(0, 0, BEACON_Y, 0);
    let scan = &world.resource::<BlockEntityRuntime>().columns[&key.chunk()];
    assert_eq!(scan.beacons.len(), SUB_CHUNK_SIDE * SUB_CHUNK_SIDE);
    let sections = Arc::clone(&scan.beacons[&key].sections);
    assert!(!sections.is_empty());
    world.run_system_once(update_block_entity_scene).unwrap();
    assert!(Arc::ptr_eq(
        &sections,
        &world.resource::<BlockEntityRuntime>().columns[&key.chunk()].beacons[&key].sections
    ));
    let stone = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|record| record.name.as_ref() == "minecraft:stone")
    .unwrap()
    .sequential_id;
    let mut previous = sections;
    for event in [
        WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
            dimension: 0,
            position: key.position(),
            nbt: beacon_nbt(2),
        }),
        WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position: [0, BEACON_Y - 1, 0],
            layer: 0,
            network_id: stone,
        }]),
    ] {
        {
            let mut client_world = world.resource_mut::<ClientWorld>();
            commit(client_world.stream.as_mut().unwrap(), event);
        }
        world.run_system_once(update_block_entity_scene).unwrap();
        let current =
            &world.resource::<BlockEntityRuntime>().columns[&key.chunk()].beacons[&key].sections;
        assert!(
            !Arc::ptr_eq(&previous, current),
            "a column edit invalidates admitted beam sections"
        );
        assert_eq!(previous.as_ref(), current.as_ref());
        previous = Arc::clone(current);
    }
}

#[test]
fn beacon_cache_waits_when_portals_fill_the_submission_limit() {
    let mut world = fixture(1, Vec3::splat(8.0), true);
    world.run_system_once(update_block_entity_scene).unwrap();
    let scan = &world.resource::<BlockEntityRuntime>().columns[&ChunkKey::new(0, 0, 0)];
    assert_eq!(scan.portals.len(), MAX_SUBMISSIONS);
    assert_eq!(scan.entities.len(), 1);
    assert!(
        scan.beacons.is_empty(),
        "a full submission list must not scan a beacon beam"
    );
}
