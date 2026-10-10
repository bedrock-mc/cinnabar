//! The per-frame system, carrier loading and the caches behind them.

use std::{collections::HashMap, path::Path, sync::Arc};

use assets::{RuntimeBlockEntityAssets, RuntimeFontCatalog};
use bevy::prelude::{
    App, IntoScheduleConfigs, Mat4, Projection, Query, Real, Res, ResMut, Resource, Time,
    Transform, Update, Vec3, With,
};
use render::{
    AtlasRect, AtmosphereFrame, BeaconModel, BellModel, BlockEntityFrame, BlockEntityKind,
    BlockEntityLight, BlockEntityScene, BlockEntitySubmission, ConduitModel, SceneClock, SignFace,
    SignModel, StaticItemPlacement, StaticItemPlacements, item_frame_item_transform, matrix_rows,
};
use ui::TextLayoutCache;
use world::{ChunkKey, SUB_CHUNK_SIDE};

use super::{
    containers::{ContainerKind, ContainerLids, cue_is_open},
    cracks::{CachedCrackShape, crack_instances, crack_shape},
    describe::{HeldItem, Template, describe},
    sign_text,
    state::BlockState,
};
use client_ui::ui_runtime::UiRuntime;
use {
    crate::{movement::PhysicsCollisionRegistries, runtime::world::ClientWorld},
    client_presentation::{actor_publication::ActorFramePartialTick, local_player::LocalViewPose},
};

const BLOCK_ENTITY_ASSETS_FILENAME: &str = assets::carriers::BLOCK_ENTITY.output;
/// Block entities farther than this from the eye are not drawn.
const SCAN_RADIUS_BLOCKS: f32 = 64.0;
const MAX_SUBMISSIONS: usize = 4_096;
const TICKS_PER_SECOND: f64 = 20.0;
const TEXT_CACHE_ENTRIES: usize = 256;
const TEXT_CACHE_BYTES: usize = 2 * 1024 * 1024;

mod bed;
mod columns;
mod crystal_beams;
mod dragon_death;
mod portals;

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
            eprintln!(
                "block-entity carrier {} unavailable ({error}); block-entity models, sign text, break cracks and worn heads are not drawn; rebuild with: make block-entity-assets",
                path.display()
            );
            return None;
        }
    };
    match RuntimeBlockEntityAssets::decode(&bytes) {
        Ok(assets) => {
            eprintln!(
                "loaded block-entity carrier from {} ({} textures, atlas {:?})",
                path.display(),
                assets.placements().len(),
                assets.atlas_size()
            );
            Some(Arc::new(assets))
        }
        Err(error) => {
            eprintln!(
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
}

#[derive(Resource)]
pub(crate) struct BlockEntityRuntime {
    lids: ContainerLids,
    /// Scan results per loaded column, rebuilt when the column changes.
    columns: HashMap<ChunkKey, columns::ColumnScan>,
    rescans: columns::RescanOrder,
    frame: u64,
    blocks: HashMap<u32, Option<Arc<BlockInfo>>>,
    layouts: TextLayoutCache,
    shapes: HashMap<(u32, u32), CachedCrackShape>,
    bell_rings: HashMap<[i32; 3], (u64, f64)>,
    /// Framed map ids seen this frame without an image.
    missing_maps: Vec<i64>,
    /// When each map id was last requested from the server, in real seconds.
    map_requests: HashMap<i64, f64>,
    /// Session and dimension the runtime-id keyed caches were filled from.
    session: Option<(u64, i32)>,
}

impl BlockEntityRuntime {
    pub(crate) fn new() -> Self {
        Self {
            lids: ContainerLids::default(),
            columns: HashMap::new(),
            rescans: columns::RescanOrder::default(),
            frame: 0,
            blocks: HashMap::new(),
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            shapes: HashMap::new(),
            bell_rings: HashMap::new(),
            missing_maps: Vec::new(),
            map_requests: HashMap::new(),
            session: None,
        }
    }

    /// Runtime ids, block states and positions mean nothing across sessions, so a
    /// session or dimension change drops every cache keyed by them.
    fn bind_session(&mut self, session: Option<(u64, i32)>) {
        if self.session == session {
            return;
        }
        self.session = session;
        self.lids = ContainerLids::default();
        self.missing_maps.clear();
        self.columns.clear();
        self.rescans = columns::RescanOrder::default();
        self.blocks.clear();
        self.shapes.clear();
        self.bell_rings.clear();
        self.map_requests.clear();
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
    // through mob_head's ordinary entity material.
    if matches!(kind, BlockEntityKind::Skull(_)) {
        BlockEntityLight::Actor { block, sky }
    } else {
        light_factor(block, sky, daylight).into()
    }
}

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
            }))
        })
        .clone()
}

