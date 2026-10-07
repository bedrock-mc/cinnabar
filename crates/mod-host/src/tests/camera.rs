use super::*;

fn source(frame: &str) -> String {
    include_str!("camera.wat").replace("$FRAME", frame)
}

fn load(frame: &str, camera: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("camera.wat");
    std::fs::write(&path, source(frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            camera,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.0,
            y: 64.0,
            z: 0.0,
        },
        yaw: 0.0,
        pitch: 0.0,
        attack_held: false,
        frame_seconds: 0.016,
        players: Vec::new(),
    }
}

fn call(enabled: bool, denied: bool) -> String {
    format!(
        "i32.const {} i32.const 256 call $preserve i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(enabled),
        u8::from(denied)
    )
}

#[test]
fn camera_policy_is_granted_per_gameplay_frame_and_false_disables_it() {
    let (_dir, mut denied) = load(&call(true, true), false);
    denied.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(!denied.preserves_teleport_rotation());
    let frame = format!(
        "global.get $count i32.const 1 i32.eq if {} else {} end",
        call(true, false),
        call(false, false)
    );
    let (_dir, mut host) = load(&frame, true);
    assert!(!host.preserves_teleport_rotation());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(host.preserves_teleport_rotation());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(!host.preserves_teleport_rotation());
    let (_dir, mut menus) = load(&call(true, false), true);
    menus.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(menus.preserves_teleport_rotation());
    menus.frame(false).unwrap();
    assert!(!menus.preserves_teleport_rotation());
}

#[test]
fn omission_traps_and_reload_revoke_camera_policy() {
    let first = format!(
        "global.get $count i32.const 1 i32.eq if {} end",
        call(true, false)
    );
    let (_dir, mut omitted) = load(&first, true);
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(omitted.preserves_teleport_rotation());
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(!omitted.preserves_teleport_rotation());
    let frame = format!(
        "{} global.get $count i32.const 2 i32.eq if unreachable end",
        call(true, false)
    );
    let (_dir, mut trapped) = load(&frame, true);
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(trapped.preserves_teleport_rotation());
    assert!(
        trapped
            .frame_with_gameplay(false, Some(snapshot()))
            .is_err()
    );
    assert!(!trapped.is_active());
    assert!(!trapped.preserves_teleport_rotation());
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(!trapped.preserves_teleport_rotation());
    let (_dir, mut reloaded) = load(&call(true, false), true);
    reloaded
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(reloaded.preserves_teleport_rotation());
    std::fs::write(&reloaded.path, source("")).unwrap();
    assert!(reloaded.reload_if_changed().unwrap());
    assert!(!reloaded.preserves_teleport_rotation());
}

#[test]
fn camera_import_budget_quarantines_excess_writes() {
    let calls = std::iter::repeat_n(call(true, false), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (_dir, mut host) = load(&calls, true);
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(!host.is_active());
    assert!(!host.preserves_teleport_rotation());
}
