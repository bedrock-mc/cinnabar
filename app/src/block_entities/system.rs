//! The per-frame system, carrier loading and the caches behind them.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::platform::collections::HashMap;
use bevy::prelude::IntoScheduleConfigs;
use client_runtime::network::NetworkHandle;
use std::{path::Path, sync::Arc};

use assets::{RuntimeBlockEntityAssets, RuntimeFontCatalog};
use bevy::prelude::{
    App, Mat4, Projection, Query, Real, Res, ResMut, Resource, Time, Transform, Update, Vec3, With,
};
use ui::TextLayoutCache;
use world::{ChunkKey, SUB_CHUNK_SIDE, SubChunkKey};
use {
    render_model::{
        AtlasRect, BellModel, BlockEntityKind, BlockEntityLight, BlockEntityScene,
        BlockEntitySubmission, ConduitModel, CrackShapeCache, FramedMap, SceneClock, SignFace,
        SignModel, StaticItemPlacement, item_frame_item_transform, matrix_rows,
    },
    render_scene::AtmosphereFrame,
};

use super::{
    books,
    containers::{ContainerKind, ContainerLids, cue_is_open},
    cracks::{CrackLayers, crack_instances, with_second_parts},
    describe::{HeldItem, Template, describe},
    sign_text,
    state::BlockState,
};
use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::{
        telemetry::camera_sub_chunk_key, visibility::CaveVisibilityCache, world::ClientWorld,
    },
    client_interaction::movement::PhysicsCollisionRegistries,
    client_presentation::{actor_publication::ActorFramePartialTick, local_player::LocalViewPose},
};

const BLOCK_ENTITY_ASSETS_FILENAME: &str = assets::carriers::BLOCK_ENTITY.output;
/// Block entities farther than this from the eye are not drawn.
pub(super) const SCAN_RADIUS_BLOCKS: f32 = 64.0;
const MAX_SUBMISSIONS: usize = 4_096;
const TICKS_PER_SECOND: f64 = 20.0;
const TEXT_CACHE_ENTRIES: usize = 256;
const TEXT_CACHE_BYTES: usize = 2 * 1024 * 1024;

mod beacon;
#[cfg(test)]
mod beacon_tests;
mod bed;
mod bell_rings;
use bell_rings::{BELL_RING_EVENT_TYPE, bell_elapsed, prune_bell_rings};
pub(super) mod columns;
mod crystal_beams;
mod culling;
mod dragon_death;
mod portals;

#[cfg(test)]
mod beacon_tint_tests;

/// Reads the optional block-entity carrier next to the world carrier, which the block-entity
/// scene and worn heads share; on absence or corruption logs once and returns `None`.
pub(crate) fn load_block_entity_carrier(
    world_asset_path: &Path,
) -> Option<Arc<RuntimeBlockEntityAssets>> {
    let path = world_asset_path.with_file_name(BLOCK_ENTITY_ASSETS_FILENAME);
    let bytes = match diagnostics::bounded_file::read(
        &path,
        assets::MAX_BLOCK_ENTITY_CARRIER_BYTES as u64,
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            bevy::log::warn!(
                "block-entity carrier {} unavailable ({error}); block-entity models, sign text, break cracks and worn heads are not drawn; rebuild with: make block-entity-assets",
                path.display()
            );
            return None;
        }
    };
    match RuntimeBlockEntityAssets::decode(&bytes) {
        Ok(assets) => {
            bevy::log::info!(
                "loaded block-entity carrier from {} ({} textures, atlas {:?})",
                path.display(),
                assets.placements().len(),
                assets.atlas_size()
            );
            Some(Arc::new(assets))
        }
        Err(error) => {
            bevy::log::warn!(
                "block-entity carrier {} is invalid ({error}); block-entity models, sign text, break cracks and worn heads are not drawn; rebuild with: make block-entity-assets",
                path.display()
            );
            None
        }
    }
}

/// A scene drawing `assets`, or nothing without them.
pub(crate) fn block_entity_scene(assets: Option<&RuntimeBlockEntityAssets>) -> BlockEntityScene {
    let mut scene = BlockEntityScene::default();
    if let Some(assets) = assets {
        scene.install_assets(assets);
    }
    scene
}

/// The UI font used to rasterize sign text.
#[derive(Resource)]
pub(crate) struct BlockEntityFont(pub(crate) Arc<RuntimeFontCatalog>);

