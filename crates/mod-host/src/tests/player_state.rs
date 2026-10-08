use super::*;

fn source(init: &str, frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("player_state.wat")
        .replace("$PLAYER_STATE", &format!("{name}/player-state@{version}"))
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}

fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.wat");
    std::fs::write(&path, source(init, frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            player_state: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

fn snapshot() -> PlayerStateSnapshot {
    let unknown = PlayerStateSlot {
        known: false,
        item: None,
    };
    PlayerStateSnapshot {
        session: 7,
        dimension: 0,
        selected_slot: None,
        inventory: vec![unknown.clone(); mod_api::PLAYER_STATE_INVENTORY_SLOTS],
        armor: vec![unknown.clone(); mod_api::PLAYER_STATE_ARMOR_SLOTS],
        offhand: unknown,
        effects: Vec::new(),
    }
}

fn call(denied: bool, present: bool) -> String {
    let result = format!(
        "i32.const 256 call $read i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(denied)
    );
    if denied {
        return result;
    }
    format!(
        "{result} i32.const 264 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(present)
    )
}

#[test]
fn player_state_crosses_the_component_boundary_without_gameplay_and_clears_between_frames() {
    let frame = format!(
        "global.get $count i32.const 1 i32.eq if {} i32.const 272 i64.load i64.const 7 i64.ne if unreachable end else {} end",
        call(false, true),
        call(false, false),
    );
    let (_dir, mut host) = load(&call(false, false), &frame, true);
    host.frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
        .unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
    let (_dir, mut denied) = load(&call(true, false), &call(true, false), false);
    denied
        .frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
        .unwrap();
    assert!(denied.is_active());
}

#[test]
fn player_state_read_budget_traps_and_reload_starts_without_previous_facts() {
    let frame = std::iter::repeat_n(call(false, true), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (_dir, mut host) = load("", &frame, true);
    assert!(
        host.frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
            .is_err()
    );
    assert!(!host.is_active());
    std::fs::write(&host.path, source(&call(false, false), &call(false, false))).unwrap();
    assert!(host.reload_if_changed().unwrap());
    host.frame(false).unwrap();
    assert!(host.is_active());
}
