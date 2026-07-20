use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, ensure};
#[cfg(test)]
use bevy::prelude::{App, Update};
use bevy::prelude::{Local, Res, ResMut, Resource};
use client_world::{ActorSnapshot, CommittedActorMove, WorldStream};
use protocol::{ActorMoveEvent, ActorPositionOrigin};
use render::{
    ActorPresentedFrameAck, ChunkRenderQueue, ModelWitnessEvidence, ModelWitnessRequest,
    PresentedFrameGate, TargetRenderExpectation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Instant;
use world::SubChunkKey;

use super::markers::ACTOR_POSE_WITNESS;
use super::teleport::render_view_cohort;
use crate::runtime::world::ClientWorld;

pub(crate) fn actor_pose_witness_marker(
    sequence: u64,
    movement: &ActorMoveEvent,
    actor: Option<&ActorSnapshot>,
) -> String {
    let origin = match movement.position_origin {
        ActorPositionOrigin::Feet => "feet",
        ActorPositionOrigin::NetworkOffset => "network_offset",
    };
    let store = actor.map(|actor| {
        serde_json::json!({
            "unique_id": actor.unique_id,
            "movement_revision": actor.movement_revision,
            "applied": actor.movement_revision == sequence,
            "position": actor.position,
            "previous_position": actor.previous_pose.position,
            "received_position": actor.received_pose.position,
            "interpolation_ticks_remaining": actor.interpolation_ticks_remaining,
            "on_ground": actor.on_ground,
            "source_tick": actor.source_tick,
        })
    });
    format!(
        "{ACTOR_POSE_WITNESS}={}",
        serde_json::json!({
            "sequence": sequence,
            "runtime_id": movement.runtime_id,
            "packet": {
                "position": movement.position,
                "origin": origin,
                "teleported": movement.teleported,
                "snap": movement.snap,
                "on_ground": movement.on_ground,
                "source_tick": movement.source_tick,
            },
            "store": store,
        })
    )
}

/// Correlate a committed store pose with an exact presented actor frame acknowledgement.
#[must_use]
pub(crate) fn committed_actor_move_matches_presented_frame(
    commit: &CommittedActorMove,
    acknowledgement: &ActorPresentedFrameAck,
) -> bool {
    let Some(applied) = commit.applied.as_ref() else {
        return false;
    };
    acknowledgement.frame_sequence != 0
        && !acknowledgement.manifest.is_empty()
        && acknowledgement.manifest.iter().any(|entry| {
            entry.identity.session_id == applied.lifetime.session_id
                && entry.identity.dimension == applied.lifetime.dimension
                && entry.identity.runtime_id == applied.lifetime.runtime_id
                && entry.identity.spawn_revision == applied.lifetime.spawn_revision
                && entry.identity.movement_revision == applied.movement_revision
        })
}

pub(crate) const MODEL_WITNESS_SCHEMA: &str = "rust-mcbe-model-witness-v1";
pub(crate) const WITNESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelWitnessSubChunk {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelWitnessFile {
    pub(crate) schema: String,
    pub(crate) revision: u64,
    pub(crate) dimension: i32,
    pub(crate) request_sha256: String,
    pub(crate) sub_chunks: Vec<ModelWitnessSubChunk>,
}

#[derive(Serialize)]
pub(crate) struct ModelWitnessHashInput<'a> {
    pub(crate) schema: &'a str,
    pub(crate) revision: u64,
    pub(crate) dimension: i32,
    pub(crate) sub_chunks: &'a [ModelWitnessSubChunk],
}

pub(crate) fn canonical_request_hash(file: &ModelWitnessFile) -> Result<[u8; 32]> {
    let canonical = serde_json::to_vec(&ModelWitnessHashInput {
        schema: &file.schema,
        revision: file.revision,
        dimension: file.dimension,
        sub_chunks: &file.sub_chunks,
    })
    .context("encode canonical model witness request")?;
    Ok(Sha256::digest(canonical).into())
}

pub(crate) fn decode_lower_hex_hash(value: &str) -> Result<[u8; 32]> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "request_sha256 must be exactly 64 lowercase hexadecimal characters"
    );
    let mut hash = [0_u8; 32];
    for (index, output) in hash.iter_mut().enumerate() {
        *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .context("decode request_sha256")?;
    }
    Ok(hash)
}

