use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use bevy::{
    log::{debug, warn},
    prelude::{Res, ResMut, Resource},
};
use client_world::{ActorLifetimeId, ActorPose, CommittedActorMove};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionWorld, PLAYER_HORIZONTAL_EPSILON, PLAYER_WIDTH, Vec3, WorldCollisionIdentity,
};
use thiserror::Error;

use super::model_witness::{WITNESS_POLL_INTERVAL, decode_lower_hex_hash};
use crate::{movement::PhysicsCollisionRegistries, runtime::world::ClientWorld};

pub(crate) const ACTOR_WITNESS_SCHEMA: &str = "rust-mcbe-actor-witness-v1";
pub(crate) const MAX_ACTOR_WITNESS_REQUEST_BYTES: usize = 64 * 1_024;
pub(crate) const MAX_ACTOR_WITNESS_ACTORS: usize = 16;
pub(crate) const MAX_ACTOR_FEET_ERROR_MICROS: u32 = 10_000;
pub(crate) const REQUIRED_CONSECUTIVE_PRESENTED_FRAMES: u8 = 2;
pub(crate) const MAX_ACTOR_GROUND_CAPTURES: usize = 4_096;

const GROUND_CONTACT_TOLERANCE_BLOCKS: f64 = MAX_ACTOR_FEET_ERROR_MICROS as f64 / 1_000_000.0;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActorWitnessSelector {
    pub(crate) runtime_id: u64,
    pub(crate) spawn_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActorWitnessRequest {
    pub(crate) session: u64,
    pub(crate) dimension: i32,
    pub(crate) actors: Vec<ActorWitnessSelector>,
    pub(crate) maximum_feet_error_micros: u32,
    pub(crate) required_consecutive_presented_frames: u8,
    pub(crate) request_sha256: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorWitnessFile {
    schema: String,
    session: u64,
    dimension: i32,
    actors: Vec<ActorWitnessSelector>,
    maximum_feet_error_micros: u32,
    required_consecutive_presented_frames: u8,
    request_sha256: String,
}

#[derive(Serialize)]
struct ActorWitnessHashInput<'a> {
    schema: &'a str,
    session: u64,
    dimension: i32,
    actors: &'a [ActorWitnessSelector],
    maximum_feet_error_micros: u32,
    required_consecutive_presented_frames: u8,
}

pub(crate) fn read_actor_witness_request(path: &Path) -> Result<ActorWitnessRequest> {
    let file = File::open(path).context("open actor witness request")?;
    let mut bytes = Vec::new();
    file.take(MAX_ACTOR_WITNESS_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context("read actor witness request")?;
    ensure!(
        bytes.len() <= MAX_ACTOR_WITNESS_REQUEST_BYTES,
        "actor witness request is too large"
    );
    decode_actor_witness_request(&bytes)
}

pub(crate) fn decode_actor_witness_request(bytes: &[u8]) -> Result<ActorWitnessRequest> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_ACTOR_WITNESS_REQUEST_BYTES,
        "actor witness request must contain 1..={MAX_ACTOR_WITNESS_REQUEST_BYTES} bytes"
    );
    let file: ActorWitnessFile =
        serde_json::from_slice(bytes).context("decode actor witness request JSON")?;
    ensure!(
        file.schema == ACTOR_WITNESS_SCHEMA,
        "unsupported actor witness schema"
    );
    ensure!(file.session != 0, "actor witness session must be nonzero");
    ensure!(
        !file.actors.is_empty() && file.actors.len() <= MAX_ACTOR_WITNESS_ACTORS,
        "actor witness request must contain 1..={MAX_ACTOR_WITNESS_ACTORS} actors"
    );
    ensure!(
        file.maximum_feet_error_micros <= MAX_ACTOR_FEET_ERROR_MICROS,
        "actor witness feet error exceeds {MAX_ACTOR_FEET_ERROR_MICROS} micrometres"
    );
    ensure!(
        file.required_consecutive_presented_frames == REQUIRED_CONSECUTIVE_PRESENTED_FRAMES,
        "actor witness requires exactly {REQUIRED_CONSECUTIVE_PRESENTED_FRAMES} consecutive presented frames"
    );

    let mut identities = HashSet::with_capacity(file.actors.len());
    for actor in &file.actors {
        ensure!(
            actor.runtime_id != 0 && actor.spawn_revision != 0,
            "actor witness lifetime identities must be nonzero"
        );
        ensure!(
            identities.insert((actor.runtime_id, actor.spawn_revision)),
            "actor witness lifetime identities must be unique"
        );
    }

    let request_sha256 = decode_lower_hex_hash(&file.request_sha256)?;
    ensure!(
        request_sha256 != [0; 32],
        "actor witness request hash must be nonzero"
    );
    let canonical = serde_json::to_vec(&ActorWitnessHashInput {
        schema: &file.schema,
        session: file.session,
        dimension: file.dimension,
        actors: &file.actors,
        maximum_feet_error_micros: file.maximum_feet_error_micros,
        required_consecutive_presented_frames: file.required_consecutive_presented_frames,
    })
    .context("encode canonical actor witness request")?;
    let actual_sha256: [u8; 32] = Sha256::digest(canonical).into();
    ensure!(
        actual_sha256 == request_sha256,
        "actor witness request hash mismatch"
    );

    Ok(ActorWitnessRequest {
        session: file.session,
        dimension: file.dimension,
        actors: file.actors,
        maximum_feet_error_micros: file.maximum_feet_error_micros,
        required_consecutive_presented_frames: file.required_consecutive_presented_frames,
        request_sha256,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActorGroundContact {
    pub(crate) ground_plane_y: f64,
    pub(crate) feet_error_micros: u32,
    pub(crate) collision_identity: WorldCollisionIdentity,
}

#[derive(Debug, Error)]
pub(crate) enum ActorGroundContactError {
    #[error("actor witness feet position must be finite")]
    NonFiniteFeet,
    #[error("actor witness collision data is unavailable")]
    CollisionUnavailable,
    #[error("actor witness found no supporting collision plane")]
    NoSupportingPlane,
}

pub(crate) fn sample_actor_ground_contact(
    world: &impl CollisionWorld,
    feet: [f32; 3],
) -> Result<ActorGroundContact, ActorGroundContactError> {
    if !feet.iter().all(|coordinate| coordinate.is_finite()) {
        return Err(ActorGroundContactError::NonFiniteFeet);
    }
    let feet = Vec3::new(f64::from(feet[0]), f64::from(feet[1]), f64::from(feet[2]));
    let half_width = PLAYER_WIDTH * 0.5 - PLAYER_HORIZONTAL_EPSILON;
    let query = Aabb::new(
        Vec3::new(
            feet.x - half_width,
            feet.y - GROUND_CONTACT_TOLERANCE_BLOCKS,
            feet.z - half_width,
        ),
        Vec3::new(
            feet.x + half_width,
            feet.y + GROUND_CONTACT_TOLERANCE_BLOCKS,
            feet.z + half_width,
        ),
    );
    let collision = world
        .collision_boxes(query)
        .map_err(|_| ActorGroundContactError::CollisionUnavailable)?;
    let ground_plane_y = collision
        .value
        .iter()
        .filter(|shape| shape.min.is_finite() && shape.max.is_finite())
        .filter(|shape| {
            shape.min.x < shape.max.x && shape.min.y < shape.max.y && shape.min.z < shape.max.z
        })
        .filter(|shape| {
            shape.max.x >= query.min.x
                && shape.min.x <= query.max.x
                && shape.max.z >= query.min.z
                && shape.min.z <= query.max.z
        })
        .map(|shape| shape.max.y)
        .filter(|plane| (feet.y - plane).abs() <= GROUND_CONTACT_TOLERANCE_BLOCKS)
        .max_by(f64::total_cmp)
        .ok_or(ActorGroundContactError::NoSupportingPlane)?;
    let feet_error_micros = ((feet.y - ground_plane_y).abs() * 1_000_000.0).round() as u32;

    Ok(ActorGroundContact {
        ground_plane_y,
        feet_error_micros,
        collision_identity: collision.identity,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CommittedActorGroundContact {
    pub(crate) request_sha256: [u8; 32],
    pub(crate) sequence: u64,
    pub(crate) collision_world_generation: u64,
    pub(crate) lifetime: ActorLifetimeId,
    pub(crate) movement_revision: u64,
    pub(crate) previous_pose: ActorPose,
    pub(crate) current_pose: ActorPose,
    pub(crate) received_pose: ActorPose,
    pub(crate) interpolation_ticks_remaining: u8,
    pub(crate) source_tick: Option<i64>,
    pub(crate) packet_on_ground: Option<bool>,
    pub(crate) store_on_ground: Option<bool>,
    pub(crate) ground_plane_y: f64,
    pub(crate) feet_error_micros: u32,
    pub(crate) collision_identity: WorldCollisionIdentity,
    pub(crate) within_requested_error: bool,
    pub(crate) required_consecutive_presented_frames: u8,
}

pub(crate) fn capture_committed_actor_ground_contact(
    request: &ActorWitnessRequest,
    commit: &CommittedActorMove,
    current_collision_world_generation: Option<u64>,
    world: &impl CollisionWorld,
) -> Result<Option<CommittedActorGroundContact>, ActorGroundContactError> {
    if Some(commit.collision_world_generation) != current_collision_world_generation
        || commit.session_id != request.session
        || commit.dimension != request.dimension
    {
        return Ok(None);
    }
    let Some(applied) = commit.applied.as_ref() else {
        return Ok(None);
    };
    if applied.lifetime.session_id != request.session
        || applied.lifetime.dimension != request.dimension
        || applied.movement_revision != commit.sequence
        || commit.movement.runtime_id != applied.lifetime.runtime_id
        || commit.movement.dimension != request.dimension
        || !request.actors.iter().any(|selector| {
            selector.runtime_id == applied.lifetime.runtime_id
                && selector.spawn_revision == applied.lifetime.spawn_revision
        })
    {
        return Ok(None);
    }

    let contact = sample_actor_ground_contact(world, applied.received_pose.position)?;
    Ok(Some(CommittedActorGroundContact {
        request_sha256: request.request_sha256,
        sequence: commit.sequence,
        collision_world_generation: commit.collision_world_generation,
        lifetime: applied.lifetime,
        movement_revision: applied.movement_revision,
        previous_pose: applied.previous_pose,
        current_pose: applied.current_pose,
        received_pose: applied.received_pose,
        interpolation_ticks_remaining: applied.interpolation_ticks_remaining,
        source_tick: applied.source_tick,
        packet_on_ground: commit.movement.on_ground,
        store_on_ground: applied.on_ground,
        ground_plane_y: contact.ground_plane_y,
        feet_error_micros: contact.feet_error_micros,
        collision_identity: contact.collision_identity,
        within_requested_error: contact.feet_error_micros <= request.maximum_feet_error_micros,
        required_consecutive_presented_frames: request.required_consecutive_presented_frames,
    }))
}

#[derive(Resource)]
pub(crate) struct ActorWitnessFileSource {
    path: Option<PathBuf>,
    next_poll: Instant,
    request: Option<ActorWitnessRequest>,
    last_error: Option<String>,
    lifecycle: Option<(u64, i32)>,
    captures: VecDeque<CommittedActorGroundContact>,
    dropped_capture_count: u64,
}

impl ActorWitnessFileSource {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            next_poll: Instant::now(),
            request: None,
            last_error: None,
            lifecycle: None,
            captures: VecDeque::new(),
            dropped_capture_count: 0,
        }
    }

    fn poll(&mut self, now: Instant) {
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + WITNESS_POLL_INTERVAL;
        let Some(path) = self.path.as_deref() else {
            return;
        };
        match read_actor_witness_request(path) {
            Ok(request) => {
                if self.request.as_ref() != Some(&request) {
                    self.request = Some(request);
                    self.captures.clear();
                    self.dropped_capture_count = 0;
                }
                self.last_error = None;
            }
            Err(error) => {
                let message = error.to_string();
                if self.last_error.as_deref() != Some(&message) {
                    warn!("actor witness request rejected: {message}");
                }
                self.last_error = Some(message);
                self.request = None;
                self.captures.clear();
                self.dropped_capture_count = 0;
            }
        }
    }

    pub(crate) fn observe_lifecycle(&mut self, session: u64, dimension: i32) {
        let lifecycle = (session, dimension);
        if self.lifecycle == Some(lifecycle) {
            return;
        }
        self.lifecycle = Some(lifecycle);
        self.captures.clear();
        self.dropped_capture_count = 0;
    }

    pub(crate) fn try_record(&mut self, capture: CommittedActorGroundContact) -> bool {
        if self.pending_capture_count() == MAX_ACTOR_GROUND_CAPTURES {
            self.dropped_capture_count = self.dropped_capture_count.saturating_add(1);
            let dropped_capture_count = self.dropped_capture_count();
            warn!(
                dropped_capture_count,
                "actor ground-contact capture queue is full"
            );
            return false;
        }
        self.captures.push_back(capture);
        let capture = self
            .captures
            .back()
            .expect("admitted actor ground-contact capture is retained");
        debug!(
            request_sha256_prefix = ?&capture.request_sha256[..4],
            sequence = capture.sequence,
            collision_world_generation = capture.collision_world_generation,
            session = capture.lifetime.session_id,
            dimension = capture.lifetime.dimension,
            runtime_id = capture.lifetime.runtime_id,
            spawn_revision = capture.lifetime.spawn_revision,
            movement_revision = capture.movement_revision,
            previous_position = ?capture.previous_pose.position,
            current_position = ?capture.current_pose.position,
            received_position = ?capture.received_pose.position,
            interpolation_ticks_remaining = capture.interpolation_ticks_remaining,
            source_tick = ?capture.source_tick,
            packet_on_ground = ?capture.packet_on_ground,
            store_on_ground = ?capture.store_on_ground,
            ground_plane_y = capture.ground_plane_y,
            feet_error_micros = capture.feet_error_micros,
            collision_protocol = capture.collision_identity.registry.protocol,
            collision_chunk_count = capture.collision_identity.chunks.len(),
            within_requested_error = capture.within_requested_error,
            required_consecutive_presented_frames = capture.required_consecutive_presented_frames,
            "captured committed actor ground contact"
        );
        true
    }

    #[must_use]
    pub(crate) fn pending_capture_count(&self) -> usize {
        self.captures.len()
    }

    #[must_use]
    pub(crate) const fn dropped_capture_count(&self) -> u64 {
        self.dropped_capture_count
    }
}

pub(crate) fn poll_and_capture_actor_ground_contacts(
    mut client_world: ResMut<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    mut source: ResMut<ActorWitnessFileSource>,
) {
    source.poll(Instant::now());
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    source.observe_lifecycle(stream.actor_session_id(), stream.current_dimension());
    let commits = stream.take_committed_actor_moves();
    let Some(request) = source.request.clone() else {
        return;
    };
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let current_collision_world_generation = stream.collision_world_generation_identity();
    for commit in commits {
        match capture_committed_actor_ground_contact(
            &request,
            &commit,
            current_collision_world_generation,
            &world,
        ) {
            Ok(Some(capture)) => {
                let _ = source.try_record(capture);
            }
            Ok(None) => {}
            Err(error) => {
                debug!(%error, sequence = commit.sequence, "actor ground contact unavailable")
            }
        }
    }
}