struct BlockInfo {
    name: Arc<str>,
    state: BlockState,
    backing: Option<assets::facts::BlockEntityBacking<'static>>,
}

#[derive(Resource)]
pub(crate) struct BlockEntityRuntime {
    lids: ContainerLids,
    beacon_tints: assets::BeaconTintCatalog,
    /// Scan results per loaded column, rebuilt when the column changes.
    columns: HashMap<ChunkKey, columns::ColumnScan>,
    rescans: columns::RescanOrder,
    frame: u64,
    blocks: HashMap<u32, Option<Arc<BlockInfo>>>,
    layouts: TextLayoutCache,
    shapes: CrackShapeCache,
    bell_rings: HashMap<[i32; 3], (u64, f64)>,
    /// Framed map ids seen this frame without an image.
    missing_maps: Vec<i64>,
    /// When each map id was last requested from the server, in real seconds.
    map_requests: HashMap<i64, f64>,
    /// Session and dimension the runtime-id keyed caches were filled from.
    session: Option<(u64, i32)>,
    beam_source: Option<(Arc<assets::RuntimeAssets>, Option<protocol::DimensionRange>)>,
    /// Enchanting-table books, ticked like the native block actor.
    books: books::TableBooks,
    /// This frame's submissions, kept empty between frames so their buffer is reused.
    submissions: Vec<BlockEntitySubmission>,
}

impl BlockEntityRuntime {
    pub(crate) fn new() -> Self {
        Self {
            lids: ContainerLids::default(),
            beacon_tints: assets::BeaconTintCatalog::pinned(),
            columns: HashMap::new(),
            rescans: columns::RescanOrder::default(),
            frame: 0,
            blocks: HashMap::new(),
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            shapes: CrackShapeCache::default(),
            bell_rings: HashMap::new(),
            missing_maps: Vec::new(),
            map_requests: HashMap::new(),
            session: None,
            beam_source: None,
            books: books::TableBooks::default(),
            submissions: Vec::new(),
        }
    }

    /// Invalidates cached column rules when pack light data or dimension height changes.
    fn bind_beam_source(
        &mut self,
        assets: &Arc<assets::RuntimeAssets>,
        range: Option<protocol::DimensionRange>,
    ) {
        if !self
            .beam_source
            .as_ref()
            .is_some_and(|(previous, previous_range)| {
                Arc::ptr_eq(previous, assets) && *previous_range == range
            })
        {
            self.beam_source = Some((Arc::clone(assets), range));
            self.columns.clear();
            self.blocks.clear();
        }
    }

    /// Runtime ids, block states and positions mean nothing across sessions, so a
    /// session or dimension change drops every cache keyed by them.
    fn bind_session(&mut self, session: Option<(u64, i32)>) {
        if self.session == session {
            return;
        }
        self.session = session;
        self.beam_source = None;
        self.lids = ContainerLids::default();
        self.missing_maps.clear();
        self.columns.clear();
        self.rescans = columns::RescanOrder::default();
        self.blocks.clear();
        self.shapes.clear();
        self.bell_rings.clear();
        self.map_requests.clear();
        self.books = books::TableBooks::default();
    }
}

impl BlockEntityRuntime {
    /// Width of one sign line in design pixels; `None` when the font cannot lay it out.
    pub(crate) fn line_width_design_pixels(
        &mut self,
        font: &RuntimeFontCatalog,
        text: &str,
    ) -> Option<f32> {
        sign_text::line_width_design_pixels(text, font, &mut self.layouts)
    }
}

pub(crate) fn configure(app: &mut App, font: Arc<RuntimeFontCatalog>) {
    app.insert_resource(BlockEntityFont(font))
        .insert_resource(BlockEntityRuntime::new())
        .add_systems(
            Update,
            (
                update_block_entity_scene
                    .after(crate::runtime::network::prepare_actor_render_frame),
                request_missing_maps,
            )
                .chain(),
        );
}

/// Brightness per light level, matching the terrain lighting curve.
const LIGHT_CURVE: [f32; 16] = [
    0.0,
    0.017_543_86,
    0.037_037_037,
    0.058_823_53,
    0.083_333_336,
    0.111_111_11,
    0.142_857_15,
    0.179_487_18,
    0.222_222_22,
    0.272_727_28,
    0.333_333_34,
    0.407_407_4,
    0.5,
    0.619_047_64,
    0.777_777_8,
    1.0,
];
/// Lowest sky-light transfer at night, as in terrain lighting.
const NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.083_333_336;

