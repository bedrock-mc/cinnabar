use super::*;
use crate::ModGrants;
use cinnabar::extension::{events::Host as _, gameplay::Host as _};

fn vector(x: f32, y: f32, z: f32) -> GameplayVector3 {
    GameplayVector3 { x, y, z }
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: vector(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
        attack_held: false,
        frame_seconds: 0.016,
        players: Vec::new(),
    }
}

fn mob(x: f32) -> GameplayMob {
    GameplayMob {
        runtime_id: 7,
        unique_id: -7,
        type_id: "cinnabar:hollow_warden".into(),
        position: vector(x, 64.0, 0.0),
        health: Some(200.0),
        max_health: Some(400.0),
    }
}

fn rig(x: f32, z: f32) -> GameplayCameraRig {
    GameplayCameraRig {
        offset: vector(x, 0.5, z),
        roll: 0.1,
        fov_delta: 5.0,
    }
}

fn granted() -> ModGrants {
    ModGrants {
        camera: true,
        entities: true,
        commands: vec!["ability".into()],
        ..Default::default()
    }
}

fn state(grants: ModGrants) -> State {
    let mut state = State::new(grants, String::new());
    state.snapshot = Some(snapshot());
    state.world.mobs = vec![mob(3.0)];
    state
}

#[test]
fn new_capabilities_are_denied_by_default() {
    let mut state = state(ModGrants::default());
    assert!(state.read_mobs().unwrap().is_err());
    assert!(state.set_camera_rig(Some(rig(0.8, 3.0))).unwrap().is_err());
    assert!(state.set_camera_rig(None).unwrap().is_err());
    assert!(
        state
            .request_command("/ability flash".into())
            .unwrap()
            .is_err()
    );
    state.world.commit();
    assert_eq!(state.world.rig, None);
    assert!(state.world.commands.is_empty());
}

#[test]
fn granted_reads_return_only_the_current_frame_mobs() {
    let mut state = state(granted());
    assert_eq!(state.read_mobs().unwrap().unwrap(), vec![mob(3.0)]);
    state.world.begin_frame();
    assert!(state.read_mobs().unwrap().unwrap().is_empty());
}

#[test]
fn rig_bounds_and_frame_requirement_are_enforced() {
    let mut state = state(granted());
    for bad in [
        rig(MAX_RIG_SIDE_BLOCKS + 0.1, 3.0),
        rig(0.0, -0.1),
        rig(0.0, MAX_RIG_BACK_BLOCKS + 0.1),
        rig(f32::NAN, 3.0),
        GameplayCameraRig {
            roll: MAX_RIG_ROLL_RADIANS + 0.1,
            ..rig(0.0, 3.0)
        },
        GameplayCameraRig {
            fov_delta: -MAX_RIG_FOV_DELTA_DEGREES - 1.0,
            ..rig(0.0, 3.0)
        },
    ] {
        assert!(state.set_camera_rig(Some(bad)).unwrap().is_err());
    }
    state.snapshot = None;
    assert!(state.set_camera_rig(Some(rig(0.8, 3.0))).unwrap().is_err());
    assert!(state.set_camera_rig(None).unwrap().is_ok());
}

#[test]
fn rig_is_retained_across_frames_until_cleared() {
    let mut state = state(granted());
    state.set_camera_rig(Some(rig(0.8, 3.0))).unwrap().unwrap();
    assert_eq!(state.world.rig, None, "staged until the callback returns");
    state.world.commit();
    assert_eq!(state.world.rig, Some(rig(0.8, 3.0)));
    state.world.begin_frame();
    state.world.commit();
    assert_eq!(state.world.rig, Some(rig(0.8, 3.0)));
    state.set_camera_rig(None).unwrap().unwrap();
    state.world.commit();
    assert_eq!(state.world.rig, None);
}

#[test]
fn commands_must_name_a_granted_command_in_printable_text() {
    let mut state = state(granted());
    for bad in [
        "ability flash",
        "/abilityx flash",
        "/op me",
        "/ability \u{a7}flash",
        "/ability\tflash",
    ] {
        assert!(state.request_command(bad.into()).unwrap().is_err(), "{bad}");
    }
    let long = format!("/ability {}", "x".repeat(MAX_COMMAND_BYTES));
    assert!(state.request_command(long).unwrap().is_err());
    state.snapshot = None;
    assert!(state.request_command("/ability".into()).unwrap().is_err());
    state.snapshot = Some(snapshot());
    state
        .request_command("/ability beam start".into())
        .unwrap()
        .unwrap();
    state.request_command("/ability".into()).unwrap().unwrap();
    state.world.commit();
    assert_eq!(state.world.commands, ["/ability beam start", "/ability"]);
}