/// The portal surface `runtime_id` draws, if any.
fn portal_kind(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> Option<BlockEntityKind> {
    let info = block_info(runtime, collisions, mode, runtime_id)?;
    match info.name.as_ref() {
        assets::END_PORTAL_IDENTIFIER => Some(BlockEntityKind::EndPortal),
        assets::END_GATEWAY_IDENTIFIER => Some(BlockEntityKind::EndGateway),
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
    ui: Res<UiRuntime>,
    atmosphere: Res<AtmosphereFrame>,
    time: Res<Time<Real>>,
    font: Res<BlockEntityFont>,
    mut runtime: ResMut<BlockEntityRuntime>,
    mut scene: ResMut<BlockEntityScene>,
    mut frame: ResMut<BlockEntityFrame>,
    mut placements: ResMut<StaticItemPlacements>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    if !scene.has_assets() {
        dragon_death::update_without_atlas(
            client_world.stream.as_ref(),
            actor_partial_tick.0,
            camera
                .single()
                .ok()
                .map(|(transform, _)| transform.translation),
            &mut scene,
            &mut frame,
        );
        return;
    }
    // Timed in the body: a span around the chain would also count waiting on actor publication.
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::BlockEntities));
    let now_seconds = time.elapsed_secs_f64();
    let clock = SceneClock {
        ticks: now_seconds * TICKS_PER_SECOND,
    };
    let Some(stream) = client_world.stream.as_ref() else {
        runtime.bind_session(None);
        placements.0.clear();
        *frame = scene.update(clock, &[], &[]).clone();
        return;
    };
    let runtime = &mut *runtime;
    runtime.bind_session(Some((
        stream.authority().actor_session_id(),
        stream.current_dimension(),
    )));
    runtime.missing_maps.clear();
    let dimension = stream.current_dimension();
    let store = stream.collision_store();
    let mode = stream.network_id_mode();
    let eye = view.eye_translation();
    let delta = time.delta_secs();
    let daylight = atmosphere.daylight();

    let cracks = ui
        .block_crack_snapshot()
        .filter(|snapshot| snapshot.dimension == dimension)
        .map_or_else(Vec::new, |snapshot| {
            let assets = stream.runtime_assets();
            let shapes = &mut runtime.shapes;
            crack_instances(&snapshot.entries, |entry| {
                crack_shape(
                    shapes,
                    assets,
                    mode,
                    entry.layers.iter().flatten().next().copied(),
                    entry.position,
                )
            })
        });

    let mut submissions: Vec<BlockEntitySubmission> = Vec::new();
    runtime.frame = runtime.frame.wrapping_add(1);
    let frame_stamp = runtime.frame;
    // Moved out so `resolve` can borrow a template while mutating the rest of the runtime.
    let mut scans = std::mem::take(&mut runtime.columns);
    let mut held: Vec<StaticItemPlacement> = Vec::new();
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
            let Some(mut scan) = columns::frame_scan(previous, chunk, near, budget, |previous| {
                let portals = portals::column_cells(chunk_key, chunk, |id| {
                    portal_kind(runtime, &collisions, mode, id)
                });
                let entities =
                    columns::routed_entities(chunk, previous, |id, runtime_id, nbt, position| {
                        block_info(runtime, &collisions, mode, runtime_id)
                            .zip(nbt.parse())
                            .and_then(|(info, root)| {
                                describe(id, &info.name, &info.state, &root, position)
                            })
                    });
                columns::ColumnScan::new(chunk, portals, entities)
            }) else {
                continue;
            };
            scan.seen_frame = frame_stamp;
            portals::submit(&mut submissions, &scan.portals, eye);
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
                let (block_light, sky_light) = stream.light_level_at(center.to_array());
                let context = FrameContext {
                    stream,
                    eye,
                    delta_seconds: delta,
                    now_seconds,
                    font: &font.0,
                };
                let kind = if matches!(template, Template::Beacon) {
                    beacon_kind(
                        &mut *runtime,
                        &collisions,
                        store,
                        dimension,
                        mode,
                        [x, y, z],
                    )
                } else {
                    resolve(
                        template,
                        [x, y, z],
                        &context,
                        runtime,
                        &mut scene,
                        &mut held,
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
    runtime.lids.finish();
    scans.retain(|_, scan| scan.seen_frame == frame_stamp);
    runtime.columns = scans;
    prune_bell_rings(&mut runtime.bell_rings, now_seconds, |position| {
        stream
            .block_event_cue(*position)
            .filter(|cue| cue.event_type == BELL_RING_EVENT_TYPE)
            .map(|cue| cue.sequence)
    });
    crystal_beams::submit(
        &mut submissions,
        stream.authority().crystal_beams(actor_partial_tick.0),
        camera
            .single()
            .ok()
            .map(|(transform, _)| transform.translation),
        |runtime_id, owner_position| {
            let actor = stream.authority().actor(runtime_id)?;
            stream.solved_light_at(actor.brightness_sample_position(owner_position))
        },
    );
    dragon_death::submit(
        &mut submissions,
        stream.authority().dragon_death_rays(actor_partial_tick.0),
        camera
            .single()
            .ok()
            .map(|(transform, _)| transform.translation),
        |runtime_id, owner_position| {
            let actor = stream.authority().actor(runtime_id)?;
            stream.solved_light_at(actor.brightness_sample_position(owner_position))
        },
    );
    placements.0 = held;
    *frame = scene.update(clock, &cracks, &submissions).clone();
}

struct FrameContext<'a> {
    stream: &'a chunk_pipeline::WorldStream,
    eye: Vec3,
    delta_seconds: f32,
    now_seconds: f64,
    font: &'a RuntimeFontCatalog,
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
        world_from_item,
        light,
    }
}

