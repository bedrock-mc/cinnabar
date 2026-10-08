use super::*;

fn source(frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("item_use.wat")
        .replace("$ITEM_USE", &format!("{name}/item-use@{version}"))
        .replace("$FRAME", frame)
}

fn load(frame: &str, item_use: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("item_use.wat");
    std::fs::write(&path, source(frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            item_use,
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
fn item_use_policy_is_granted_per_gameplay_frame_and_false_disables_it() {
    let (_dir, mut denied) = load(&call(true, true), false);
    denied.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(denied.item_use_delay_fix().is_none());
    let frame = format!(
        "global.get $count i32.const 1 i32.eq if {} else {} end",
        call(true, false),
        call(false, false)
    );
    let (_dir, mut host) = load(&frame, true);
    assert!(host.item_use_delay_fix().is_none());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.item_use_delay_fix(), Some((1, 0)));
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(host.item_use_delay_fix().is_none());
    let (_dir, mut menus) = load(&call(true, false), true);
    menus.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(menus.item_use_delay_fix(), Some((1, 0)));
    menus.frame(false).unwrap();
    assert!(menus.item_use_delay_fix().is_none());
}

#[test]
fn omission_traps_and_reload_revoke_item_use_policy() {
    let first = format!(
        "global.get $count i32.const 1 i32.eq if {} end",
        call(true, false)
    );
    let (_dir, mut omitted) = load(&first, true);
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(omitted.item_use_delay_fix().is_some());
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(omitted.item_use_delay_fix().is_none());
    let frame = format!(
        "{} global.get $count i32.const 2 i32.eq if unreachable end",
        call(true, false)
    );
    let (_dir, mut trapped) = load(&frame, true);
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(trapped.item_use_delay_fix().is_some());
    assert!(
        trapped
            .frame_with_gameplay(false, Some(snapshot()))
            .is_err()
    );
    assert!(!trapped.is_active());
    assert!(trapped.item_use_delay_fix().is_none());
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(trapped.item_use_delay_fix().is_none());
    let (_dir, mut reloaded) = load(&call(true, false), true);
    reloaded
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(reloaded.item_use_delay_fix().is_some());
    std::fs::write(&reloaded.path, source("")).unwrap();
    assert!(reloaded.reload_if_changed().unwrap());
    assert!(reloaded.item_use_delay_fix().is_none());
}

#[test]
fn item_use_import_budget_quarantines_excess_writes() {
    let calls = std::iter::repeat_n(call(true, false), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (_dir, mut host) = load(&calls, true);
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(!host.is_active());
    assert!(host.item_use_delay_fix().is_none());
}

#[test]
fn renewed_item_policy_tracks_its_current_session_and_dimension() {
    let (_dir, mut host) = load(&call(true, false), true);
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.item_use_delay_fix(), Some((1, 0)));
    let next = GameplaySnapshot {
        session: 2,
        dimension: 1,
        ..snapshot()
    };
    host.frame_with_gameplay(false, Some(next)).unwrap();
    assert_eq!(host.item_use_delay_fix(), Some((2, 1)));
    host.frame(false).unwrap();
    assert_eq!(host.item_use_delay_fix(), None);
}
