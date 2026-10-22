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
use client_world::{
    COMMITTED_UI_CAPACITY,
    ingestion::{PreparedSubChunk, WorldStreamError},
};
use protocol::{
    ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorKind, ActorLinkEvent,
    ActorLinkType, ActorMotionEvent, ActorMoveEvent, ActorPositionOrigin, ActorRemoveEvent,
    ActorSpawnEvent, BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockCrackAction,
    BlockCrackEvent, BlockEntityUpdateEvent, BlockUpdateEvent, ChangeDimensionEvent,
    ChunkResyncEvent, DaylightCycleUpdateEvent, HudEvent, LevelChunkEvent, LevelChunkMode,
    MovePlayerEvent, MovePlayerMode, MovementCorrectionSubject, PLAYER_NETWORK_OFFSET,
    PlayerListEntry, PlayerListUpdateEvent, PlayerMovementCorrectionEvent, PublisherUpdateEvent,
    RespawnEvent, SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
    SubChunkUnavailable, UiEvent, WeatherChannel, WeatherUpdateEvent, WorldBootstrap, WorldEvent,
};
use render_api::{MAX_VIEW_RADIUS_CHUNKS, PublicationPermitStage, PublicationServiceConfig};
use world::{
    BlockEntityKey, BlockUpdate, ChunkKey, ChunkStore, DecodedBiomeColumn, DecodedBlockEntities,
    DecodedLevelChunk, MeshDependencyMask, RawBiomeIds, RawBlockIds, SubChunk, SubChunkKey,
    SubChunkLight,
};

use super::*;
use crate::server_position;

impl WorldStream {
    /// Existing admission fixtures use the same count limit for geometry and removals.
    fn dispatch_mesh_jobs(&mut self, camera: [f32; 3], budget: usize) -> usize {
        self.dispatch_mesh_jobs_with_limits(camera, budget, budget)
    }

    /// Publishes a local physics position with the stream's current ownership identity.
    fn retain_local(&mut self, position: [f32; 3]) -> bool {
        self.retain_for_local_player(
            self.authority.actor_session_id(),
            self.current_dimension(),
            self.form_dimension_epoch(),
            position,
        )
    }
}

/// Decode registries that keep every id, for fixtures committed straight to the store.
const RAW_IDS: RawBlockIds = RawBlockIds {
    air: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
};
const RAW_BIOMES: RawBiomeIds = RawBiomeIds { default_biome: 0 };

fn test_decode_ids() -> super::DecodeIds {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
    .decode_ids(0)
}

#[test]
fn level_chunk_bytes_submit_moves_backing_allocation_into_decode_job() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let payload = bytes::Bytes::from(vec![0x6b; 1024 * 1024]);
    let pointer = payload.as_ptr();
    stream
        .submit_level_chunk_bytes(
            1,
            LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 0 },
                payload: Vec::new(),
            },
            payload,
        )
        .expect("admit LevelChunk bytes");

    let Some(QueuedDecodeJob {
        job: DecodeJob::InlineLevelChunk { payload, .. },
        ..
    }) = stream.pending_decode.front()
    else {
        panic!("inline LevelChunk must enqueue one decode job")
    };
    assert_eq!(payload.as_ptr(), pointer);
}

mod block_cracks;
mod column_residency;
mod commit_budget;
mod dimension_ranges;
mod dimension_transfer;
mod job_snapshots;
mod light_scheduler;
mod local_retention;
mod neighbour_deadlines;
mod resend_light;

mod mesh_dependency;
mod seasonal_foliage;
mod world_clocks;

fn non_default_air_runtime_assets() -> RuntimeAssets {
    let cube = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces: [0; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        contributor_role: assets::ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let air = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces: [0; 6],
        flags: BlockFlags::AIR,
        kind: VisualKind::Invisible,
        contributor_role: assets::ContributorRole::Air,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
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
            ..assets::Material::unvaried()
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
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
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

fn camera_medium_assets() -> RuntimeAssets {
    let visual = |kind, role, faces, variant| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
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
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
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
                ..assets::Material::unvaried()
            },
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_WATER_TINT,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
            },
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: assets::MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
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

fn define_custom_biomes(stream: &mut WorldStream, ids: impl IntoIterator<Item = u16>) {
    let definitions = ids
        .into_iter()
        .map(|id| BiomeDefinitionEvent {
            biome_id: Some(id),
            name: Arc::from(format!("test:biome_{id}")),
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            max_snow_accumulation: None,
            map_water_color: 0,
        })
        .collect::<Vec<_>>();
    stream.apply_immediate(
        WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
            definitions: Arc::from(definitions),
        }),
        None,
    );
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

