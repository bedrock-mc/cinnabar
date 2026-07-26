use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

use ::meshing::{BlockClassifier, Neighbourhood, PackedBiomeRecord, mesh_sub_chunk};
use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_blob,
};
use protocol::{
    ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorKind, ActorMoveEvent,
    ActorPositionOrigin, ActorSpawnEvent, BiomeDefinitionEvent, BiomeDefinitionsEvent,
    BlockCrackAction, BlockCrackEvent, BlockEntityUpdateEvent, BlockUpdateEvent,
    ChangeDimensionEvent, DaylightCycleUpdateEvent, HudEvent, LevelChunkEvent, LevelChunkMode,
    MovePlayerEvent, MovePlayerMode, PLAYER_NETWORK_OFFSET, PlayerMovementCorrectionEvent,
    PublisherUpdateEvent, SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
    SubChunkUnavailable, UiEvent, WeatherChannel, WeatherUpdateEvent, WorldBootstrap, WorldEvent,
};
use world::{
    BlockEntityKey, BlockUpdate, ChunkKey, ChunkStore, DecodedBiomeColumn, DecodedBlockEntities,
    DecodedLevelChunk, MeshDependencyMask, SubChunk, SubChunkKey, SubChunkLight,
};

use super::*;
use crate::server_position;

mod light_scheduler;

mod mesh_dependency;

fn non_default_air_runtime_assets() -> RuntimeAssets {
    let cube = BlockVisual {
        faces: [0; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        contributor_role: assets::ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let air = BlockVisual {
        faces: [0; 6],
        flags: BlockFlags::AIR,
        kind: VisualKind::Invisible,
        contributor_role: assets::ContributorRole::Air,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| TextureMip {
            size,
            rgba8: vec![0; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let compiled = CompiledAssets {
        visuals: vec![cube, cube, air].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 3].into_boxed_slice(),
        hashed: vec![(1, 0), (2, 1), (0xdbf4_4120, 2)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    RuntimeAssets::decode(
        &encode_blob(&compiled)
            .expect("encode non-default stream air registry")
            .into_vec(),
    )
    .expect("decode non-default stream air registry")
}

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value << 1) ^ (value >> 31)) as u32;
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn uniform_sub_chunk(runtime_id: u32) -> SubChunk {
    let mut bytes = vec![8, 1, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    SubChunk::decode(&bytes).expect("decode uniform test subchunk")
}

fn camera_medium_assets() -> RuntimeAssets {
    let visual = |kind, role, faces, variant| BlockVisual {
        faces,
        flags: if kind == VisualKind::Invisible {
            BlockFlags::AIR
        } else if kind == VisualKind::Cube {
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        } else {
            BlockFlags::empty()
        },
        kind,
        contributor_role: role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant,
    };
    let visuals = vec![
        visual(
            VisualKind::Invisible,
            assets::ContributorRole::Air,
            [0; 6],
            0,
        ),
        visual(
            VisualKind::Liquid,
            assets::ContributorRole::LiquidAdditional,
            [1; 6],
            0,
        ),
        visual(
            VisualKind::Liquid,
            assets::ContributorRole::LiquidAdditional,
            [2; 6],
            0,
        ),
        visual(
            VisualKind::Cube,
            assets::ContributorRole::Primary,
            [0; 6],
            0,
        ),
    ];
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| TextureMip {
            size,
            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let compiled = CompiledAssets {
        visuals: visuals.into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 4].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            },
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_WATER_TINT,
                animation: NO_ANIMATION,
            },
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: assets::MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
                animation: NO_ANIMATION,
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn biome_payload(dimension: i32, biome_id: i32) -> Vec<u8> {
    let storage_count = protocol::vanilla_dimension_range(dimension)
        .expect("test dimension should have a vanilla range")
        .sub_chunk_count;
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(biome_id));
    payload.extend(std::iter::repeat_n(0xff, storage_count - 1));
    payload.push(0); // border-block count
    payload
}

fn biome_neighbourhood_with_center(
    center: Option<Arc<world::BiomeStorage>>,
) -> super::BiomeNeighbourhood {
    let mut biomes = std::array::from_fn(|_| None);
    biomes[::meshing::biome_neighbour_index(0, 0).unwrap()] = center;
    biomes
}

fn request_level_chunk_event(
    dimension: i32,
    x: i32,
    z: i32,
    mode: LevelChunkMode,
    biome_id: i32,
) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension,
        x,
        z,
        mode,
        payload: biome_payload(dimension, biome_id),
    })
}

fn inline_air_event(x: i32) -> WorldEvent {
    let mut payload = vec![9, 0, (-4_i8) as u8];
    payload.extend(biome_payload(0, 1));
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload,
    })
}

