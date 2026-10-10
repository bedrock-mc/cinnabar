use super::*;

const EMPTY_COMPONENT: &str = r#"(component
    (core module $m (func (export "init")) (func (export "frame")))
    (core instance $i (instantiate $m))
    (func (export "init") (canon lift (core func $i "init")))
    (func (export "frame") (canon lift (core func $i "frame"))))"#;

fn component(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, EMPTY_COMPONENT).unwrap();
    path
}

#[test]
fn keys_reserved_by_an_earlier_mod_are_withheld_and_panel_events_go_to_the_owner() {
    let mut frame = mod_host::empty_controls();
    frame.keys_pressed = vec!["Digit1".into(), "KeyQ".into()];
    frame.keys_held = vec!["Digit1".into(), "ShiftLeft".into()];
    frame.events = vec![mod_host::ControlEvent {
        id: "enabled".into(),
        value: 1.0,
    }];
    let later = claim_controls(&frame, &["Digit1".into()], false);
    assert_eq!(later.keys_pressed, ["KeyQ"]);
    assert_eq!(later.keys_held, ["ShiftLeft"]);
    assert!(later.events.is_empty());
    let owner = claim_controls(&frame, &[], true);
    assert_eq!(owner.keys_pressed, frame.keys_pressed);
    assert_eq!(owner.events.len(), 1);
}

#[test]
fn labels_join_in_load_order_within_the_plain_text_limit() {
    assert_eq!(join_labels(&[]), None);
    let labels = ["Lock-on".to_owned(), "FX".to_owned()];
    assert_eq!(join_labels(&labels).as_deref(), Some("Lock-on | FX"));
    let label = join_labels(&["é".repeat(mod_host::MAX_LABEL_BYTES)]).unwrap();
    assert!(label.len() <= mod_host::MAX_LABEL_BYTES && label.chars().all(|c| c == 'é'));
}

#[test]
fn set_files_keep_order_and_per_mod_grants() {
    let directory = tempfile::tempdir().unwrap();
    let set = directory.path().join("mods.json");
    let write = |json: &str| std::fs::write(&set, json).unwrap();
    let camera = directory.path().join("camera.wasm");
    let effects = directory.path().join("effects.wasm");
    write(
        &serde_json::json!({"version":1,"mods":[
            {"component":camera,"grants":{"camera":true,"commands":["ability"]}},
            {"component":effects,"grants":{"environment":true}}
        ]})
        .to_string(),
    );
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, ModSource::Component(camera));
    assert!(mods[0].1.camera && !mods[0].1.environment);
    assert_eq!(mods[0].1.commands, ["ability"]);
    assert!(mods[1].1.environment && !mods[1].1.camera);

    for invalid in [
        r#"{"version":2,"mods":[{"component":"/a.wasm"}]}"#,
        r#"{"version":1,"mods":[]}"#,
        r#"{"version":1,"mods":[{"component":"relative.wasm"}]}"#,
        r#"{"version":1,"mods":[{"component":"/a.wasm","grants":{"root":true}}]}"#,
    ] {
        write(invalid);
        assert!(read_set(&set).is_err(), "{invalid}");
    }
    let many = format!(
        r#"{{"version":1,"mods":[{}]}}"#,
        vec![r#"{"component":"/a.wasm"}"#; MAX_LOADED_MODS + 1].join(",")
    );
    write(&many);
    assert!(read_set(&set).is_err());
}

#[test]
fn set_entries_name_one_component_or_one_package() {
    let directory = tempfile::tempdir().unwrap();
    let set = directory.path().join("mods.json");
    let bei = directory.path().join("bei");
    let lock = directory.path().join("lock.wasm");
    std::fs::write(
        &set,
        serde_json::json!({"version":1,"mods":[
            {"package":bei,"grants":{"screen":true,"items":true}},
            {"component":lock}
        ]})
        .to_string(),
    )
    .unwrap();
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, ModSource::Package(bei.clone()));
    assert!(mods[0].1.screen && mods[0].1.items && !mods[0].1.keys);
    assert_eq!(mods[1].0, ModSource::Component(lock.clone()));
    for invalid in [
        serde_json::json!({"version":1,"mods":[{"package":bei,"component":lock}]}),
        serde_json::json!({"version":1,"mods":[{"grants":{"screen":true}}]}),
    ] {
        std::fs::write(&set, invalid.to_string()).unwrap();
        assert!(read_set(&set).is_err(), "{invalid}");
    }
}

