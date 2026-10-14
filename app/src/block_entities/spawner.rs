//! Spawner contents use the actor evaluator, artwork pages and draw publication.
use crate::runtime::world::ClientWorld;
use bevy::prelude::{Mat4, Projection, Query, Res, ResMut, Resource, Transform, Vec3, With};
use client_presentation::{
    actor_publication::{ActorFrameState, PreparedActorPublication},
    camera::FlyCamera,
    local_player::LocalViewPose,
    presentation::{
        actors::{ActorPresentationBatch, PoseConversions, entity_rig_presentation_cached},
        entity_layers::{LayerPoseCache, apply_render_layers_cached},
    },
};
use client_world::DisplayActorStore;
use render_model::spawner::{SpawnerDisplayClock, spawner_actor_transform};
use render_model::{ActorArtworkPages, ActorCullView, matrix_rows};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Weak},
};
use world::{BlockEntityKey, BlockEntityNbt, ChunkKey, SUB_CHUNK_SIDE};

use super::system::{SCAN_RADIUS_BLOCKS as DISPLAY_SCAN_RADIUS, columns};
const DEFAULT_ACTIVATION_RANGE: f32 = 16.0;
const DEFAULT_SPAWN_DELAY: i32 = 0;

type EntityIndex = std::collections::BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>;
struct Column {
    source: Weak<EntityIndex>,
    displays: Vec<Request>,
}
impl Column {
    /// Whether the column still owns the index used by this scan.
    fn is_current(&self, source: &Arc<EntityIndex>) -> bool {
        self.source
            .upgrade()
            .is_some_and(|previous| Arc::ptr_eq(&previous, source))
    }
}
#[derive(Clone)]
struct Request {
    position: [i32; 3],
    identifier: Arc<str>,
    delay: i32,
    activation_range: f32,
    nbt: Arc<BlockEntityNbt>,
    /// A rejection remains valid while this snapshot and the active pack are unchanged.
    rigless: bool,
}
struct Clock {
    source: Arc<BlockEntityNbt>,
    spin: SpawnerDisplayClock,
}
struct Pool {
    source: Arc<assets::RuntimeEntityAssets>,
    session: (u64, i32),
    pack: Option<Arc<assets::SessionEntityPack>>,
    actors: DisplayActorStore,
    columns: HashMap<ChunkKey, Column>,
    rescans: columns::RescanOrder,
    spins: HashMap<[i32; 3], Clock>,
    poses: PoseConversions,
    layers: LayerPoseCache,
}

#[derive(Resource, Default)]
pub(crate) struct SpawnerDisplayPublisher(Option<Pool>);

#[cfg(feature = "developer-control")]
impl SpawnerDisplayPublisher {
    /// Returns the display actor published under this lifetime, or `None` for another namespace.
    pub(crate) fn actor(
        &self,
        session_id: u64,
        runtime_id: u64,
    ) -> Option<&client_world::ActorSnapshot> {
        let pool = self.0.as_ref()?;
        (pool.actors.session_id() == session_id)
            .then(|| pool.actors.actor(runtime_id))
            .flatten()
    }
}

/// Parses the spawners of a column NBT index, reusing requests whose snapshot is unchanged.
fn column(source: Arc<EntityIndex>, previous: Option<&Column>) -> Column {
    let previous = previous.map_or(&[][..], |column| column.displays.as_slice());
    let displays = source
        .iter()
        .filter(|(_, nbt)| {
            nbt.id()
                .and_then(|id| assets::facts::BlockEntityFacts::pinned().entry(id))
                .is_some_and(|entry| {
                    entry.renderer == assets::facts::BlockEntityRenderer::MobSpawner
                })
        })
        .filter_map(|(key, nbt)| {
            let position = key.position();
            // Requests follow the index's key order, so the previous column is sorted by position.
            if let Ok(index) = previous.binary_search_by(|request| request.position.cmp(&position))
                && Arc::ptr_eq(&previous[index].nbt, nbt)
            {
                return Some(previous[index].clone());
            }
            let root = nbt.parse()?;
            let identifier = root.string("EntityIdentifier")?;
            if !DisplayActorStore::admits_identifier(identifier) {
                return None;
            }
            Some(Request {
                position,
                identifier: Arc::from(identifier),
                delay: root
                    .integer("Delay")
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or(DEFAULT_SPAWN_DELAY)
                    .max(-1),
                activation_range: root
                    .integer("RequiredPlayerRange")
                    .map_or(DEFAULT_ACTIVATION_RANGE, |value| value.clamp(0, 128) as f32),
                nbt: Arc::clone(nbt),
                rigless: false,
            })
        })
        .collect();
    Column {
        source: Arc::downgrade(&source),
        displays,
    }
}

