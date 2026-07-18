use serde::Serialize;
use sha2::{Digest, Sha256};
use sim::{Aabb, CollisionQuery, CollisionWorld, Vec3, WorldQueryError};
use world::ChunkKey;

use client_world::{ActorLifetimeId, ActorPose, CommittedActorMove, CommittedActorPose};
use protocol::{ActorMoveEvent, ActorPositionOrigin};

use super::actor_witness::{
    ActorWitnessFileSource, ActorWitnessRequest, CommittedActorGroundContact,
    MAX_ACTOR_GROUND_CAPTURES, MAX_ACTOR_WITNESS_REQUEST_BYTES,
    capture_committed_actor_ground_contact, decode_actor_witness_request,
    read_actor_witness_request, sample_actor_ground_contact,
};

const SCHEMA: &str = "rust-mcbe-actor-witness-v1";

#[derive(Serialize)]
struct CanonicalRequest<'a> {
    schema: &'a str,
    session: u64,
    dimension: i32,
    actors: &'a [ActorSelector],
    maximum_feet_error_micros: u32,
    required_consecutive_presented_frames: u8,
}

#[derive(Clone, Serialize)]
struct ActorSelector {
    runtime_id: u64,
    spawn_revision: u64,
}

fn request_json(actors: &[ActorSelector]) -> serde_json::Value {
    let canonical = CanonicalRequest {
        schema: SCHEMA,
        session: 7,
        dimension: -1,
        actors,
        maximum_feet_error_micros: 10_000,
        required_consecutive_presented_frames: 2,
    };
    let request_sha256 = Sha256::digest(serde_json::to_vec(&canonical).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    serde_json::json!({
        "schema": canonical.schema,
        "session": canonical.session,
        "dimension": canonical.dimension,
        "actors": canonical.actors,
        "maximum_feet_error_micros": canonical.maximum_feet_error_micros,
        "required_consecutive_presented_frames": canonical.required_consecutive_presented_frames,
        "request_sha256": request_sha256,
    })
}

#[test]
fn actor_witness_request_is_hash_bound_identity_specific_and_bounded() {
    let selector = ActorSelector {
        runtime_id: 42,
        spawn_revision: 3,
    };
    let valid = request_json(std::slice::from_ref(&selector));
    let decoded = decode_actor_witness_request(&serde_json::to_vec(&valid).unwrap()).unwrap();
    assert_eq!(decoded.session, 7);
    assert_eq!(decoded.dimension, -1);
    assert_eq!(decoded.actors.len(), 1);
    assert_eq!(decoded.actors[0].runtime_id, 42);
    assert_eq!(decoded.actors[0].spawn_revision, 3);
    assert_eq!(decoded.maximum_feet_error_micros, 10_000);
    assert_eq!(decoded.required_consecutive_presented_frames, 2);
    assert_ne!(decoded.request_sha256, [0; 32]);

    let mut wrong_hash = valid.clone();
    wrong_hash["request_sha256"] = serde_json::Value::String("0".repeat(64));
    assert!(decode_actor_witness_request(&serde_json::to_vec(&wrong_hash).unwrap()).is_err());

    let too_many = vec![selector; 17];
    assert!(
        decode_actor_witness_request(&serde_json::to_vec(&request_json(&too_many)).unwrap())
            .is_err()
    );

    let mut wrong_consecutive = valid;
    wrong_consecutive["required_consecutive_presented_frames"] = serde_json::json!(1);
    assert!(
        decode_actor_witness_request(&serde_json::to_vec(&wrong_consecutive).unwrap()).is_err()
    );
}

#[test]
fn actor_witness_path_reader_rejects_oversize_before_json_decode() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-mcbe-actor-witness-oversize-{}-{nonce}.json",
        std::process::id(),
    ));
    std::fs::write(&path, vec![b'X'; MAX_ACTOR_WITNESS_REQUEST_BYTES + 1]).unwrap();

    let error = read_actor_witness_request(&path).unwrap_err().to_string();
    std::fs::remove_file(path).unwrap();

    assert!(error.contains("too large"));
    assert!(!error.contains("XXXXX"));
    assert!(!error.contains("decode actor witness request JSON"));
}

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![Aabb::new(
            Vec3::new(-16.0, 63.0, -16.0),
            Vec3::new(16.0, 64.0, 16.0),
        )]))
    }
}

struct UnavailableWorld;

impl CollisionWorld for UnavailableWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)))
    }
}

struct MalformedWorld(Aabb);

impl CollisionWorld for MalformedWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![self.0]))
    }
}