/// Item offsets over the grill, in block-center pixels; provisional.
const CAMPFIRE_SLOTS: [[f32; 2]; 4] = [[-4.0, -4.0], [4.0, -4.0], [4.0, 4.0], [-4.0, 4.0]];
const CAMPFIRE_ITEM_HEIGHT: f32 = 7.5;
const CAMPFIRE_ITEM_SCALE: f32 = 0.375;
const FLOWER_SCALE: f32 = 0.5;

/// Seconds before an unanswered map request is repeated.
const MAP_REQUEST_RETRY_SECONDS: f64 = 5.0;

/// Asks the server for the pixels of framed maps whose images have not arrived.
pub(crate) fn request_missing_maps(
    mut runtime: ResMut<BlockEntityRuntime>,
    network: Option<Res<crate::runtime::network::NetworkHandle>>,
    time: Res<Time<Real>>,
) {
    let Some(network) = network else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let runtime = &mut *runtime;
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

/// Applies per-frame state (lid openness, sign canvases, viewer yaw) to a template.
fn resolve(
    template: &Template,
    position: [i32; 3],
    context: &FrameContext<'_>,
    runtime: &mut BlockEntityRuntime,
    scene: &mut BlockEntityScene,
    held: &mut Vec<StaticItemPlacement>,
) -> Option<BlockEntityKind> {
    let stream = context.stream;
    let open_at = |at: [i32; 3]| {
        stream
            .block_event_cue(at)
            .is_some_and(|cue| cue_is_open(cue.event_type, cue.event_value))
    };
    match template {
        Template::Static(kind) => Some(kind.clone()),
        Template::Beacon => None,
        &Template::Chest(mut model) => {
            let open = open_at(position)
                || matches!(model.pair, render::ChestPair::Lead { partner } if open_at(partner));
            if !matches!(model.pair, render::ChestPair::Follower) {
                model.lid = runtime.lids.advance(
                    position,
                    ContainerKind::Chest,
                    open,
                    context.delta_seconds,
                );
            }
            Some(BlockEntityKind::Chest(model))
        }
        &Template::Shulker(mut model) => {
            model.open = runtime.lids.advance(
                position,
                ContainerKind::Shulker,
                open_at(position),
                context.delta_seconds,
            );
            Some(BlockEntityKind::Shulker(model))
        }
        Template::EnchantTable => Some(BlockEntityKind::EnchantTable {
            facing_yaw_degrees: yaw_toward(context.eye, position),
        }),
        &Template::Conduit { active, hunting } => Some(BlockEntityKind::Conduit(ConduitModel {
            active,
            hunting,
            viewer_yaw_degrees: yaw_toward(context.eye, position),
        })),
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
            Some(BlockEntityKind::Bell(BellModel {
                attachment,
                direction,
                seconds_since_ring,
            }))
        }
        Template::ItemFrame {
            model,
            item,
            rotation_steps,
            map_id,
        } => {
            let (mut model, rotation_steps, map_id) = (*model, *rotation_steps, *map_id);
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
            model.map = map;
            if let (Some(item), None) = (item, map) {
                held.push(held_placement(
                    item,
                    item_frame_item_transform(position, model.outward, rotation_steps),
                    model.glow.then_some((15, 15)),
                ));
            }
            Some(BlockEntityKind::ItemFrame(model))
        }
        Template::FlowerPot { plant } => {
            // Two crossed sprites standing in the pot; the plant icon stands in for the model.
            let base = render::block_matrix(position, [0.5, 0.32, 0.5], 0.0);
            for yaw in [45.0_f32, 135.0] {
                let pose = base
                    * Mat4::from_rotation_y(yaw.to_radians())
                    * Mat4::from_scale(Vec3::splat(FLOWER_SCALE * 16.0));
                held.push(held_placement(plant, matrix_rows(pose), None));
            }
            None
        }
        Template::Campfire { yaw_degrees, items } => {
            let base = render::block_matrix(position, [0.5, 0.0, 0.5], *yaw_degrees);
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
                    glowing: spec.glowing,
                })
            };
            let front = face(front);
            let back = face(back);
            (front.is_some() || back.is_some()).then_some(BlockEntityKind::Sign(SignModel {
                mount: *mount,
                front,
                back,
            }))
        }
    }
}

