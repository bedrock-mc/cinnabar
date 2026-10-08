use super::*;

fn source(init: &str, frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("gameplay.wat")
        .replace("$GAMEPLAY", &format!("{name}/gameplay@{version}"))
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}

fn load(init: &str, frame: &str, grants: ModGrants) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gameplay.wat");
    std::fs::write(&path, source(init, frame)).unwrap();
    let host = ModHost::load_with_grants(&path, grants).unwrap();
    (directory, host)
}

fn grants() -> ModGrants {
    ModGrants {
        players: true,
        camera: true,
        ..Default::default()
    }
}

fn delay(value: u32, error: bool) -> String {
    format!(
        "i32.const {value} i32.const 256 call $delay i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(error)
    )
}

fn show_position(enabled: bool, error: bool) -> String {
    format!(
        "i32.const {} i32.const 256 call $show i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(enabled),
        u8::from(error)
    )
}

#[test]
fn real_position_visual_is_denied_by_default_retained_on_success_and_cleared_on_trap() {
    let (_directory, mut denied) = load("", &show_position(true, true), ModGrants::default());
    denied.frame(false).unwrap();
    assert!(!denied.show_real_position());
    let granted = ModGrants {
        packet_delay: true,
        ..Default::default()
    };
    let (_directory, mut enabled) = load(&show_position(true, false), "", granted.clone());
    enabled.frame(false).unwrap();
    assert!(enabled.show_real_position());
    let (_directory, mut disabled) = load(
        &show_position(true, false),
        &show_position(false, false),
        granted.clone(),
    );
    disabled.frame(false).unwrap();
    assert!(!disabled.show_real_position());
    let (_directory, mut trapped) = load(
        &show_position(true, false),
        &format!("{} unreachable", show_position(false, false)),
        granted,
    );
    assert!(trapped.frame(false).is_err());
    assert!(!trapped.show_real_position());
}

#[test]
fn packet_delay_is_denied_by_default_and_bounded_without_a_gameplay_frame() {
    let (_directory, mut denied) = load("", &delay(200, true), ModGrants::default());
    denied.frame(false).unwrap();
    assert_eq!(denied.packet_delay_ms(), 0);
    let granted = ModGrants {
        packet_delay: true,
        ..Default::default()
    };
    let (_directory, mut invalid) = load(
        "",
        &delay(mod_api::MAX_PACKET_DELAY_MS + 1, true),
        granted.clone(),
    );
    invalid.frame(false).unwrap();
    assert_eq!(invalid.packet_delay_ms(), 0);
    let (_directory, mut enabled) = load(&delay(mod_api::MAX_PACKET_DELAY_MS, false), "", granted);
    assert_eq!(enabled.packet_delay_ms(), mod_api::MAX_PACKET_DELAY_MS);
    enabled.frame(false).unwrap();
    assert_eq!(enabled.packet_delay_ms(), mod_api::MAX_PACKET_DELAY_MS);
}

#[test]
fn packet_delay_commits_on_success_and_clears_after_a_guest_trap() {
    let granted = ModGrants {
        packet_delay: true,
        ..Default::default()
    };
    let (_directory, mut disabled) = load(&delay(200, false), &delay(0, false), granted.clone());
    assert_eq!(disabled.packet_delay_ms(), 200);
    disabled.frame(false).unwrap();
    assert_eq!(disabled.packet_delay_ms(), 0);
    let (_directory, mut trapped) = load(
        &delay(200, false),
        &format!("{} unreachable", delay(500, false)),
        granted,
    );
    assert_eq!(trapped.packet_delay_ms(), 200);
    assert!(trapped.frame(false).is_err());
    assert!(!trapped.is_active());
    assert_eq!(trapped.packet_delay_ms(), 0);
    trapped.frame(false).unwrap();
    assert_eq!(trapped.packet_delay_ms(), 0);
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 42,
        dimension: -1,
        eye: GameplayVector3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        yaw: 0.4,
        pitch: 0.2,
        attack_held: true,
        frame_seconds: 0.016,
        players: vec![GameplayPlayer {
            runtime_id: 99,
            position: GameplayVector3 {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
        }],
    }
}

fn rotate(yaw: f32, pitch: f32, error: bool) -> String {
    let yaw = yaw.to_string().to_lowercase();
    let pitch = pitch.to_string().to_lowercase();
    format!(
        "f32.const {yaw} f32.const {pitch} i32.const 256 call $rotate \
        i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(error)
    )
}

fn read(error: bool, present: bool) -> String {
    let result = format!(
        "i32.const 128 call $read i32.const 128 i32.load8_u \
        i32.const {} i32.ne if unreachable end",
        u8::from(error)
    );
    if error {
        return result;
    }
    format!(
        "{result} i32.const 136 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(present)
    )
}