pub(crate) fn decode_request(bytes: &[u8]) -> Result<ModelWitnessRequest> {
    let file: ModelWitnessFile =
        serde_json::from_slice(bytes).context("decode model witness request JSON")?;
    ensure!(
        file.schema == MODEL_WITNESS_SCHEMA,
        "unsupported model witness schema"
    );
    let declared_hash = decode_lower_hex_hash(&file.request_sha256)?;
    ensure!(
        canonical_request_hash(&file)? == declared_hash,
        "model witness request hash mismatch"
    );
    let keys = file
        .sub_chunks
        .iter()
        .map(|key| SubChunkKey::new(file.dimension, key.x, key.y, key.z))
        .collect();
    ModelWitnessRequest::try_new(file.revision, declared_hash, keys)
        .map_err(|error| anyhow::anyhow!("invalid model witness request: {error:?}"))
}

#[derive(Resource)]
pub struct ModelWitnessFileSource {
    path: Option<PathBuf>,
    next_poll: std::time::Instant,
    last_digest: Option<[u8; 32]>,
    was_missing: bool,
}

impl ModelWitnessFileSource {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            next_poll: std::time::Instant::now(),
            last_digest: None,
            was_missing: true,
        }
    }

    pub fn configured(&self) -> bool {
        self.path.is_some()
    }
}