/// The `BlockEventPacket` type a bell ring arrives as.
/// Starts each newly observed ring once and reports its elapsed animation time.
fn bell_elapsed(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    position: [i32; 3],
    sequence: Option<u64>,
    now: f64,
) -> f32 {
    if let Some(sequence) = sequence {
        let entry = rings.entry(position).or_insert((sequence, now));
        if entry.0 != sequence {
            *entry = (sequence, now);
        }
    }
    rings
        .get(&position)
        .map_or(f32::INFINITY, |(_, start)| (now - start) as f32)
}

const BELL_RING_EVENT_TYPE: i32 = 1;
/// Seconds after a ring when the swing has visibly settled and its state can go.
const BELL_RING_RETAIN_SECONDS: f64 = 10.0;

/// Keeps a latched cue's sequence so an expired swing cannot restart; evicted cues age out.
fn prune_bell_rings(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    now_seconds: f64,
    mut cue: impl FnMut(&[i32; 3]) -> Option<u64>,
) {
    rings.retain(|position, (sequence, start)| {
        cue(position) == Some(*sequence) || now_seconds - *start < BELL_RING_RETAIN_SECONDS
    });
}

/// Highest world Y a beacon beam is drawn to; the beam stops at the build limit.
const BEAM_TOP: i32 = 320;

