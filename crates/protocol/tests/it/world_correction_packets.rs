//! `CorrectPlayerMovePrediction` normalization contracts.
//!
//! Split from `world_packets.rs` to keep each integration-test binary inside
//! the architecture policy line limit.

use bedrock_protocol_1_26_51::enums::RewindType as CorrectPlayerMovePredictionPacketPredictionType;
use bedrock_protocol_1_26_51::packets::CorrectPlayerMovePrediction;
use bedrock_protocol_1_26_51::types::PlayerInputTick;
use protocol::{
    MovementCorrectionSubject, PlayerMovementCorrectionEvent, WorldEvent, into_world_event,
};

#[test]
fn normalizes_server_authoritative_movement_correction_to_the_local_player_surface() {
    let packet = CorrectPlayerMovePrediction {
        pos: [27.5, 111.0, 91.5].into(),
        pos_delta: [0.25, -1.5, 2.75].into(),
        rotation: [-12.25, 143.5].into(),
        on_ground: true,
        tick: PlayerInputTick(4_096),
        ..Default::default()
    };

    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
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
    let packet = CorrectPlayerMovePrediction {
        prediction_type: CorrectPlayerMovePredictionPacketPredictionType::Vehicle,
        pos: [300.0, 90.0, -200.0].into(),
        ..Default::default()
    };

    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
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
    let packet = CorrectPlayerMovePrediction {
        prediction_type: CorrectPlayerMovePredictionPacketPredictionType::Unknown(7),
        pos: [1.0, 2.0, 3.0].into(),
        ..Default::default()
    };

    let Some(WorldEvent::PlayerMovementCorrection(event)) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
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
        let packet = CorrectPlayerMovePrediction {
            pos_delta: [bad_delta[0], bad_delta[1], bad_delta[2]].into(),
            ..Default::default()
        };

        assert_eq!(
            into_world_event(protocol::Packet::new(packet), 0).unwrap(),
            None
        );
    }
}

#[test]
fn non_finite_rotation_skips_the_whole_correction() {
    for rotation in [[f32::NAN, 90.0], [-12.0, f32::INFINITY]] {
        let packet = CorrectPlayerMovePrediction {
            rotation: rotation.into(),
            ..Default::default()
        };

        assert_eq!(
            into_world_event(protocol::Packet::new(packet), 0).unwrap(),
            None
        );
    }
}

#[test]
fn non_finite_positions_stay_normalized_for_downstream_sentinel_recovery() {
    let packet = CorrectPlayerMovePrediction {
        pos: [32_769.62, f32::NAN, -12.0].into(),
        ..Default::default()
    };

    let Some(WorldEvent::PlayerMovementCorrection(event)) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("position sentinels are resolved downstream, not dropped here")
    };
    assert_eq!(event.position[0], 32_769.62);
    assert!(event.position[1].is_nan());
    assert_eq!(event.position[2], -12.0);
}