/// Publishes client-owned display actors into this frame's normal actor draw batch.
pub(crate) fn prepare_spawner_actors(
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    (artwork, state): (
        Res<render_scene::ActorArtworkResource>,
        Res<ActorFrameState>,
    ),
    mut pending: ResMut<PreparedActorPublication>,
    mut publisher: ResMut<SpawnerDisplayPublisher>,
    camera: Query<(&Transform, &Projection), With<FlyCamera>>,
) {
    let (Some(stream), Some(assets)) = (world.stream.as_ref(), world.entity_assets.as_ref()) else {
        publisher.0 = None;
        return;
    };
    let session = (
        stream.authority().actor_session_id(),
        stream.current_dimension(),
    );
    if publisher
        .0
        .as_ref()
        .is_none_or(|pool| pool.session != session || !Arc::ptr_eq(&pool.source, assets))
    {
        publisher.0 = Some(Pool {
            source: Arc::clone(assets),
            session,
            pack: None,
            actors: DisplayActorStore::new(session.1, Arc::clone(assets)),
            columns: Default::default(),
            rescans: Default::default(),
            spins: Default::default(),
            poses: Default::default(),
            layers: Default::default(),
        });
    }
    let pool = publisher.0.as_mut().unwrap();
    let pack_changed = match (&pool.pack, &world.pack_entities) {
        (None, None) => false,
        (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
        _ => true,
    };
    if pack_changed {
        pool.actors
            .set_pack(world.pack_entities.as_ref().map(|pack| {
                (
                    Arc::clone(&pack.assets),
                    pack.bindings
                        .iter()
                        .map(|binding| binding.geometry_candidate)
                        .collect(),
                )
            }));
        pool.pack = world.pack_entities.clone();
        for column in pool.columns.values_mut() {
            for request in &mut column.displays {
                request.rigless = false;
            }
        }
        pool.poses = Default::default();
        pool.layers = Default::default();
    }
    let Some(step) = state.frame_step() else {
        return;
    };
    if let Some((rotation, position)) = state.sampling_camera() {
        pool.actors.set_camera(rotation, position);
    }
    let eye = view.eye_translation();
    let authority = stream.authority();
    let local = authority.actor(authority.local_player_runtime_id());
    let mut players: Vec<Vec3> = authority
        .present_remote_players()
        .filter_map(|actor| {
            actor
                .interpolated_position(step.partial_tick)
                .map(Vec3::from_array)
        })
        .collect();
    if local.is_some_and(|actor| authority.camera_aim_assist_eligible(actor) == Some(true)) {
        players.push(view.feet_translation());
    }
    let range = |center: f32| {
        ((center - DISPLAY_SCAN_RADIUS) / SUB_CHUNK_SIDE as f32).floor() as i32
            ..=((center + DISPLAY_SCAN_RADIUS) / SUB_CHUNK_SIDE as f32).floor() as i32
    };
    let side = SUB_CHUNK_SIDE as i32;
    let eye_column = [eye.x, eye.z].map(|axis| (axis / SUB_CHUNK_SIDE as f32).floor() as i32);
    let store = stream.collision_store();
    let scans = &pool.columns;
    let selected = pool.rescans.select(
        range(eye.x)
            .flat_map(|x| range(eye.z).map(move |z| ChunkKey::new(session.1, x, z)))
            .filter(|key| {
                !columns::is_near_column(eye_column, key.x, key.z)
                    && store.chunk(*key).is_some_and(|chunk| {
                        scans
                            .get(key)
                            .is_none_or(|scan| !scan.is_current(&chunk.shared_block_entities()))
                    })
            }),
    );
    let mut rescans_left = columns::MAX_COLUMN_RESCANS_PER_FRAME;
    let mut columns = BTreeSet::new();
    let mut candidates = Vec::new();
    for x in range(eye.x) {
        for z in range(eye.z) {
            let key = ChunkKey::new(session.1, x, z);
            let Some(chunk) = store.chunk(key) else {
                continue;
            };
            columns.insert(key);
            let near = columns::is_near_column(eye_column, x, z);
            let mut deferred = 0;
            let budget = if near || selected.contains(&Some(key)) {
                &mut rescans_left
            } else {
                &mut deferred
            };
            let Some(scan) = columns::frame_scan(
                pool.columns.remove(&key),
                chunk,
                near,
                budget,
                |scan, chunk| scan.is_current(&chunk.shared_block_entities()),
                |previous| column(chunk.shared_block_entities(), previous.as_ref()),
            ) else {
                continue;
            };
            pool.columns.insert(key, scan);
            for (index, request) in pool.columns[&key].displays.iter().enumerate() {
                let [x, y, z] = request.position;
                let backing = chunk
                    .sub_chunk(y.div_euclid(side))
                    .and_then(|part| {
                        part.runtime_id(
                            0,
                            x.rem_euclid(side) as u8,
                            y.rem_euclid(side) as u8,
                            z.rem_euclid(side) as u8,
                        )
                    })
                    .and_then(|id| {
                        assets::facts::BlockEntityFacts::pinned().for_runtime(
                            id,
                            stream.network_id_mode(),
                            stream.runtime_assets(),
                        )
                    });
                if !backing.is_some_and(|fact| {
                    fact.entity.renderer == assets::facts::BlockEntityRenderer::MobSpawner
                }) {
                    continue;
                }
                if display_center(request.position).distance_squared(eye)
                    > DISPLAY_SCAN_RADIUS * DISPLAY_SCAN_RADIUS
                {
                    continue;
                }
                candidates.push((key, index));
            }
        }
    }
    pool.columns.retain(|key, _| columns.contains(key));
    let view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: DISPLAY_SCAN_RADIUS * 2.0,
        });
    // Stable, so spawners in view keep their scan order ahead of those outside it.
    candidates.sort_by_cached_key(|&(key, index)| {
        !block_in_view(view, pool.columns[&key].displays[index].position)
    });
    // Retire every lifetime outside the wanted set first so in-view newcomers find free slots.
    let wanted = candidates
        .iter()
        .map(|&(key, index)| &pool.columns[&key].displays[index])
        .filter(|request| !request.rigless)
        .take(client_world::MAX_DISPLAY_ACTORS)
        .map(|request| request.position)
        .collect();
    pool.actors.retain(&wanted);
    let mut visible = BTreeSet::new();
    let mut displays = Vec::new();
    for (key, index) in candidates {
        if displays.len() >= client_world::MAX_DISPLAY_ACTORS {
            break;
        }
        let request = &mut pool.columns.get_mut(&key).unwrap().displays[index];
        if request.rigless {
            continue;
        }
        // Only admitted displays count; rejected or unrigged entities leave the budget free.
        let Some(runtime) = pool.actors.resolve(request.position, &request.identifier) else {
            request.rigless = true;
            continue;
        };
        visible.insert(request.position);
        let clock = pool.spins.entry(request.position).or_insert_with(|| Clock {
            source: Arc::clone(&request.nbt),
            spin: SpawnerDisplayClock {
                delay: request.delay,
                ..Default::default()
            },
        });
        if !Arc::ptr_eq(&clock.source, &request.nbt) {
            clock.source = Arc::clone(&request.nbt);
            clock.spin.delay = request.delay;
        }
        let center = display_center(request.position);
        clock.spin.advance(
            step.ticks,
            players.iter().any(|player| {
                center.distance_squared(*player)
                    <= request.activation_range * request.activation_range
            }),
        );
        displays.push((runtime, request.position, clock.spin));
    }
    pool.spins.retain(|position, _| visible.contains(position));
    pool.actors.retain(&visible);
    pool.actors.advance(step.ticks);
    let artwork = state.actor_artwork(&artwork.0);
    pending.extend_entities(display_batch(
        pool,
        stream,
        artwork,
        step.partial_tick,
        displays,
    ));
}