/// The dye color of a stained-glass block name, as linear RGB.
fn glass_tint(name: &str) -> Option<[f32; 3]> {
    let color = name
        .strip_prefix("minecraft:")?
        .strip_suffix("_stained_glass_pane")
        .or_else(|| {
            name.strip_prefix("minecraft:")?
                .strip_suffix("_stained_glass")
        })?;
    let java_id = match color {
        "white" => 0,
        "orange" => 1,
        "magenta" => 2,
        "light_blue" => 3,
        "yellow" => 4,
        "lime" => 5,
        "pink" => 6,
        "gray" => 7,
        "light_gray" | "silver" => 8,
        "cyan" => 9,
        "purple" => 10,
        "blue" => 11,
        "brown" => 12,
        "green" => 13,
        "red" => 14,
        "black" => 15,
        _ => return None,
    };
    Some(assets::banner::color_linear(15 - java_id))
}

/// Blends the tint of each stained-glass block above the beacon, each new pane averaging
/// with the color so far.
fn beam_tint(glass: impl IntoIterator<Item = [f32; 3]>) -> [f32; 3] {
    glass.into_iter().fold([1.0; 3], |tint, color| {
        if tint == [1.0; 3] {
            color
        } else {
            std::array::from_fn(|axis| (tint[axis] + color[axis]) * 0.5)
        }
    })
}