fn committed_capture_fixture() -> (
    ActorWitnessRequest,
    CommittedActorMove,
    CommittedActorGroundContact,
) {
    let selector = ActorSelector {
        runtime_id: 42,
        spawn_revision: 3,
    };
    let request = decode_actor_witness_request(
        &serde_json::to_vec(&request_json(std::slice::from_ref(&selector))).unwrap(),
    )
    .unwrap();
    let pose = ActorPose {
        position: [1.25, 64.0, -2.5],
        pitch: 5.0,
        yaw: 90.0,
        head_yaw: 100.0,
    };
    let movement = ActorMoveEvent {
        dimension: -1,
        runtime_id: 42,
        position: [Some(1.25), Some(65.620_01), Some(-2.5)],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(5.0),
        yaw: Some(90.0),
        head_yaw: Some(100.0),
        on_ground: Some(true),
        teleported: false,
        snap: false,
        player_mode: None,
        source_tick: Some(27),
    };
    let commit = CommittedActorMove {
        session_id: 7,
        dimension: -1,
        sequence: 9,
        collision_world_generation: 11,
        movement,
        applied: Some(CommittedActorPose {
            lifetime: ActorLifetimeId {
                session_id: 7,
                dimension: -1,
                runtime_id: 42,
                spawn_revision: 3,
            },
            movement_revision: 9,
            previous_pose: pose,
            current_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 3,
            on_ground: Some(true),
            source_tick: Some(27),
        }),
    };
    let capture = capture_committed_actor_ground_contact(&request, &commit, Some(11), &Floor)
        .unwrap()
        .expect("exact requested lifetime and generation are captured");
    (request, commit, capture)
}

#[test]
fn actor_ground_contact_uses_collision_plane_and_fails_closed_when_unavailable() {
    let sample = sample_actor_ground_contact(&Floor, [1.25, 64.0, -2.5]).unwrap();
    assert_eq!(sample.ground_plane_y, 64.0);
    assert_eq!(sample.feet_error_micros, 0);
    assert_eq!(sample.collision_identity.registry.protocol, 1001);

    assert!(sample_actor_ground_contact(&UnavailableWorld, [1.25, 64.0, -2.5]).is_err());
}

#[test]
fn actor_ground_contact_rejects_zero_and_inverted_collision_extents() {
    let zero_height = Aabb::new(Vec3::new(-16.0, 64.0, -16.0), Vec3::new(16.0, 64.0, 16.0));
    let inverted_height = Aabb::new(Vec3::new(-16.0, 65.0, -16.0), Vec3::new(16.0, 64.0, 16.0));

    for shape in [zero_height, inverted_height] {
        assert!(sample_actor_ground_contact(&MalformedWorld(shape), [1.25, 64.0, -2.5]).is_err());
    }
}

#[test]
fn committed_actor_capture_requires_exact_identity_and_samples_received_feet() {
    let (request, commit, capture) = committed_capture_fixture();
    assert_eq!(capture.request_sha256, request.request_sha256);
    assert_eq!(capture.sequence, 9);
    assert_eq!(capture.collision_world_generation, 11);
    assert_eq!(capture.lifetime, commit.applied.as_ref().unwrap().lifetime);
    assert_eq!(capture.movement_revision, 9);
    assert_eq!(capture.source_tick, Some(27));
    assert_eq!(capture.packet_on_ground, Some(true));
    assert_eq!(capture.store_on_ground, Some(true));
    assert_eq!(capture.ground_plane_y, 64.0);
    assert_eq!(capture.feet_error_micros, 0);
    assert!(capture.within_requested_error);
    assert!(
        capture_committed_actor_ground_contact(&request, &commit, Some(12), &UnavailableWorld)
            .unwrap()
            .is_none(),
        "later world generation must reject before collision sampling"
    );
    assert!(
        capture_committed_actor_ground_contact(&request, &commit, None, &UnavailableWorld)
            .unwrap()
            .is_none(),
        "exhausted collision identity must reject before collision sampling"
    );

    let mut wrong_session = request;
    wrong_session.session = 8;
    assert!(
        capture_committed_actor_ground_contact(&wrong_session, &commit, Some(11), &Floor)
            .unwrap()
            .is_none()
    );
}

#[test]
fn actor_witness_capture_lifecycle_resets_on_dimension_and_session() {
    let (_, _, capture) = committed_capture_fixture();
    let mut source = ActorWitnessFileSource::new(None);
    source.observe_lifecycle(7, -1);
    assert!(source.try_record(capture.clone()));
    assert_eq!(source.pending_capture_count(), 1);

    source.observe_lifecycle(7, 0);
    assert_eq!(source.pending_capture_count(), 0);
    assert!(source.try_record(capture.clone()));

    source.observe_lifecycle(8, 0);
    assert_eq!(source.pending_capture_count(), 0);
    assert_eq!(source.dropped_capture_count(), 0);
}

#[test]
fn actor_witness_capture_reports_success_only_after_capacity_admission() {
    let (_, _, capture) = committed_capture_fixture();
    let mut source = ActorWitnessFileSource::new(None);
    source.observe_lifecycle(7, -1);

    for _ in 0..MAX_ACTOR_GROUND_CAPTURES {
        assert!(source.try_record(capture.clone()));
    }
    assert!(!source.try_record(capture));
    assert_eq!(source.pending_capture_count(), MAX_ACTOR_GROUND_CAPTURES);
    assert_eq!(source.dropped_capture_count(), 1);
}
