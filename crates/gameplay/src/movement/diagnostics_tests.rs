use std::time::{Duration, Instant};

use client_world::{
    CommittedControlEvent, LocalPlayerFacts, MovementFlagUpdate, ResolvedServerPosition,
};
use protocol::{
    MovePlayerEvent, MovePlayerMode, MovementCorrectionSubject, PlayerGameMode,
    PlayerMovementCorrectionEvent,
};

use super::{
    CorrectionContext, CorrectionContextLine, CorrectionContextLog, CorrectionPacket,
    FULL_CONTEXT_LINES, MAX_CAMERA_PRESET_CHARS, SUMMARY_WINDOW, bounded_preset,
};

const RESOLVED: ResolvedServerPosition = ResolvedServerPosition {
    position: [0.0, 84.0, 0.0],
    surface_anchor: None,
};

#[test]
fn packet_facts_keep_move_player_ride_cause_and_tick() {
    let teleport = CommittedControlEvent::MovePlayer {
        sequence: 1,
        source_cohort: None,
        movement: MovePlayerEvent {
            mode: MovePlayerMode::Teleport,
            teleported: true,
            source_tick: 0,
            riding_runtime_id: 91,
            teleport_cause: Some(3),
            ..Default::default()
        },
        resolved: RESOLVED,
    };
    assert_eq!(
        CorrectionPacket::from_control(&teleport),
        Some(CorrectionPacket {
            move_mode: Some(MovePlayerMode::Teleport),
            tick: 0,
            riding_runtime_id: Some(91),
            teleport_cause: Some(3),
            on_ground: false,
        })
    );

    let correction = CommittedControlEvent::PlayerMovementCorrection {
        sequence: 2,
        correction: PlayerMovementCorrectionEvent {
            position: [0.0, 84.0, 0.0],
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: MovementCorrectionSubject::Player,
            on_ground: true,
            tick: 412,
        },
        resolved: RESOLVED,
    };
    let facts = CorrectionPacket::from_control(&correction).expect("correction facts");
    assert_eq!(facts.move_mode, None);
    assert_eq!(facts.tick, 412);
    assert_eq!(facts.riding_runtime_id, None);
    assert!(facts.on_ground);

    let other = CommittedControlEvent::LocalHurt {
        sequence: 3,
        source_direction: None,
    };
    assert_eq!(CorrectionPacket::from_control(&other), None);
}

#[test]
fn context_reads_mode_flight_gravity_immobility_and_mount() {
    let mut facts = LocalPlayerFacts::new(7);
    let unknown = CorrectionContext::read(&facts, true, None, false);
    assert_eq!(unknown.game_mode, None);
    assert!(!unknown.can_fly && !unknown.server_flying);
    assert!(unknown.has_gravity && !unknown.immobile);

    facts.publish_player_game_mode(PlayerGameMode::Creative);
    facts.apply_local_movement_flags(
        7,
        MovementFlagUpdate {
            immobile: Some(true),
            has_gravity: Some(false),
            ..Default::default()
        },
    );
    facts.set_mount(Some(-55));
    let context = CorrectionContext::read(&facts, false, Some("minecraft:free"), true);
    assert_eq!(context.game_mode, Some(PlayerGameMode::Creative));
    assert!(context.can_fly);
    assert!(!context.server_flying);
    assert!(!context.has_gravity);
    assert!(context.immobile);
    assert_eq!(context.mount_unique_id, Some(-55));
    assert!(!context.predicting);
    assert_eq!(context.camera_preset, Some("minecraft:free"));
    assert!(context.camera_override);
}

#[test]
fn correction_loop_prints_a_bounded_prefix_then_one_summary_per_window() {
    let mut log = CorrectionContextLog::default();
    let start = Instant::now();
    let half_second = Duration::from_millis(500);
    let mut printed = Vec::new();
    // Two minutes of 2 Hz teleports.
    for step in 0..240_u32 {
        let now = start + half_second * step;
        if let CorrectionContextLine::Printed { seq, suppressed } = log.admit(1, now) {
            printed.push((now, seq, suppressed));
        }
    }
    let full = usize::try_from(FULL_CONTEXT_LINES).expect("small budget");
    for (expected, &(_, seq, suppressed)) in (1..=FULL_CONTEXT_LINES).zip(&printed) {
        assert_eq!((seq, suppressed), (expected, 0));
    }
    let summaries = &printed[full..];
    assert!(!summaries.is_empty() && summaries.len() <= 2);
    for pair in printed[full - 1..].windows(2) {
        assert!(pair[1].0.duration_since(pair[0].0) >= SUMMARY_WINDOW);
    }
    let accounted = u64::try_from(printed.len()).expect("few lines")
        + summaries.iter().map(|line| line.2).sum::<u64>();
    let last_seq = printed.last().expect("printed lines").1;
    assert_eq!(
        accounted, last_seq,
        "every quiet correction is counted once"
    );

    // A new session gets a fresh full budget.
    assert_eq!(
        log.admit(2, start + half_second * 240),
        CorrectionContextLine::Printed {
            seq: 1,
            suppressed: 0
        }
    );
}

#[test]
fn server_camera_preset_names_are_truncated() {
    let long = "p".repeat(500);
    assert_eq!(
        bounded_preset(&long).chars().count(),
        MAX_CAMERA_PRESET_CHARS
    );
    assert_eq!(bounded_preset("minecraft:free"), "minecraft:free");
}