fn beacon_kind(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    store: &world::ChunkStore,
    dimension: i32,
    mode: assets::NetworkIdMode,
    position: [i32; 3],
) -> Option<BlockEntityKind> {
    let height = u32::try_from(BEAM_TOP - position[1] - 1)
        .ok()
        .filter(|height| *height > 0)?;
    let mut glass = Vec::new();
    for y in position[1] + 1..BEAM_TOP {
        let runtime_id = store
            .sub_chunk(world::SubChunkKey::new(
                dimension,
                position[0].div_euclid(16),
                y.div_euclid(16),
                position[2].div_euclid(16),
            ))
            .and_then(|sub_chunk| {
                sub_chunk.runtime_id(
                    0,
                    position[0].rem_euclid(16) as u8,
                    y.rem_euclid(16) as u8,
                    position[2].rem_euclid(16) as u8,
                )
            });
        if let Some(info) = runtime_id.and_then(|id| block_info(runtime, collisions, mode, id))
            && let Some(color) = glass_tint(&info.name)
        {
            glass.push(color);
        }
    }
    Some(BlockEntityKind::Beacon(BeaconModel {
        height,
        tint: beam_tint(glass),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_first_bell_cue_starts_once() {
        let mut rings = HashMap::new();
        assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(7), 100.0), 0.0);
        prune_bell_rings(&mut rings, 120.0, |_| Some(7));
        assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(7), 120.0), 20.0);
        assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(8), 121.0), 0.0);
    }

    #[test]
    fn review_session_changes_reset_container_animation() {
        let mut runtime = BlockEntityRuntime::new();
        runtime.bind_session(Some((1, 0)));
        runtime
            .lids
            .advance([0; 3], ContainerKind::Chest, true, 1.0);
        runtime.bind_session(Some((2, 0)));
        assert_eq!(
            runtime
                .lids
                .advance([0; 3], ContainerKind::Chest, false, 0.05),
            0.0
        );
    }

    #[test]
    fn light_follows_the_terrain_curve_and_night_transfer_floor() {
        assert!((light_factor(15, 0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!((light_factor(0, 15, 1.0) - 1.0).abs() < 1.0e-6);
        // Full sky light at night is throttled to the transfer floor.
        assert!((light_factor(0, 15, 0.0) - NIGHT_SKY_TRANSFER_FLOOR).abs() < 1.0e-6);
        assert_eq!(light_factor(0, 0, 1.0), 0.0);
        assert!(light_factor(8, 0, 1.0) > light_factor(4, 0, 1.0));
    }

    #[test]
    fn placed_player_skulls_keep_native_light_coordinates_for_the_environment_shader() {
        let kind = BlockEntityKind::Skull(render::SkullModel {
            kind: render::SkullKind::Player,
            mount: render::SkullMount::Floor {
                rotation_degrees: 0.0,
            },
        });
        for (block, sky) in [(0, 0), (0, 15), (12, 0)] {
            for daylight in [0.0, 1.0] {
                assert_eq!(
                    model_light(&kind, block, sky, daylight),
                    BlockEntityLight::Actor { block, sky }
                );
            }
        }
        assert_eq!(
            model_light(&BlockEntityKind::EndPortal, 0, 15, 1.0),
            1.0.into()
        );
    }

    #[test]
    fn beam_tint_averages_each_new_pane_with_the_color_so_far() {
        assert_eq!(beam_tint([]), [1.0; 3]);
        let red = [1.0, 0.0, 0.0];
        let blue = [0.0, 0.0, 1.0];
        assert_eq!(beam_tint([red]), red);
        assert_eq!(beam_tint([red, blue]), [0.5, 0.0, 0.5]);
        assert!(glass_tint("minecraft:red_stained_glass").is_some());
        assert!(glass_tint("minecraft:silver_stained_glass_pane").is_some());
        assert!(glass_tint("minecraft:glass").is_none());
    }

    #[test]
    fn the_viewer_yaw_turns_the_model_front_toward_the_eye() {
        // An eye due north of the block needs no turn; due east needs a quarter turn.
        assert!(yaw_toward(Vec3::new(0.5, 0.0, -5.0), [0, 0, 0]).abs() < 1.0e-3);
        let east = yaw_toward(Vec3::new(5.5, 0.0, 0.5), [0, 0, 0]);
        let front = Mat4::from_rotation_y(east.to_radians()).transform_vector3(Vec3::NEG_Z);
        assert!(front.abs_diff_eq(Vec3::X, 1.0e-4));
    }

    #[test]
    fn map_pixels_unpack_red_first_and_keys_track_revisions() {
        assert_eq!(map_canvas(&[0x4433_2211]), vec![0x11, 0x22, 0x33, 0x44]);
        assert_ne!(map_cache_key(1, 1), map_cache_key(1, 2));
        assert_ne!(map_cache_key(1, 1), map_cache_key(2, 1));
        assert_eq!(map_cache_key(5, 9), map_cache_key(5, 9));
    }

    /// Bell state must not accumulate for every bell ever seen.
    #[test]
    fn bell_ring_state_expires_once_the_swing_settles() {
        let mut rings = HashMap::from([
            ([0, 0, 0], (1, 100.0)),
            ([1, 0, 0], (2, f64::NEG_INFINITY)),
            ([2, 0, 0], (3, 80.0)),
        ]);
        prune_bell_rings(&mut rings, 101.0, |_| None);
        assert_eq!(rings.keys().copied().collect::<Vec<_>>(), vec![[0, 0, 0]]);
    }

    /// A runtime id cached in one session must not resolve blocks in the next.
    #[test]
    fn session_change_drops_runtime_id_keyed_caches() {
        let mut runtime = BlockEntityRuntime::new();
        runtime.bind_session(Some((1, 0)));
        runtime.blocks.insert(7, None);
        runtime.shapes.insert(
            (7, 0),
            CachedCrackShape {
                column: None,
                shape: render::CrackShape::Cube,
            },
        );
        runtime.bell_rings.insert([0, 0, 0], (1, 0.0));
        runtime.bind_session(Some((1, 0)));
        assert_eq!(runtime.blocks.len(), 1);
        runtime.bind_session(Some((2, 0)));
        assert!(runtime.blocks.is_empty() && runtime.shapes.is_empty());
        assert!(runtime.bell_rings.is_empty());
    }
}