/// Reads the pinned registry once for visual fixtures and their event identities.
fn block_entity_visual_registry() -> &'static [assets::RegistryRecord] {
    static RECORDS: std::sync::OnceLock<Box<[assets::RegistryRecord]>> = std::sync::OnceLock::new();
    RECORDS.get_or_init(|| {
        assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .expect("decode pinned block-entity visual registry")
    })
}

/// Selects a real state for a named block used in a visual event.
fn block_entity_runtime_id(name: &str) -> u32 {
    block_entity_visual_registry()
        .iter()
        .find(|record| record.name.as_ref() == name)
        .expect("visual fixture block exists in the pinned registry")
        .sequential_id
}

/// Gives synthetic cube artwork the pinned registry's state and hash identities.
fn block_entity_visual_assets() -> RuntimeAssets {
    let visual = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces: [0; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        contributor_role: assets::ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let records = block_entity_visual_registry();
    let count = records
        .iter()
        .map(|record| record.sequential_id)
        .max()
        .unwrap() as usize
        + 1;
    let mut hashed = records
        .iter()
        .map(|record| (record.network_hash, record.sequential_id))
        .collect::<Vec<_>>();
    hashed.sort_unstable();
    let compiled = CompiledAssets {
        visuals: vec![visual; count].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); count].into_boxed_slice(),
        hashed: hashed.into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
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
        provenance: *assets::pinned_world_provenance(),
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

/// Builds the usual visual fixture with the default local player identity.
fn block_entity_visual_stream() -> WorldStream {
    block_entity_visual_stream_for_player(1)
}

/// Builds the visual fixture with its local identity fixed at session creation.
fn block_entity_visual_stream_for_player(local_player_unique_id: i64) -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: block_entity_runtime_id("minecraft:air"),
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
            diagnostics: None,
            position: [chunk_x, -4, 0],
            result: SubChunkResult::Success { payload },
        }],
    })
}

/// Runs one poll's commit pass after its deadline: light work plus its one heavy step.
fn apply_spent_slice(stream: &mut WorldStream) {
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.poll_heavy_guarantee = true;
    stream.apply_ready();
}

/// A block batch too large to prepare on the commit thread, so it keeps its decode fence.
fn worker_block_batch(chunk_x: i32, network_id: u32) -> WorldEvent {
    WorldEvent::BlockUpdates(
        (0..=INLINE_BLOCK_MUTATION_UPDATES as i32)
            .map(|index| BlockUpdateEvent {
                dimension: 0,
                position: [chunk_x * 16 + index % 16, -64, index / 16],
                layer: 0,
                network_id,
            })
            .collect(),
    )
}

/// Finishes fixture decoding and every sliced ordered commit it releases.
fn complete_pending_decode_jobs(stream: &mut WorldStream) {
    loop {
        stream.promote_deferred_ingress();
        let Some(job) = stream.pending_decode.pop_front() else {
            if !settle_ready_commits(stream) || stream.pending_decode.is_empty() {
                break;
            }
            continue;
        };
        complete_decode_job(stream, job);
    }
}

/// Applies ready commits until nothing moves; true when anything did.
fn settle_ready_commits(stream: &mut WorldStream) -> bool {
    let mut moved = false;
    loop {
        let before = (
            stream.order.admitted_count(),
            stream.order.next_sequence(),
            stream.order.pending_batch_sequence().is_some(),
        );
        stream.apply_ready();
        stream.promote_deferred_ingress();
        let after = (
            stream.order.admitted_count(),
            stream.order.next_sequence(),
            stream.order.pending_batch_sequence().is_some(),
        );
        if before == after && stream.order.pending_batch_sequence().is_none() {
            return moved;
        }
        moved = true;
    }
}