#[test]
fn per_frame_command_and_cue_budgets_trap() {
    let mut state = state(granted());
    for _ in 0..MAX_COMMANDS_PER_FRAME {
        state
            .request_command("/ability flash".into())
            .unwrap()
            .unwrap();
    }
    assert!(state.request_command("/ability flash".into()).is_err());
    for _ in 0..MAX_CUES_PER_FRAME {
        state
            .emit("ability.flash".into(), vec![1.0])
            .unwrap()
            .unwrap();
    }
    assert!(state.emit("ability.flash".into(), Vec::new()).is_err());
}

#[test]
fn cues_need_short_names_and_finite_values() {
    let mut state = state(ModGrants::default());
    assert!(state.emit(String::new(), Vec::new()).unwrap().is_err());
    assert!(state.emit("Upper".into(), Vec::new()).unwrap().is_err());
    assert!(
        state
            .emit("x".repeat(MAX_CUE_NAME_BYTES + 1), Vec::new())
            .unwrap()
            .is_err()
    );
    assert!(state.emit("a".into(), vec![f32::NAN]).unwrap().is_err());
    assert!(
        state
            .emit("a".into(), vec![0.0; MAX_CUE_VALUES + 1])
            .unwrap()
            .is_err()
    );
    state
        .emit("camera.dodge".into(), vec![-1.0])
        .unwrap()
        .unwrap();
    state.world.commit();
    assert_eq!(
        state.world.cues,
        [ModCue {
            name: "camera.dodge".into(),
            values: vec![-1.0]
        }]
    );
    state.world.begin_frame();
    assert!(state.world.cues.is_empty());
}

#[test]
fn host_mob_input_is_bounded_by_count_range_and_frame() {
    let frame = snapshot();
    assert!(validate_mobs(Some(&frame), &[mob(3.0)]).is_ok());
    assert!(validate_mobs(None, &[mob(3.0)]).is_err());
    assert!(validate_mobs(Some(&frame), &[mob(MAX_MOB_RANGE_BLOCKS + 1.0)]).is_err());
    assert!(validate_mobs(Some(&frame), &vec![mob(1.0); MAX_GAMEPLAY_MOBS + 1]).is_err());
    let mut unnamed = mob(1.0);
    unnamed.type_id.clear();
    assert!(validate_mobs(Some(&frame), &[unnamed]).is_err());
    let mut bad_health = mob(1.0);
    bad_health.health = Some(f32::INFINITY);
    assert!(validate_mobs(Some(&frame), &[bad_health]).is_err());
}

#[test]
fn command_grants_reject_non_names() {
    for commands in [
        vec!["Ability".to_owned()],
        vec!["ab ility".to_owned()],
        vec![String::new()],
        vec!["a".to_owned(); mod_api::MAX_COMMAND_GRANTS + 1],
    ] {
        let grants = ModGrants {
            commands,
            ..Default::default()
        };
        assert!(grants.validate().is_err());
    }
    assert!(granted().validate().is_ok());
}

#[test]
fn commands_are_rate_limited_per_second_of_frame_time() {
    let mut state = state(granted());
    let mut sent = 0;
    for _ in 0..MAX_COMMANDS_PER_SECOND * 2 {
        state.world.begin_frame();
        state.world.advance_command_window(0.01);
        if state
            .request_command("/ability flash".into())
            .unwrap()
            .is_ok()
        {
            sent += 1;
        }
        state.world.commit();
    }
    assert_eq!(sent, MAX_COMMANDS_PER_SECOND);
    state.world.advance_command_window(1.0);
    assert!(
        state
            .request_command("/ability flash".into())
            .unwrap()
            .is_ok()
    );
}

fn cue(name: &str) -> ModCue {
    ModCue {
        name: name.into(),
        values: vec![1.0],
    }
}

#[test]
fn delivered_cues_are_filtered_bounded_and_polled_within_budget() {
    let mut delivered = vec![cue("Bad Name"), cue("ability.flash")];
    delivered.extend(std::iter::repeat_n(cue("x"), MAX_INCOMING_CUES + 4));
    let kept = incoming(delivered);
    assert_eq!(kept.len(), MAX_INCOMING_CUES);
    assert_eq!(kept[0], cue("ability.flash"));
    assert!(
        incoming(vec![ModCue {
            name: "a".into(),
            values: vec![f32::NAN]
        }])
        .is_empty()
    );

    let mut state = state(ModGrants::default());
    state.world.incoming = vec![cue("ability.flash")];
    for _ in 0..MAX_IMPORT_WRITES {
        assert_eq!(state.poll().unwrap(), [cue("ability.flash")]);
    }
    assert!(state.poll().is_err(), "poll budget traps");
}