fn block_entity_nbt(id: &str, position: [i32; 3]) -> Vec<u8> {
    let mut bytes = vec![10, 0, 8, 2, b'i', b'd'];
    bytes.push(u8::try_from(id.len()).expect("test block-entity ID fits one VarUInt byte"));
    bytes.extend_from_slice(id.as_bytes());
    for (name, value) in [
        (b'x', position[0]),
        (b'y', position[1]),
        (b'z', position[2]),
    ] {
        bytes.extend([3, 1, name]);
        bytes.extend(zig_zag_i32(value));
    }
    bytes.push(0);
    bytes
}

fn block_entity_nbt_with_marker(id: &str, position: [i32; 3], marker: u8) -> Vec<u8> {
    let mut bytes = block_entity_nbt(id, position);
    bytes.pop();
    bytes.extend([1, 6, b'm', b'a', b'r', b'k', b'e', b'r', marker, 0]);
    bytes
}

fn idless_note_block_entity_nbt(position: [i32; 3], note: u8, powered: u8, marker: u8) -> Vec<u8> {
    let mut bytes = vec![10, 0];
    for (name, value) in [
        (b"note".as_slice(), note),
        (b"powered".as_slice(), powered),
        (b"marker".as_slice(), marker),
    ] {
        bytes.push(1);
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name);
        bytes.push(value);
    }
    for (name, value) in [
        (b'x', position[0]),
        (b'y', position[1]),
        (b'z', position[2]),
    ] {
        bytes.extend([3, 1, name]);
        bytes.extend(zig_zag_i32(value));
    }
    bytes.push(0);
    bytes
}

fn block_entity_visual_assets() -> RuntimeAssets {
    let visual = BlockVisual {
        faces: [0; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        contributor_role: assets::ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let compiled = CompiledAssets {
        visuals: vec![visual; 15_692].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 15_692].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn block_entity_visual_stream() -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(block_entity_visual_assets()),
        [0.0, 80.0, 0.0],
        None,
    )
}

fn inline_block_entity_event(chunk_x: i32, runtime_id: u32, nbt: Vec<u8>) -> WorldEvent {
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(runtime_id as i32));
    payload.extend(biome_payload(0, 1));
    payload.extend(nbt);
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x: chunk_x,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload,
    })
}

fn request_block_entity_event(chunk_x: i32, nbt: Vec<u8>) -> WorldEvent {
    request_block_entity_event_with_biome(chunk_x, 1, nbt)
}

fn request_block_entity_event_with_biome(chunk_x: i32, biome_id: i32, nbt: Vec<u8>) -> WorldEvent {
    let mut payload = biome_payload(0, biome_id);
    payload.extend(nbt);
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x: chunk_x,
        z: 0,
        mode: LevelChunkMode::LimitedRequests { highest: 1 },
        payload,
    })
}

fn requested_block_entity_sub_chunk_event(
    chunk_x: i32,
    runtime_id: u32,
    nbt: Vec<u8>,
) -> WorldEvent {
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(runtime_id as i32));
    payload.extend(nbt);
    WorldEvent::SubChunks(SubChunkBatchEvent {
        dimension: 0,
        entries: vec![SubChunkEntryEvent {
            position: [chunk_x, -4, 0],
            result: SubChunkResult::Success { payload },
        }],
    })
}