#[test]
fn the_earliest_drawing_package_owns_the_screens() {
    // (may draw, draws) in load order.
    assert_eq!(
        screen_owner([(false, false), (true, false)].into_iter()),
        Some(1)
    );
    assert_eq!(
        screen_owner([(true, false), (true, true), (true, true)].into_iter()),
        Some(1)
    );
    // A mod without the screen grant never owns, whatever it committed.
    assert_eq!(
        screen_owner([(false, true), (true, false)].into_iter()),
        Some(1)
    );
    assert_eq!(screen_owner([(false, false)].into_iter()), None);
}

#[test]
fn a_set_loads_every_valid_component_in_order_and_skips_broken_ones() {
    let directory = tempfile::tempdir().unwrap();
    let broken = directory.path().join("broken.wat");
    std::fs::write(&broken, "(component").unwrap();
    let mut app = bevy::prelude::App::new();
    super::super::configure_set(
        &mut app,
        vec![
            (broken, ModGrants::default()),
            (
                component(directory.path(), "camera.wat"),
                ModGrants {
                    camera: true,
                    ..Default::default()
                },
            ),
            (
                component(directory.path(), "effects.wat"),
                ModGrants {
                    environment: true,
                    ..Default::default()
                },
            ),
        ],
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert_eq!(runtime.host_count(), 2);
    assert!(runtime.host(0).grants().camera);
    assert!(runtime.host(1).grants().environment);
    assert_eq!(runtime.panel_owner(), 0);
    assert!(runtime.reserved_keys().is_empty());
}

/// A guest that labels, sets time, a rig and a command, emits a cue, and checks polled cues.
struct Probe {
    label: &'static str,
    ticks: u32,
    rig_x: f32,
    command: &'static str,
    second_frame_cues: usize,
    extra: &'static str,
}

impl Probe {
    fn new(label: &'static str, ticks: u32, rig_x: f32) -> Self {
        Self {
            label,
            ticks,
            rig_x,
            command: "/ability flash",
            second_frame_cues: 2,
            extra: "",
        }
    }

    fn write(&self, directory: &Path) -> PathBuf {
        let package = include_str!("../../../crates/mod-api/wit/extension.wit")
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("package ")
            .trim_end_matches(';');
        let (name, version) = package.split_once('@').unwrap();
        let source = include_str!("multi_probe.wat")
            .replace("$HUD", &format!("{name}/hud@{version}"))
            .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
            .replace("$GAMEPLAY", &format!("{name}/gameplay@{version}"))
            .replace("$EVENTS", &format!("{name}/events@{version}"))
            .replace("$LABEL_LENGTH", &self.label.len().to_string())
            .replace("$LABEL", self.label)
            .replace("$COMMAND_LENGTH", &self.command.len().to_string())
            .replace("$COMMAND", self.command)
            .replace("$TICKS", &self.ticks.to_string())
            .replace("$RIG_X", &self.rig_x.to_string())
            .replace("$SECOND_FRAME_CUES", &self.second_frame_cues.to_string())
            .replace("$EXTRA", self.extra);
        let path = directory.join(format!("{}.wat", self.label));
        std::fs::write(&path, source).unwrap();
        path
    }
}

fn granted() -> ModGrants {
    ModGrants {
        environment: true,
        camera: true,
        commands: vec!["ability".into()],
        ..Default::default()
    }
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: mod_host::GameplayVector3 {
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

/// Loads `probes` as one set and runs `frames` frames, returning the last merge and failures.
fn run(probes: &[Probe], frames: usize) -> (ModRuntime, Merged, Vec<usize>) {
    let directory = tempfile::tempdir().unwrap();
    let mut app = bevy::prelude::App::new();
    let set = probes
        .iter()
        .map(|probe| (probe.write(directory.path()), granted()))
        .collect();
    super::super::configure_set(&mut app, set);
    let mut runtime = app.world_mut().remove_resource::<ModRuntime>().unwrap();
    let controls = mod_host::empty_controls();
    let mut previous = Vec::new();
    let mut failures = Vec::new();
    let mut merged = Merged::default();
    for _ in 0..frames {
        merged = run_frame(
            &mut runtime,
            FrameInput {
                pressed: false,
                controls: &controls,
                previous_cues: &previous,
                player_state: None,
            },
            |_| (Some(snapshot()), Vec::new(), None),
            |index, _| failures.push(index),
        );
        previous = merged.cues.clone();
    }
    (runtime, merged, failures)
}

#[test]
fn load_order_decides_single_valued_outputs_and_keeps_every_command() {
    let (mut runtime, merged, failures) = run(
        &[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, -0.5)],
        2,
    );
    assert!(failures.is_empty());
    assert_eq!(merged.time_override, Some(1000));
    assert_eq!(merged.rig.unwrap().offset.x, 0.5);
    assert_eq!(runtime.merged_label(), Some("A | B"));
    assert_eq!(merged.commands, ["/ability flash", "/ability flash"]);

    let (mut runtime, merged, _) = run(
        &[Probe::new("B", 6000, -0.5), Probe::new("A", 1000, 0.5)],
        2,
    );
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(runtime.merged_label(), Some("B | A"));
}

#[test]
fn every_mod_polls_all_mods_cues_from_the_previous_frame() {
    // Each probe traps on frame two unless it polls both mods' frame-one cues.
    let (_, merged, failures) = run(&[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, 0.5)], 3);
    assert!(failures.is_empty(), "a probe saw the wrong cues");
    assert_eq!(merged.cues.len(), 2);
}

#[test]
fn a_trapping_mod_is_quarantined_without_disturbing_the_others() {
    let crashing = Probe {
        extra: "unreachable",
        ..Probe::new("A", 1000, 0.5)
    };
    let survivor = Probe {
        second_frame_cues: 1,
        ..Probe::new("B", 6000, -0.5)
    };
    let (mut runtime, merged, failures) = run(&[crashing, survivor], 3);
    assert_eq!(
        failures,
        [0],
        "the trap is reported once, then the guest stays quarantined"
    );
    assert!(!runtime.host(0).is_active() && runtime.host(1).is_active());
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(runtime.merged_label(), Some("B"));
    assert_eq!(merged.commands, ["/ability flash"]);
}

#[test]
fn grants_stay_per_mod() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = bevy::prelude::App::new();
    super::super::configure_set(
        &mut app,
        vec![
            (
                Probe::new("A", 1000, 0.5).write(directory.path()),
                ModGrants::default(),
            ),
            (
                Probe::new("B", 6000, -0.5).write(directory.path()),
                granted(),
            ),
        ],
    );
    let mut runtime = app.world_mut().remove_resource::<ModRuntime>().unwrap();
    let controls = mod_host::empty_controls();
    let merged = run_frame(
        &mut runtime,
        FrameInput {
            pressed: false,
            controls: &controls,
            previous_cues: &[],
            player_state: None,
        },
        |_| (Some(snapshot()), Vec::new(), None),
        |_, error| panic!("{error}"),
    );
    // The ungranted first mod cannot claim time, rig or commands ahead of the granted one.
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(merged.commands, ["/ability flash"]);
}