/// Light multiplier from retained block/sky levels and the current daylight transfer.
fn light_factor(block: u8, sky: u8, daylight: f32) -> f32 {
    let curve = |level: u8| LIGHT_CURVE[usize::from(level.min(15))];
    let transfer = daylight.clamp(0.0, 1.0).max(NIGHT_SKY_TRANSFER_FLOOR);
    curve(block).max(curve(sky) * transfer)
}

fn model_light(kind: &BlockEntityKind, block: u8, sky: u8, daylight: f32) -> BlockEntityLight {
    // Vanilla lights a skull with the world light at its block position,
    // through mob_head's ordinary entity material. The enchanting table's book is an entity
    // model lit the same way, so the table's own glow lights it at night (provisional).
    if matches!(
        kind,
        BlockEntityKind::Skull(_)
            | BlockEntityKind::Chalkboard(_)
            | BlockEntityKind::EnchantTable { .. }
    ) {
        BlockEntityLight::Actor { block, sky }
    } else {
        light_factor(block, sky, daylight).into()
    }
}

/// Caches the canonical state and generated route for one admitted runtime identity.
fn block_info(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> Option<Arc<BlockInfo>> {
    runtime
        .blocks
        .entry(runtime_id)
        .or_insert_with(|| {
            let name = collisions.block_identifier(mode, runtime_id)?;
            let state = collisions
                .block_canonical_state(mode, runtime_id)
                .map(BlockState::parse)
                .unwrap_or_default();
            Some(Arc::new(BlockInfo {
                name: Arc::from(name),
                state,
                backing: assets::facts::BlockEntityFacts::pinned().for_registry(
                    runtime_id,
                    mode,
                    collisions.breg_sha256(),
                ),
            }))
        })
        .clone()
}

/// The other block of `entry`'s two-block structure and its layers, while that block is still
/// the same kind (a lone door half has no partner to crack).
fn second_crack_part(
    collisions: &PhysicsCollisionRegistries,
    store: &world::ChunkStore,
    mode: assets::NetworkIdMode,
    dimension: i32,
    entry: &chunk_pipeline::ActiveBlockCrack,
) -> Option<([i32; 3], CrackLayers)> {
    let id = entry.layers[0]?;
    let offset = collisions.second_part_offset(mode, id)?;
    let position: [i32; 3] = std::array::from_fn(|axis| entry.position[axis] + offset[axis]);
    let side = SUB_CHUNK_SIDE as i32;
    let key = world::SubChunkKey::new(
        dimension,
        position[0].div_euclid(side),
        position[1].div_euclid(side),
        position[2].div_euclid(side),
    );
    let chunk = store.sub_chunk(key)?;
    let [x, y, z] = position.map(|value| value.rem_euclid(side) as u8);
    let layers: CrackLayers = std::array::from_fn(|layer| chunk.runtime_id(layer, x, y, z));
    let other = layers[0]?;
    (collisions.block_identifier(mode, other) == collisions.block_identifier(mode, id))
        .then_some((position, layers))
}

/// The portal surface `runtime_id` draws, if any.
fn portal_kind(
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> Option<BlockEntityKind> {
    let backing = assets::facts::BlockEntityFacts::pinned().for_registry(
        runtime_id,
        mode,
        collisions.breg_sha256(),
    )?;
    match backing.entity.renderer {
        assets::facts::BlockEntityRenderer::EndPortal => Some(BlockEntityKind::EndPortal),
        assets::facts::BlockEntityRenderer::EndGateway => Some(BlockEntityKind::EndGateway),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_block_entity_scene(
    client_world: Res<ClientWorld>,
    actor_partial_tick: Res<ActorFramePartialTick>,
    camera: Query<(&Transform, &Projection), With<client_presentation::camera::FlyCamera>>,
    collisions: Res<PhysicsCollisionRegistries>,
    view: Res<LocalViewPose>,
    ui: Res<UiRuntimeResource>,
    atmosphere: Res<AtmosphereFrame>,
    time: Res<Time<Real>>,
    font: Res<BlockEntityFont>,
    mut runtime: ResMut<BlockEntityRuntime>,
    mut scene: ResMut<render_scene::BlockEntitySceneResource>,
    mut frame: ResMut<render_scene::BlockEntityFrameResource>,
    mut placements: ResMut<render_scene::StaticItemPlacementsResource>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
    cave: Option<Res<CaveVisibilityCache>>,
) {
    let camera = camera.single().ok();
    if !scene.has_assets() {
        dragon_death::update_without_atlas(
            client_world.stream.as_ref(),
            actor_partial_tick.0,
            camera.map(|(transform, _)| transform.translation),
            &mut scene,
            &mut frame,
        );
        return;
    }
    // Timed in the body: a span around the chain would also count waiting on actor publication.
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::BlockEntities));
    let now_seconds = time.elapsed_secs_f64();
    let clock = SceneClock {
        ticks: now_seconds * TICKS_PER_SECOND,
    };
    let Some(stream) = client_world.stream.as_ref() else {
        runtime.bind_session(None);
        placements.0.0.clear();
        frame.0 = scene.update(clock, &[], &[]).clone();
        return;
    };
    let runtime = &mut *runtime;
    runtime.bind_session(Some((
        stream.authority().actor_session_id(),
        stream.current_dimension(),
    )));
    runtime.bind_beam_source(
        stream.runtime_assets(),
        stream
            .authority()
            .dimension_range(stream.current_dimension()),
    );
    runtime.missing_maps.clear();
    let dimension = stream.current_dimension();
    let beam_top = stream.authority().dimension_range(dimension).map(|range| {
        (range.base_sub_chunk_y + range.sub_chunk_count as i32) * SUB_CHUNK_SIDE as i32
    });
    let store = stream.collision_store();
    let mode = stream.network_id_mode();
    let eye = view.eye_translation();
    let delta = time.delta_secs();
    let daylight = atmosphere.daylight();
    let camera_position = camera.map(|(transform, _)| transform.translation);
    let clip_from_world = camera.map(|(transform, projection)| {
        projection.get_clip_from_view() * transform.to_matrix().inverse()
    });
    let culling = cave
        .as_deref()
        .zip(camera_position)
        .map(|(cave, position)| {
            culling::CaveCulling::new(
                cave,
                camera_sub_chunk_key(dimension, position),
                stream.connectivity_generation(),
                |key: SubChunkKey| stream.has_sub_chunk_connectivity(key),
            )
        });
    let mut work = culling::BlockEntityWork::default();
    let mut occlusion_budget = meshing::occlusion::OcclusionBudget::default();
    let occlusion_eye = camera
        .zip(clip_from_world)
        .and_then(|((transform, _), matrix)| {
            let eye = transform.translation.to_array();
            render_model::visibility_bounds::perspective_supports_occlusion(
                matrix,
                eye,
                f64::from(meshing::occlusion::OCCLUSION_EYE_GAP_BLOCKS),
            )
            .then_some(eye)
        });

    let cracks = ui
        .block_crack_snapshot()
        .filter(|snapshot| snapshot.dimension == dimension)
        .map_or_else(Vec::new, |snapshot| {
            let assets = stream.runtime_assets();
            let shapes = &mut runtime.shapes;
            let entries = with_second_parts(&snapshot.entries, |entry| {
                second_crack_part(&collisions, store, mode, dimension, entry)
            });
            crack_instances(&entries, |entry| {
                let runtime_id = entry.layers.iter().flatten().next().copied();
                let template = runtime_id.and_then(|id| {
                    assets.model_template_at(mode, id, entry.position, |position| {
                        let side = SUB_CHUNK_SIDE as i32;
                        let key = SubChunkKey::new(
                            dimension,
                            position[0].div_euclid(side),
                            position[1].div_euclid(side),
                            position[2].div_euclid(side),
                        );
                        let [x, y, z] = position.map(|value| value.rem_euclid(side) as u8);
                        store.sub_chunk(key)?.runtime_id(0, x, y, z)
                    })
                });
                shapes.shape(assets, mode, runtime_id, entry.position, template)
            })
        });

    runtime.books.begin(clock.ticks);
    let readers = book_readers(stream, eye);
    let mut submissions = std::mem::take(&mut runtime.submissions);
    runtime.frame = runtime.frame.wrapping_add(1);
    let frame_stamp = runtime.frame;
    // Moved out so `resolve` can borrow a template while mutating the rest of the runtime.
    let mut scans = std::mem::take(&mut runtime.columns);
    let mut held = std::mem::take(&mut placements.0.0);
    held.clear();
    runtime.lids.begin();
    let mut rescans_left = columns::MAX_COLUMN_RESCANS_PER_FRAME;
    let eye_column = [eye.x, eye.z].map(|axis| (axis / SUB_CHUNK_SIDE as f32).floor() as i32);
    let chunk_range = |center: f32| {
        ((center - SCAN_RADIUS_BLOCKS) / SUB_CHUNK_SIDE as f32).floor() as i32
            ..=((center + SCAN_RADIUS_BLOCKS) / SUB_CHUNK_SIDE as f32).floor() as i32
    };
    let selected = runtime.rescans.select(
        chunk_range(eye.x)
            .flat_map(|x| chunk_range(eye.z).map(move |z| ChunkKey::new(dimension, x, z)))
            .filter(|key| {
                !columns::is_near_column(eye_column, key.x, key.z)
                    && store.chunk(*key).is_some_and(|chunk| {
                        scans.get(key).is_none_or(|scan| !scan.is_current(chunk))
                    })
            }),
    );
    let prepare_zone = bevy::log::info_span!("block_entity.prepare").entered();
    'columns: for chunk_x in chunk_range(eye.x) {
        for chunk_z in chunk_range(eye.z) {
            let chunk_key = ChunkKey::new(dimension, chunk_x, chunk_z);
            let Some(chunk) = store.chunk(chunk_key) else {
                continue;
            };
            let previous = scans.remove(&chunk_key);
            let near = columns::is_near_column(eye_column, chunk_x, chunk_z);
            let mut deferred = 0;
            let budget = if near || selected.contains(&Some(chunk_key)) {
                &mut rescans_left
            } else {
                &mut deferred
            };
            let Some(mut scan) = columns::frame_scan(
                previous,
                chunk,
                near,
                budget,
                columns::ColumnScan::is_current,
                |previous| {
                    let portals = portals::column_cells(chunk_key, chunk, |id| {
                        portal_kind(&collisions, mode, id)
                    });
                    let entities = columns::routed_entities(
                        chunk,
                        previous,
                        |id, runtime_id, nbt, position| {
                            block_info(runtime, &collisions, mode, runtime_id)
                                .zip(nbt.parse())
                                .and_then(|(info, root)| {
                                    describe(
                                        id,
                                        info.backing?,
                                        &info.name,
                                        &info.state,
                                        &root,
                                        position,
                                    )
                                })
                        },
                    );
                    columns::ColumnScan::new(chunk, portals, entities)
                },
            ) else {
                continue;
            };
            scan.seen_frame = frame_stamp;
            let before_portals = submissions.len();
            portals::submit(&mut submissions, &scan.portals, eye);
            let portals = (submissions.len() - before_portals) as u32;
            work.candidates += portals;
            work.prepared += portals;
            let mut full = false;
            for entity in &scan.entities {
                if submissions.len() >= MAX_SUBMISSIONS {
                    full = true;
                    break;
                }
                let Some(template) = entity.template.as_ref() else {
                    continue;
                };
                let [x, y, z] = entity.key.position();
                let center = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                if center.distance_squared(eye) > SCAN_RADIUS_BLOCKS * SCAN_RADIUS_BLOCKS {
                    continue;
                }
                // Hidden entities keep animations, map requests and canvases without geometry.
                let mut hidden =
                    !work.admit(culling.as_ref(), clip_from_world, template, [x, y, z]);
                if !hidden
                    && let Some((low, high)) = culling::bounds(template, [x, y, z])
                    && let Some(eye) = occlusion_eye
                    && let Some(cave) = cave.as_deref()
                {
                    let _zone = bevy::log::info_span!("block_entity.occlusion").entered();
                    hidden = stream.terrain_hides_box(
                        &mut occlusion_budget,
                        eye,
                        low,
                        high,
                        |_, key| cave.is_visible(key) && cave.rendered.contains_key(&key),
                    );
                    work.prepared -= u32::from(hidden);
                }
                let context = FrameContext {
                    stream,
                    collisions: &collisions,
                    eye,
                    delta_seconds: delta,
                    now_seconds,
                    font: &font.0,
                    readers: &readers,
                };
                let kind = if matches!(template, Template::Beacon) {
                    beam_top.map(|top| {
                        let model = scan.beacons.entry(entity.key).or_insert_with(|| {
                            beacon::model(
                                runtime,
                                &collisions,
                                stream.runtime_assets(),
                                store,
                                dimension,
                                mode,
                                [x, y, z],
                                top,
                            )
                        });
                        BlockEntityKind::Beacon(model.clone())
                    })
                } else {
                    resolve(
                        template,
                        [x, y, z],
                        &context,
                        runtime,
                        &mut scene,
                        &mut held,
                        hidden,
                    )
                };
                if let Some(kind) = kind {
                    // Stateless portal surfaces are admitted from the primary palette above.
                    // A missing or stale NBT record must never add or remove their geometry.
                    if matches!(
                        kind,
                        BlockEntityKind::EndPortal | BlockEntityKind::EndGateway
                    ) {
                        continue;
                    }
                    let light = if let BlockEntityKind::Bed(model) = &kind {
                        bed::light(*model, [x, y, z], |position| {
                            stream.light_level_at(position)
                        })
                    } else {
                        let (block_light, sky_light) = stream.light_level_at(center.to_array());
                        model_light(&kind, block_light, sky_light, daylight)
                    };
                    submissions.push(BlockEntitySubmission {
                        block: [x, y, z],
                        light,
                        kind,
                    });
                }
            }
            scans.insert(chunk_key, scan);
            if full {
                break 'columns;
            }
        }
    }
    drop(prepare_zone);
    runtime.lids.finish();
    runtime.books.finish();
    scans.retain(|_, scan| scan.seen_frame == frame_stamp);
    runtime.columns = scans;
    prune_bell_rings(&mut runtime.bell_rings, now_seconds, |position| {
        stream
            .block_event_cue(*position)
            .filter(|cue| cue.event_type == BELL_RING_EVENT_TYPE)
            .map(|cue| cue.sequence)
    });
    let before_global = submissions.len();
    crystal_beams::submit(
        &mut submissions,
        stream.authority().crystal_beams(actor_partial_tick.0),
        camera_position,
        |runtime_id, owner_position| {
            let actor = stream.authority().actor(runtime_id)?;
            stream.solved_light_at(actor.brightness_sample_position(owner_position))
        },
    );
    dragon_death::submit(
        &mut submissions,
        stream.authority().dragon_death_rays(actor_partial_tick.0),
        camera_position,
        |runtime_id, owner_position| {
            let actor = stream.authority().actor(runtime_id)?;
            stream.solved_light_at(actor.brightness_sample_position(owner_position))
        },
    );
    let global = (submissions.len() - before_global) as u32;
    work.candidates += global;
    work.prepared += global;
    placements.0.0 = held;
    let _zone = bevy::log::info_span!(
        "block_entity.scene_update",
        candidates = work.candidates,
        prepared = work.prepared,
        hidden_by_visibility = work.candidates - work.prepared,
        probes = meshing::occlusion::ENTITY_OCCLUSION_PROBES - occlusion_budget.remaining(),
        submitted = submissions.len()
    )
    .entered();
    frame.0 = scene.update(clock, &cracks, &submissions).clone();
    submissions.clear();
    runtime.submissions = submissions;
}

struct FrameContext<'a> {
    stream: &'a chunk_pipeline::WorldStream,
    collisions: &'a PhysicsCollisionRegistries,
    eye: Vec3,
    delta_seconds: f32,
    now_seconds: f64,
    font: &'a RuntimeFontCatalog,
    /// Eye-level positions of every player who could read an enchanting table's book.
    readers: &'a [Vec3],
}

