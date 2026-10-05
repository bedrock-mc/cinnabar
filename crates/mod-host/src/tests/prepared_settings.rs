use super::*;

fn settings_guest(init: &str, frame: &str) -> String {
    let source = include_str!("../../../mod-api/wit/extension.wit");
    let package = source
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let save = "i32.const 2048 i32.const $SIZE i32.const 512 call $save"
        .replace("$SIZE", &frame.len().to_string());
    fixture(&save, "Hello")
        .replacen("(component", &format!("(component (import \"{name}/settings@{version}\" (instance $settings (export \"save\" (func (param \"json\" string) (result (result (error string))))))) (alias export $settings \"save\" (func $set-settings))"), 1)
        .replace("(core func $lower-label", "(core func $lower-settings (canon lower (func $set-settings) (memory $memory) (realloc $realloc))) (core func $lower-label")
        .replace("(import \"host\" \"time\"", "(import \"host\" \"save\" (func $save (param i32 i32 i32))) (import \"host\" \"time\"")
        .replace("(export \"time\" (func $lower-time))", "(export \"save\" (func $lower-settings)) (export \"time\" (func $lower-time))")
        .replace("(data (i32.const 0)", &format!("(data (i32.const 1024) \"{}\") (data (i32.const 2048) \"{}\") (data (i32.const 0)", init.replace('"', "\\22"), frame.replace('"', "\\22")))
        .replacen("(func (export \"init\")", &format!("(func (export \"init\") i32.const 1024 i32.const {} i32.const 512 call $save", init.len()), 1)
}

#[test]
fn unaccepted_candidate_never_persists_committed_guest_init_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected.component.wat");
    let companion = path.with_extension("settings.json");
    std::fs::write(&companion, "{\"cps\":12}").unwrap();
    let source = settings_guest("{\"cps\":25}", "{\"cps\":30}");
    let candidate = ModHost::prepare_snapshot_with_grants(
        &path,
        source.as_bytes(),
        ModGrants {
            settings: true,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(candidate.settings_snapshot(), Some("{\"cps\":25}"));
    assert_eq!(candidate.instance.settings_write(), Some("{\"cps\":25}"));
    drop(candidate);
    assert_eq!(std::fs::read_to_string(companion).unwrap(), "{\"cps\":12}");
}

#[test]
fn accepted_successor_uses_live_seed_and_old_retirement_cannot_overwrite_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected.component.wat");
    let companion = path.with_extension("settings.json");
    std::fs::write(&path, settings_guest("{\"cps\":12}", "{\"cps\":30}")).unwrap();
    let grants = ModGrants {
        settings: true,
        ..Default::default()
    };
    let mut previous = ModHost::load_with_grants(&path, grants).unwrap();
    previous.frame(false).unwrap();
    assert_eq!(previous.settings_snapshot(), Some("{\"cps\":30}"));
    std::fs::write(&companion, "{\"cps\":1}").unwrap();
    let source = settings_guest("{\"cps\":40}", "{\"cps\":50}");
    let mut accepted = ModHost::prepare_snapshot_with_grants(
        &path,
        source.as_bytes(),
        grants,
        Some((
            previous.settings_path().unwrap(),
            previous.settings_snapshot().unwrap(),
        )),
    )
    .unwrap();
    assert_eq!(accepted.settings_seed(), Some("{\"cps\":30}"));
    accepted.activate_settings(Some(&mut previous));
    accepted.frame(false).unwrap();
    // A retired host cannot submit through the transferred writer, even if called accidentally.
    previous.frame(false).unwrap();
    drop(previous);
    drop(accepted);
    assert_eq!(std::fs::read_to_string(companion).unwrap(), "{\"cps\":50}");
}