#[test]
fn current_frame_crosses_real_component_boundary_and_rotation_is_consumed_once() {
    let frame = format!(
        "{} i32.const 144 i64.load i64.const 42 i64.ne if unreachable end \
        i32.const 152 i32.load i32.const -1 i32.ne if unreachable end \
        i32.const 156 f32.load f32.const 1 f32.ne if unreachable end \
        i32.const 168 f32.load f32.const 0.4 f32.ne if unreachable end \
        i32.const 172 f32.load f32.const 0.2 f32.ne if unreachable end \
        i32.const 176 i32.load8_u i32.const 1 i32.ne if unreachable end \
        i32.const 180 f32.load f32.const 0.016 f32.ne if unreachable end \
        i32.const 188 i32.load i32.const 1 i32.ne if unreachable end \
        i32.const 184 i32.load i64.load i64.const 99 i64.ne if unreachable end {}",
        read(false, true),
        rotate(0.1, -0.1, false)
    );
    let (_dir, mut host) = load("", &frame, grants());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(
        host.take_camera_delta(),
        Some(CameraDelta {
            yaw: 0.1,
            pitch: -0.1
        })
    );
    assert_eq!(host.take_camera_delta(), None);
}

#[test]
fn capabilities_are_independent_and_denied_by_default() {
    for permissions in [
        ModGrants::default(),
        ModGrants {
            players: true,
            ..Default::default()
        },
        ModGrants {
            camera: true,
            ..Default::default()
        },
    ] {
        let frame = format!(
            "{} {}",
            read(!permissions.players, true),
            rotate(0.1, 0.0, !permissions.camera)
        );
        let (_dir, mut host) = load("", &frame, permissions.clone());
        host.frame_with_gameplay(false, Some(snapshot())).unwrap();
        assert_eq!(host.take_camera_delta().is_some(), permissions.camera);
        assert!(host.is_active());
    }
}

#[test]
fn init_and_frames_without_gameplay_cannot_rotate_or_read_stale_data() {
    let no_frame = format!("{} {}", read(false, false), rotate(0.1, 0.0, true));
    let (_dir, mut host) = load(&no_frame, &no_frame, grants());
    host.frame(false).unwrap();
    assert_eq!(host.take_camera_delta(), None);
}

#[test]
fn trap_discards_staged_and_previous_camera_delta() {
    let (_dir, mut host) = load(
        "",
        &format!("{} unreachable", rotate(0.1, 0.0, false)),
        grants(),
    );
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert_eq!(host.take_camera_delta(), None);
    assert!(!host.is_active());
}

#[test]
fn next_frame_revokes_unconsumed_delta_and_snapshot() {
    let frame = format!(
        "i32.const 128 call $read i32.const 136 i32.load8_u \
        if {} else {} end",
        rotate(0.1, 0.0, false),
        rotate(0.1, 0.0, true)
    );
    let (_dir, mut host) = load("", &frame, grants());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    host.frame(false).unwrap();
    assert_eq!(host.take_camera_delta(), None);
}

#[test]
fn malformed_snapshots_fail_before_callback_and_revoke_previous_delta() {
    let (_dir, mut host) = load("", &rotate(0.1, 0.0, false), grants());
    for invalid in [0, 1, 2] {
        host.frame_with_gameplay(false, Some(snapshot())).unwrap();
        let mut frame = snapshot();
        match invalid {
            0 => frame.eye.x = f32::NAN,
            1 => frame.players = vec![frame.players[0]; mod_api::MAX_GAMEPLAY_PLAYERS + 1],
            _ => frame.frame_seconds = f32::INFINITY,
        }
        assert!(host.frame_with_gameplay(false, Some(frame)).is_err());
        assert_eq!(host.take_camera_delta(), None);
        assert!(host.is_active());
    }
}

#[test]
fn camera_requires_finite_and_cumulatively_bounded_deltas() {
    let limit = mod_api::MAX_CAMERA_DELTA_RADIANS;
    let frame = format!(
        "{} {} {} {}",
        rotate(limit, -limit, false),
        rotate(0.01, 0.0, true),
        rotate(f32::INFINITY, 0.0, true),
        rotate(f32::NAN, 0.0, true)
    );
    let (_dir, mut host) = load("", &frame, grants());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(
        host.take_camera_delta(),
        Some(CameraDelta {
            yaw: limit,
            pitch: -limit
        })
    );
}

#[test]
fn import_budgets_quarantine_before_commit() {
    for (call, expected) in [
        (read(false, true), "gameplay read budget exhausted"),
        (rotate(0.0, 0.0, false), "camera import budget exhausted"),
    ] {
        let (_dir, mut host) = load("", &format!("{call} ").repeat(9), grants());
        let error = host
            .frame_with_gameplay(false, Some(snapshot()))
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected));
        assert_eq!(host.take_camera_delta(), None);
    }
}

#[test]
fn reload_retains_grants_but_clears_pending_motion() {
    let (directory, mut host) = load("", &rotate(0.1, 0.0, false), grants());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    let path = directory.path().join("gameplay.wat");
    std::fs::write(
        path,
        source(&rotate(0.1, 0.0, true), &rotate(0.2, 0.0, false)),
    )
    .unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.take_camera_delta(), None);
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(
        host.take_camera_delta(),
        Some(CameraDelta {
            yaw: 0.2,
            pitch: 0.0
        })
    );
}