/// Whether any part of the spawner block at `position` lies in the view; fails open without one.
fn block_in_view(view: Option<ActorCullView>, position: [i32; 3]) -> bool {
    let [x, y, z] = position.map(|axis| axis as f32);
    render_model::actor_bounds_are_visible(
        [x + 0.5, y, z + 0.5],
        1.0,
        assets::SkinGeometryBounds {
            center: [0.0, 0.5, 0.0],
            half_extents: [0.5; 3],
        },
        view,
    )
}

/// Returns the centre of the spawner block at `position`.
fn display_center(position: [i32; 3]) -> Vec3 {
    Vec3::from_array(position.map(|value| value as f32)) + Vec3::splat(0.5)
}

/// Converts and places display poses, then resolves their lighting and controller layers.
fn display_batch(
    pool: &mut Pool,
    stream: &chunk_pipeline::WorldStream,
    artwork: &ActorArtworkPages,
    partial_tick: f32,
    displays: Vec<(u64, [i32; 3], SpawnerDisplayClock)>,
) -> ActorPresentationBatch {
    let mut batch = ActorPresentationBatch {
        submissions: Vec::new(),
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    pool.poses.begin_frame();
    let mut sampled = pool.actors.frame(partial_tick);
    for (runtime, position, spin) in displays {
        let (Some(actor), Some(rig)) = (pool.actors.actor(runtime), pool.actors.rig(runtime))
        else {
            continue;
        };
        let rig = sampled.sample_rig_scale(rig);
        let Some(mut presentation) = entity_rig_presentation_cached(
            &rig,
            actor,
            artwork,
            partial_tick,
            Some(&mut pool.poses),
        ) else {
            continue;
        };
        let dimensions = presentation
            .submission
            .culling_bounds
            .half_extents
            .map(|axis| axis * 2.0);
        let dimensions = std::array::from_fn(|axis| {
            dimensions[axis] * rig.scale * actor.render_scale() * rig.axis_scale[axis].abs()
        });
        let Some(placement) = spawner_actor_transform(position, dimensions, spin, partial_tick)
        else {
            continue;
        };
        let body = &mut presentation.submission;
        let [x, y, z] = body.world_from_actor;
        let normal = Mat4::from_cols_array(&[
            x[0], y[0], z[0], 0.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
        body.world_from_actor = matrix_rows(placement * normal);
        if let Some(location) = presentation.artwork {
            batch.artwork.insert(body.input.identity, location);
        }
        batch.submissions.push(presentation.submission);
    }
    for body in &mut batch.submissions {
        let feet = body.world_from_actor.map(|row| row[3]);
        let position = pool
            .actors
            .actor(body.input.identity.runtime_id)
            .map_or(feet, |actor| actor.brightness_sample_position(feet));
        if let Some((block, sky)) = stream.solved_light_at(position) {
            body.light = render_model::pack_actor_light(block, sky);
        }
    }
    pool.layers.begin_frame();
    apply_render_layers_cached(
        &mut batch,
        |runtime| sampled.layers(runtime),
        artwork,
        &mut pool.layers,
    );
    batch
}

#[cfg(test)]
mod tests;