pub fn poll_model_witness_request(
    mut source: ResMut<ModelWitnessFileSource>,
    mut request: ResMut<ModelWitnessRequest>,
    evidence: Res<ModelWitnessEvidence>,
) {
    let now = std::time::Instant::now();
    if now < source.next_poll {
        return;
    }
    source.next_poll = now + WITNESS_POLL_INTERVAL;
    let Some(path) = source.path.as_ref() else {
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !source.was_missing || request.enabled() {
                *request = ModelWitnessRequest::default();
                evidence.reset();
            }
            source.was_missing = true;
            source.last_digest = None;
            return;
        }
        Err(error) => {
            *request = ModelWitnessRequest::default();
            evidence.reset();
            source.last_digest = None;
            eprintln!("model witness request read failed: {error}");
            return;
        }
    };
    source.was_missing = false;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    if source.last_digest == Some(digest) {
        return;
    }
    source.last_digest = Some(digest);
    match decode_request(&bytes) {
        Ok(next) => {
            evidence.set_authoritative_request(&next);
            if *request != next {
                *request = next;
            }
        }
        Err(error) => {
            *request = ModelWitnessRequest::default();
            evidence.reset();
            eprintln!("model witness request rejected: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_pose_witness_pairs_packet_origin_with_applied_store_pose() {
        use client_world::ActorPose;
        use protocol::{ActorGameMode, ActorKind};

        let pose = ActorPose {
            position: [4.0, 63.2, -8.0],
            pitch: 10.0,
            yaw: 20.0,
            head_yaw: 30.0,
        };
        let actor = ActorSnapshot {
            unique_id: -7,
            runtime_id: 42,
            spawn_revision: 1,
            movement_revision: 9,
            kind: ActorKind::Player {
                uuid: [7; 16],
                username: "witness".into(),
            },
            game_mode: Some(ActorGameMode::Survival),
            resolved_game_mode: Some(ActorGameMode::Survival),
            game_mode_tick: None,
            position: pose.position,
            velocity: [0.0; 3],
            pitch: pose.pitch,
            yaw: pose.yaw,
            head_yaw: pose.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: pose.yaw,
            on_ground: Some(true),
            teleported: false,
            player_mode: None,
            source_tick: Some(120),
            metadata: Default::default(),
            attributes: Default::default(),
            int_properties: Default::default(),
            float_properties: Default::default(),
        };
        let movement = ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(4.0), Some(65.0), Some(-8.0)],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: Some(10.0),
            yaw: Some(20.0),
            head_yaw: Some(30.0),
            on_ground: Some(true),
            teleported: false,
            snap: true,
            player_mode: None,
            source_tick: Some(120),
        };

        let marker = actor_pose_witness_marker(9, &movement, Some(&actor));
        let (_, payload) = marker.split_once('=').unwrap();
        let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(payload["packet"]["origin"], "network_offset");
        assert_eq!(payload["packet"]["position"][1], 65.0);
        assert_eq!(payload["packet"]["snap"], true);
        assert_eq!(payload["packet"]["teleported"], false);
        assert!((payload["store"]["position"][1].as_f64().unwrap() - 63.2).abs() < 1e-5);
        assert_eq!(payload["store"]["movement_revision"], 9);
        assert_eq!(payload["store"]["applied"], true);

        let missing = actor_pose_witness_marker(10, &movement, None);
        let (_, payload) = missing.split_once('=').unwrap();
        let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert!(payload["store"].is_null());
    }

    #[test]
    fn committed_actor_move_correlates_with_exact_presented_frame_manifest() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        use client_world::{ActorLifetimeId, ActorPose, CommittedActorPose};
        use render::{ActorDrawManifestEntry, ActorRenderIdentity, ActorRigRoute, EntityRigId};

        let lifetime = ActorLifetimeId {
            session_id: 3,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 1,
        };
        let pose = ActorPose {
            position: [1.0, 64.0, 2.0],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
        };
        let commit = CommittedActorMove {
            session_id: 3,
            dimension: 0,
            sequence: 9,
            movement: ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(1.0), Some(65.620_01), Some(2.0)],
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: Some(0.0),
                yaw: Some(0.0),
                head_yaw: Some(0.0),
                on_ground: Some(true),
                teleported: false,
                snap: false,
                player_mode: None,
                source_tick: Some(120),
            },
            applied: Some(CommittedActorPose {
                lifetime,
                movement_revision: 9,
                previous_pose: pose,
                current_pose: pose,
                received_pose: pose,
                interpolation_ticks_remaining: 3,
                on_ground: Some(true),
                source_tick: Some(120),
            }),
        };
        let now = Instant::now();
        let acknowledgement = ActorPresentedFrameAck {
            frame_sequence: 1,
            frame_generation: 1,
            draw_generation: 1,
            manifest: Arc::from([ActorDrawManifestEntry {
                identity: ActorRenderIdentity {
                    session_id: 3,
                    dimension: 0,
                    runtime_id: 42,
                    spawn_revision: 1,
                    ingress_sequence: 9,
                    source_tick: Some(120),
                    movement_revision: 9,
                    pose_generation: 1,
                },
                rig: EntityRigId(0),
                completed_tick: 0,
                reset_generation: 0,
                route: ActorRigRoute::Compiled,
                instance_index: 0,
                previous_bone_base: 0,
                current_bone_base: 0,
                bone_count: 0,
            }]),
            present_returned_at: now,
            gpu_completed_at: now + Duration::from_millis(1),
        };
        assert!(committed_actor_move_matches_presented_frame(
            &commit,
            &acknowledgement
        ));

        let mut mismatched = acknowledgement.clone();
        let mut entry = mismatched.manifest[0].clone();
        entry.identity.movement_revision = 8;
        mismatched.manifest = Arc::from([entry]);
        assert!(!committed_actor_move_matches_presented_frame(
            &commit,
            &mismatched
        ));
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    pub(crate) fn request_json(revision: u64, sub_chunks: &[ModelWitnessSubChunk]) -> Vec<u8> {
        let file = ModelWitnessFile {
            schema: MODEL_WITNESS_SCHEMA.to_owned(),
            revision,
            dimension: 0,
            request_sha256: String::new(),
            sub_chunks: sub_chunks.to_vec(),
        };
        let hash = canonical_request_hash(&file).unwrap();
        let hash = hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        serde_json::to_vec(&serde_json::json!({
            "schema": MODEL_WITNESS_SCHEMA,
            "revision": revision,
            "dimension": 0,
            "request_sha256": hash,
            "sub_chunks": sub_chunks,
        }))
        .unwrap()
    }

    #[test]
    pub(crate) fn request_json_decodes_exact_hash_dimension_keys_and_revision() {
        let bytes = request_json(
            7,
            &[
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
                ModelWitnessSubChunk { x: 0, y: 4, z: 5 },
            ],
        );
        let request = decode_request(&bytes).unwrap();
        assert_eq!(request.revision(), 7);
        assert_eq!(
            request.keys(),
            &[SubChunkKey::new(0, 0, 4, 5), SubChunkKey::new(0, 1, 4, 5)]
        );
        assert_ne!(request.request_hash(), &[0; 32]);
    }

    #[test]
    pub(crate) fn request_json_fails_closed_for_hash_schema_duplicates_and_unknown_fields() {
        let valid = request_json(7, &[ModelWitnessSubChunk { x: 1, y: 4, z: 5 }]);
        let mut tampered: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        tampered["request_sha256"] = serde_json::Value::String("0".repeat(64));
        assert!(decode_request(&serde_json::to_vec(&tampered).unwrap()).is_err());

        tampered = serde_json::from_slice(&valid).unwrap();
        tampered["extra"] = serde_json::json!(true);
        assert!(decode_request(&serde_json::to_vec(&tampered).unwrap()).is_err());

        let duplicate = request_json(
            7,
            &[
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
            ],
        );
        assert!(decode_request(&duplicate).is_err());
    }

    #[test]
    pub(crate) fn file_poller_retries_same_bytes_after_non_not_found_read_error() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rust-mcbe-model-witness-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("request.json");
        let valid = request_json(7, &[ModelWitnessSubChunk { x: 1, y: 4, z: 5 }]);
        std::fs::write(&path, &valid).unwrap();

        let mut app = App::new();
        app.insert_resource(ModelWitnessFileSource::new(Some(path.clone())))
            .init_resource::<ModelWitnessRequest>()
            .init_resource::<ModelWitnessEvidence>()
            .add_systems(Update, poll_model_witness_request);
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 7);

        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        app.world_mut()
            .resource_mut::<ModelWitnessFileSource>()
            .next_poll = std::time::Instant::now();
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 0);

        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, &valid).unwrap();
        app.world_mut()
            .resource_mut::<ModelWitnessFileSource>()
            .next_poll = std::time::Instant::now();
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 7);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
#[derive(Default)]
pub(crate) struct ModelWitnessExpectationState {
    pub(crate) request: ModelWitnessRequest,
    pub(crate) expectation: Option<TargetRenderExpectation>,
    pub(crate) next_view_generation: u64,
}

