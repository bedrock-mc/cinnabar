//! `CorrectPlayerMovePrediction` normalization contracts.
//!
//! Split from `world_packets.rs` to keep each integration-test binary inside
//! the architecture policy line limit.

use protocol::{
    MovementCorrectionSubject, PlayerMovementCorrectionEvent, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_44::{
    CorrectPlayerMovePredictionPacket,
    EnumsRewindType as CorrectPlayerMovePredictionPacketPredictionType, PlayerInputTick, Vec2,
    Vec3,
};

#[test]
fn normalizes_server_authoritative_movement_correction_to_the_local_player_surface() {
    let packet = CorrectPlayerMovePredictionPacket {
        pos: Vec3 {
            x: 27.5,
            y: 111.0,
            z: 91.5,
        },
        pos_delta: Vec3 {
            x: 0.25,
            y: -1.5,
            z: 2.75,
        },
        rotation: Vec2 {
            x: -12.25,
            y: 143.5,
        },
        on_ground: true,
        tick: PlayerInputTick { inputtick: 4_096 },
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::PlayerMovementCorrection(
            PlayerMovementCorrectionEvent {
                position: [27.5, 111.0, 91.5],
                delta: [0.25, -1.5, 2.75],
                pitch: -12.25,
                yaw: 143.5,
                subject: MovementCorrectionSubject::Player,
                on_ground: true,
                tick: 4_096,
            }
        ))
    );
}

#[test]
fn vehicle_prediction_correction_is_retained_with_its_rewind_subject() {
    let packet = CorrectPlayerMovePredictionPacket {
        prediction_type: CorrectPlayerMovePredictionPacketPredictionType::Vehicle,
        pos: Vec3 {
            x: 300.0,
            y: 90.0,
            z: -200.0,
        },
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::PlayerMovementCorrection(
            PlayerMovementCorrectionEvent {
                position: [300.0, 90.0, -200.0],
                delta: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                subject: MovementCorrectionSubject::Vehicle,
                on_ground: false,
                tick: 0,
            }
        ))
    );
}

#[test]
fn unknown_prediction_subjects_are_retained_without_invention() {
    let packet = CorrectPlayerMovePredictionPacket {
        prediction_type: CorrectPlayerMovePredictionPacketPredictionType::Unknown(7),
        pos: Vec3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        ..Default::default()
    };

    let Some(WorldEvent::PlayerMovementCorrection(event)) =
        into_world_event(packet.into(), 0).unwrap()
    else {
        panic!("expected a movement correction event")
    };
    assert_eq!(event.subject, MovementCorrectionSubject::Unknown(7));
}

#[test]
fn a_non_finite_velocity_record_skips_the_whole_correction() {
    for bad_delta in [
        [f32::NAN, 0.0, 0.0],
        [0.0, f32::INFINITY, 0.0],
        [0.0, 0.0, f32::NEG_INFINITY],
    ] {
        let packet = CorrectPlayerMovePredictionPacket {
            pos_delta: Vec3 {
                x: bad_delta[0],
                y: bad_delta[1],
                z: bad_delta[2],
            },
            ..Default::default()
        };

        assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
    }
}

#[test]
fn non_finite_rotation_skips_the_whole_correction() {
    for rotation in [
        Vec2 {
            x: f32::NAN,
            y: 90.0,
        },
        Vec2 {
            x: -12.0,
            y: f32::INFINITY,
        },
    ] {
        let packet = CorrectPlayerMovePredictionPacket {
            rotation,
            ..Default::default()
        };

        assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
    }
}

#[test]
fn non_finite_positions_stay_normalized_for_downstream_sentinel_recovery() {
    let packet = CorrectPlayerMovePredictionPacket {
        pos: Vec3 {
            x: 32_769.62,
            y: f32::NAN,
            z: -12.0,
        },
        ..Default::default()
    };

    let Some(WorldEvent::PlayerMovementCorrection(event)) =
        into_world_event(packet.into(), 0).unwrap()
    else {
        panic!("position sentinels are resolved downstream, not dropped here")
    };
    assert_eq!(event.position[0], 32_769.62);
    assert!(event.position[1].is_nan());
    assert_eq!(event.position[2], -12.0);
}