/// The local eye and every remote player's eye-level position, as Bedrock measures readers.
fn book_readers(stream: &chunk_pipeline::WorldStream, eye: Vec3) -> Vec<Vec3> {
    let remote = stream
        .authority()
        .remote_actors()
        .filter(|actor| matches!(&actor.kind, protocol::ActorKind::Player { .. }))
        .map(|actor| Vec3::from_array(actor.native_position()));
    std::iter::once(eye).chain(remote).collect()
}

/// Yaw that turns a front-toward--Z model to face `eye` from `position`.
fn yaw_toward(eye: Vec3, position: [i32; 3]) -> f32 {
    let (dx, dz) = (
        eye.x - (position[0] as f32 + 0.5),
        eye.z - (position[2] as f32 + 0.5),
    );
    (-dx).atan2(-dz).to_degrees()
}

fn held_placement(
    item: &HeldItem,
    world_from_item: [[f32; 4]; 3],
    light: Option<(u8, u8)>,
) -> StaticItemPlacement {
    StaticItemPlacement {
        identifier: Arc::clone(&item.identifier),
        metadata: item.metadata,
        potted_block: None,
        world_from_item,
        light,
    }
}

/// Item offsets over the grill, in block-center pixels; provisional.
const CAMPFIRE_SLOTS: [[f32; 2]; 4] = [[-4.0, -4.0], [4.0, -4.0], [4.0, 4.0], [-4.0, 4.0]];
const CAMPFIRE_ITEM_HEIGHT: f32 = 7.5;
const CAMPFIRE_ITEM_SCALE: f32 = 0.375;