pub(crate) fn drive_model_witness(
    client_world: Res<ClientWorld>,
    render_queue: Res<ChunkRenderQueue>,
    presented_frames: Res<PresentedFrameGate>,
    request: Res<ModelWitnessRequest>,
    evidence: Res<ModelWitnessEvidence>,
    mut state: Local<ModelWitnessExpectationState>,
) {
    if !request.enabled() {
        if state.request.enabled() {
            presented_frames.clear();
        }
        *state = ModelWitnessExpectationState::default();
        return;
    }
    if state.request != *request {
        presented_frames.clear();
        state.request = (*request).clone();
        state.expectation = None;
        state.next_view_generation = 0;
    }
    if evidence.is_complete_for(&request) {
        presented_frames.clear();
        state.expectation = None;
        return;
    }
    let Some(cohort) = client_world
        .stream
        .as_ref()
        .and_then(WorldStream::committed_view_cohort)
        .map(render_view_cohort)
    else {
        return;
    };
    let now = Instant::now();
    let Some(proposed) = render_queue.freeze_target_expectation_for_keys(
        cohort,
        None,
        request.keys().iter().copied(),
        0,
        now,
    ) else {
        if state.expectation.take().is_some() {
            presented_frames.clear();
        }
        return;
    };
    let expectation = if let Some(current) = state.expectation.as_ref().filter(|current| {
        current.cohort == proposed.cohort
            && current.source_cohort == proposed.source_cohort
            && current.manifest == proposed.manifest
    }) {
        current.clone()
    } else {
        state.next_view_generation = state.next_view_generation.wrapping_add(1).max(1);
        let mut next = proposed;
        next.view_generation = state.next_view_generation;
        state.expectation = Some(next.clone());
        next
    };
    presented_frames.set_expectation(expectation);
    for acknowledgement in presented_frames.drain() {
        evidence.observe_presented_frame(&request, &acknowledgement);
    }
    if evidence.is_complete_for(&request) {
        presented_frames.clear();
        state.expectation = None;
    }
}