/// Runs one popped decode job on the test thread and accepts its completion.
fn complete_decode_job(stream: &mut WorldStream, job: super::QueuedDecodeJob) {
    {
        let (sequence, event) = match job.job {
            super::DecodeJob::InlineLevelChunk {
                sequence,
                event,
                payload,
                slots,
                count,
                ids,
                previous,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                let decoded = DecodedLevelChunk::decode_inline_reusing(
                    chunk,
                    slots,
                    count,
                    &payload,
                    &ids,
                    &ids,
                    previous.as_deref(),
                );
                let light_neutral = previous.map_or_else(Vec::new, |previous| {
                    client_world::ingestion::light_neutral_sections(&previous, &decoded, &ids)
                });
                (
                    sequence,
                    super::PreparedWorldEvent::InlineLevelChunk {
                        event,
                        decoded,
                        light_neutral,
                        duration: std::time::Duration::ZERO,
                    },
                )
            }
            super::DecodeJob::RequestLevelChunk {
                sequence,
                event,
                payload,
                slots,
                ids,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                (
                    sequence,
                    super::PreparedWorldEvent::RequestLevelChunk {
                        event,
                        decoded: world::decode_column_tail(chunk, slots, &payload, &ids),
                        duration: std::time::Duration::ZERO,
                    },
                )
            }
            super::DecodeJob::SubChunks {
                sequence,
                batch,
                ids,
            } => (
                sequence,
                super::PreparedWorldEvent::SubChunks {
                    dimension: batch.dimension,
                    entries: client_world::ingestion::prepare_sub_chunks(batch, &ids),
                    duration: std::time::Duration::ZERO,
                },
            ),
            super::DecodeJob::BlockUpdates {
                sequence,
                batches,
                ids,
            } => (
                sequence,
                super::PreparedWorldEvent::BlockUpdates {
                    result: client_world::ingestion::prepare_block_mutations(batches, &ids),
                    duration: std::time::Duration::ZERO,
                },
            ),
            job @ super::DecodeJob::SyncedBlockUpdates { .. } => {
                let completion = job.run(Instant::now());
                (completion.sequence, completion.event)
            }
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
}

fn cave_test_assets() -> RuntimeAssets {
    let compiled = CompiledAssets {
        visuals: vec![
            BlockVisual {
                native_material: assets::native_material::NativeMaterialFacts::Unresolved,
                faces: [0; 6],
                flags: BlockFlags::AIR,
                kind: VisualKind::Invisible,
                support: assets::VisualSupport::Exact,
                contributor_role: assets::ContributorRole::Air,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
                destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            },
            BlockVisual {
                native_material: assets::native_material::NativeMaterialFacts::Unresolved,
                faces: [1; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
                kind: VisualKind::Cube,
                support: assets::VisualSupport::Exact,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
                destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            },
            BlockVisual {
                native_material: assets::native_material::NativeMaterialFacts::Unresolved,
                faces: [2; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: assets::VisualSupport::Exact,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
                destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            },
        ]
        .into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 3].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
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
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
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
    SubChunk::decode(&encoded, &RawBlockIds { air: 0 })
}

use sub_chunk_fixtures::*;
mod sub_chunk_batching;
mod sub_chunk_fixtures;

mod actor_authority;
mod actor_light;
mod admission_backpressure;
mod audio_events;
mod biome_admission;
mod block_entity_admission;
mod block_update_admission;
#[path = "tests/camera_events.rs"]
mod camera_events;
mod cave_connectivity;
mod cohort_progress;
mod column_admission;
mod entity_occlusion;
mod environment_admission;
mod environment_sampling;
mod far_summaries;
mod forced_remesh;
mod full_detail;
mod inbound_lanes;
mod inline_cohort;
mod inventory_commit_fence;
mod lane_footprints;
mod lenient_decode;
mod local_abilities;
mod mesh_dispatch;
mod mesh_publication;
mod mesh_removal;
mod near_drawn;
mod player_grid_interest;
mod player_retention;
mod prediction;
mod publication_metrics;
mod publisher_epochs;
mod publisher_scope;
mod remote_motion_replay;
mod render_distance;
mod request_admission;
mod request_lifecycle;
mod request_order;
mod service;
mod streaming_harness;
mod sub_chunk_recovery;
mod ui_epochs;
mod vertical_detail;

mod actor_block_sync;
#[path = "tests/ordered_commits.rs"]
mod ordered_commits;