/// Seconds before an unanswered map request is repeated.
const MAP_REQUEST_RETRY_SECONDS: f64 = 5.0;

/// Asks for missing held and framed map images through the shared retry ledger.
pub(crate) fn request_missing_maps(
    mut runtime: ResMut<BlockEntityRuntime>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    time: Res<Time<Real>>,
    world: Res<ClientWorld>,
    player: Res<client_state::PlayerRuntime>,
    ui: Option<Res<UiRuntimeResource>>,
) {
    let Some(network) = network else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let runtime = &mut *runtime;
    if let Some(stream) = world.stream.as_ref() {
        let held = client_presentation::presentation::equipment::local_input(
            &player,
            stream,
            ui.as_deref().map(|ui| &ui.0),
            stream.local_player_runtime_id(),
        );
        for item in [held.main, held.off].into_iter().flatten() {
            if item.identifier.as_ref() == assets::facts::FILLED_MAP_IDENTIFIER
                && let Some(id) = item.map_id
                && stream.map_image(id).is_none()
            {
                runtime.missing_maps.push(id);
            }
        }
    }
    runtime.missing_maps.sort_unstable();
    runtime.missing_maps.dedup();
    for id in std::mem::take(&mut runtime.missing_maps) {
        let due = runtime
            .map_requests
            .get(&id)
            .is_none_or(|last| now - last >= MAP_REQUEST_RETRY_SECONDS);
        if due
            && network
                .send_inventory_packet(protocol::map_info_request_packet(id))
                .is_ok()
        {
            runtime.map_requests.insert(id, now);
        }
    }
    if runtime.map_requests.len() > 256 {
        runtime
            .map_requests
            .retain(|_, last| now - *last < MAP_REQUEST_RETRY_SECONDS);
    }
}