fn complete_pending_decode_jobs(stream: &mut WorldStream) {
    while let Some(job) = stream.pending_decode.pop_front() {
        let (sequence, event) = match job.job {
            super::DecodeJob::InlineLevelChunk {
                sequence,
                mut event,
                base_sub_chunk_y,
                count,
                biome_storage_count,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                let payload = std::mem::take(&mut event.payload);
                (
                    sequence,
                    super::PreparedWorldEvent::InlineLevelChunk {
                        event,
                        decoded: DecodedLevelChunk::decode_with_biomes_and_block_entities(
                            chunk,
                            base_sub_chunk_y,
                            count,
                            base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        ),
                        duration: std::time::Duration::ZERO,
                    },
                )
            }
            super::DecodeJob::RequestLevelChunk {
                sequence,
                mut event,
                biome_base_sub_chunk_y,
                biome_storage_count,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                let payload = std::mem::take(&mut event.payload);
                (
                    sequence,
                    super::PreparedWorldEvent::RequestLevelChunk {
                        event,
                        decoded: world::DecodedBiomeColumn::decode(
                            biome_base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        )
                        .and_then(|biomes| {
                            let block_entities = DecodedBlockEntities::decode_level_chunk_tail(
                                chunk,
                                &payload[biomes.bytes_consumed()..],
                            )?;
                            Ok((biomes, block_entities))
                        }),
                        duration: std::time::Duration::ZERO,
                    },
                )
            }
            super::DecodeJob::SubChunks { sequence, batch } => (
                sequence,
                super::PreparedWorldEvent::SubChunks {
                    dimension: batch.dimension,
                    entries: super::prepare_sub_chunks(batch),
                    duration: std::time::Duration::ZERO,
                },
            ),
            super::DecodeJob::BlockUpdates {
                sequence,
                batches,
                air_runtime_id,
            } => (
                sequence,
                super::PreparedWorldEvent::BlockUpdates {
                    result: batches
                        .into_iter()
                        .map(|batch| {
                            ChunkStore::prepare_sub_chunk_blocks(
                                batch.key,
                                batch.previous.as_deref(),
                                &batch.updates,
                                air_runtime_id,
                            )
                        })
                        .collect(),
                    duration: std::time::Duration::ZERO,
                },
            ),
            super::DecodeJob::BlockEntityUpdate { sequence, event } => {
                let key = BlockEntityKey::new(
                    event.dimension,
                    event.position[0],
                    event.position[1],
                    event.position[2],
                );
                (
                    sequence,
                    super::PreparedWorldEvent::BlockEntityUpdate {
                        key,
                        decoded: DecodedBlockEntities::decode_live(key, &event.nbt),
                        duration: std::time::Duration::ZERO,
                    },
                )
            }
        };
        stream.accept_decode_completion(super::DecodeCompletion {
            sequence,
            event,
            queue_wait: std::time::Duration::ZERO,
        });
    }
    stream.apply_ready();
}

fn cave_test_assets() -> RuntimeAssets {
    let compiled = CompiledAssets {
        visuals: vec![
            BlockVisual {
                faces: [0; 6],
                flags: BlockFlags::AIR,
                kind: VisualKind::Invisible,
                contributor_role: assets::ContributorRole::Air,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
            BlockVisual {
                faces: [1; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
                kind: VisualKind::Cube,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
            BlockVisual {
                faces: [2; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
        ]
        .into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 3].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION
            };
            3
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    let blob = encode_blob(&compiled).expect("encode cave-connectivity test assets");
    RuntimeAssets::decode(&blob).expect("decode cave-connectivity test assets")
}

fn cave_test_slab(runtime_id: u8) -> SubChunk {
    let mut words = vec![0_u32; 128];
    for y in 0..16 {
        for z in 0..16 {
            let linear = (8 << 8) | (z << 4) | y;
            words[linear / 32] |= 1 << (linear % 32);
        }
    }

    let mut encoded = vec![9, 1, 0, 3];
    for word in words {
        encoded.extend_from_slice(&word.to_le_bytes());
    }
    encoded.extend([4, 0, runtime_id << 1]);
    SubChunk::decode(&encoded).expect("decode cave-connectivity slab")
}

fn stream_with_one_expected_sub_chunk() -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    (stream, key)
}

fn apply_sub_chunk_result(
    stream: &mut WorldStream,
    key: SubChunkKey,
    result: super::PreparedSubChunkResult,
) {
    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: key.dimension,
        entries: vec![super::PreparedSubChunk {
            position: [key.x, key.y, key.z],
            result,
        }],
        duration: std::time::Duration::ZERO,
    });
}

fn stream_with_unsent_sub_chunks(
    count: u16,
) -> (WorldStream, Vec<SubChunkKey>, super::PendingSubChunkRequest) {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 0, 0);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: count },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let request = stream
        .pop_next_request()
        .expect("request-mode LevelChunk should enqueue one request");
    let keys = (0..count)
        .map(|offset| SubChunkKey::from_chunk(chunk, -4 + i32::from(offset)))
        .collect();
    (stream, keys, request)
}

fn acknowledge_request_sent(
    stream: &mut WorldStream,
    request: &super::PendingSubChunkRequest,
    sent_at: Instant,
) {
    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        sent_at,
    );
}

enum Action {
    Decode(DecodedLevelChunk),
    Update,
}

mod cases_01;
mod cases_02;
mod cases_03;
mod cases_04;
mod cases_05;
mod cases_06;
mod cases_07;
mod cases_08;
mod cases_09;