/// An oversized manifest is rejected after a bounded read, even from a source that never ends.
#[cfg(unix)]
#[test]
fn oversized_set_is_rejected_without_reading_it_whole() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("mods.json");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!(
            "skipping oversized_set_is_rejected_without_reading_it_whole: mkfifo unavailable"
        );
        return;
    }
    let (release, held) = std::sync::mpsc::channel::<()>();
    let writer_path = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut pipe = std::fs::OpenOptions::new()
            .write(true)
            .open(writer_path)
            .unwrap();
        let _ = pipe.write_all(&vec![b' '; MAX_SET_BYTES + 64]);
        // Keeps the pipe open, so an unbounded reader never sees end of file.
        let _ = held.recv();
    });
    let (sender, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || sender.send(read_set(&fifo)));
    let outcome = result.recv_timeout(std::time::Duration::from_secs(10));
    drop(release);
    writer.join().unwrap();
    let error = outcome
        .expect("the manifest read was unbounded")
        .unwrap_err();
    assert!(error.contains("byte limit"), "{error}");
}

#[test]
fn the_merged_label_is_rebuilt_only_when_a_mod_label_changes() {
    let (mut runtime, _, _) = run(
        &[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, -0.5)],
        1,
    );
    assert_eq!(runtime.merged_label(), Some("A | B"));
    let built = runtime.label_rebuilds;
    for _ in 0..3 {
        assert_eq!(runtime.merged_label(), Some("A | B"));
    }
    assert_eq!(
        runtime.label_rebuilds, built,
        "unchanged labels must not be rejoined"
    );
}