/// Cache key for a map image at one revision.
fn map_cache_key(map_id: i64, revision: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (map_id, revision).hash(&mut hasher);
    hasher.finish()
}

/// A 128x128 RGBA8 canvas from packed map pixels (red in the low byte).
fn map_canvas(pixels: &[u32]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|pixel| pixel.to_le_bytes())
        .collect()
}

/// Applies per-frame state (lid openness, sign canvases, viewer yaw) to a template; a `hidden`
/// one still advances its book, lid, ring and map request and keeps its text and map canvases
/// resident, but prepares no geometry.
fn resolve(
    template: &Template,
    position: [i32; 3],
    context: &FrameContext<'_>,
    runtime: &mut BlockEntityRuntime,
    scene: &mut BlockEntityScene,
    held: &mut Vec<StaticItemPlacement>,
    hidden: bool,
) -> Option<BlockEntityKind> {
    let stream = context.stream;
    let open_at = |at: [i32; 3]| {
        stream
            .block_event_cue(at)
            .is_some_and(|cue| cue_is_open(cue.event_type, cue.event_value))
    };
    match template {
        Template::Static(kind) => (!hidden).then(|| kind.clone()),
        Template::Beacon => None,
        &Template::Chest(mut model) => {
            let open = open_at(position)
                || matches!(model.pair, render_model::ChestPair::Lead { partner } if open_at(partner));
            if !matches!(model.pair, render_model::ChestPair::Follower) {
                model.lid = runtime.lids.advance(
                    position,
                    ContainerKind::Chest,
                    open,
                    context.delta_seconds,
                );
            }
            (!hidden).then_some(BlockEntityKind::Chest(model))
        }
        &Template::Shulker(mut model) => {
            model.open = runtime.lids.advance(
                position,
                ContainerKind::Shulker,
                open_at(position),
                context.delta_seconds,
            );
            (!hidden).then_some(BlockEntityKind::Shulker(model))
        }
        Template::EnchantTable => {
            let book = runtime.books.advance(position, context.readers);
            (!hidden).then_some(BlockEntityKind::EnchantTable { book })
        }
        &Template::Conduit { active, hunting } => (!hidden).then(|| {
            BlockEntityKind::Conduit(ConduitModel {
                active,
                hunting,
                viewer_yaw_degrees: yaw_toward(context.eye, position),
            })
        }),
        &Template::Bell {
            attachment,
            direction,
        } => {
            let sequence = stream
                .block_event_cue(position)
                .filter(|cue| cue.event_type == BELL_RING_EVENT_TYPE)
                .map(|cue| cue.sequence);
            let seconds_since_ring = bell_elapsed(
                &mut runtime.bell_rings,
                position,
                sequence,
                context.now_seconds,
            );
            (!hidden).then_some(BlockEntityKind::Bell(BellModel {
                attachment,
                direction,
                seconds_since_ring,
            }))
        }
        Template::ItemFrame {
            model,
            item,
            rotation_degrees,
            map_id,
        } => {
            let (mut model, rotation_degrees, map_id) = (*model, *rotation_degrees, *map_id);
            if let Some(id) = map_id
                && stream.map_image(id).is_none()
            {
                runtime.missing_maps.push(id);
            }
            let map = map_id.and_then(|id| {
                let image = stream.map_image(id)?;
                scene.map_rect(map_cache_key(id, image.revision), || {
                    map_canvas(&image.pixels)
                })
            });
            if hidden {
                return None;
            }
            model.map = map.map(|image| FramedMap {
                image,
                rotation_degrees,
            });
            if let (Some(item), None) = (item, map) {
                held.push(held_placement(
                    item,
                    item_frame_item_transform(position, model.outward, rotation_degrees),
                    model.glow.then_some((15, 15)),
                ));
            }
            Some(BlockEntityKind::ItemFrame(model))
        }
        Template::FlowerPot { plant } => {
            if hidden {
                return None;
            }
            let id = context.collisions.block_state_runtime_id(
                assets::NetworkIdMode::Sequential,
                assets::facts::BlockStateIdentity::Named(&plant.identifier),
                &plant.states,
            )?;
            held.push(StaticItemPlacement {
                identifier: Arc::clone(&plant.identifier),
                metadata: 0,
                potted_block: Some(id),
                world_from_item: render_model::dropped_item_transform(
                    position.map(|axis| axis as f32 + 0.5),
                    0.0,
                    1.0,
                ),
                light: None,
            });
            None
        }
        Template::Campfire { yaw_degrees, items } => {
            if hidden {
                return None;
            }
            let base = render_model::block_matrix(position, [0.5, 0.0, 0.5], *yaw_degrees);
            for (item, [x, z]) in items.iter().zip(CAMPFIRE_SLOTS) {
                let Some(item) = item else {
                    continue;
                };
                // Lie flat on the grill, sprite facing up.
                let pose = base
                    * Mat4::from_translation(Vec3::new(x, CAMPFIRE_ITEM_HEIGHT, z))
                    * Mat4::from_scale(Vec3::splat(CAMPFIRE_ITEM_SCALE * 16.0))
                    * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                held.push(held_placement(item, matrix_rows(pose), None));
            }
            None
        }
        Template::Sign { mount, front, back } => {
            let mut face = |spec: &Option<sign_text::SignTextSpec>| -> Option<SignFace> {
                let spec = spec.as_ref()?;
                let rect: AtlasRect = scene.text_rect(spec.cache_key(), || {
                    sign_text::rasterize(spec, context.font, &mut runtime.layouts)
                        .unwrap_or_default()
                })?;
                Some(SignFace {
                    rect,
                    glowing: spec.glowing(),
                })
            };
            let front = face(front);
            let back = face(back);
            (!hidden && (front.is_some() || back.is_some())).then_some(BlockEntityKind::Sign(
                SignModel {
                    mount: *mount,
                    front,
                    back,
                },
            ))
        }
    }
}

#[cfg(test)]
#[path = "system/resolve_tests.rs"]
mod resolve_tests;

#[cfg(test)]
#[path = "system/tests.rs"]
mod tests;
