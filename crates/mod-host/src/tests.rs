#[path = "tests/camera.rs"]
mod camera;
use super::*;
mod block_highlights;
mod fullbright;
mod gameplay;
mod item_use;
mod prepared_settings;
mod render;
mod screens;
mod world;

/// Builds a tiny component with the same canonical imports as the guest SDK.
fn fixture(frame: &str, text: &str) -> String {
    let source = include_str!("../../mod-api/wit/extension.wit");
    let package = source
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("tests/guest.wat")
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
        .replace("$INPUT", &format!("{name}/input@{version}"))
        .replace("$TEXT", text)
        .replace("$LENGTH", &text.len().to_string())
        .replace("$FRAME", frame)
}

/// Writes a disposable component source accepted by the host's WAT loader.
fn load_fixture(frame: &str) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wat");
    std::fs::write(&path, fixture(frame, "Hello")).unwrap();
    let host = ModHost::load(&path).unwrap();
    (directory, host)
}

#[test]
fn retained_label_and_keybind_cross_real_component_boundary() {
    let (_directory, mut host) =
        load_fixture("call $pressed if i32.const 1 i32.const 4 i32.const 128 call $label end");
    assert_eq!(host.label(), Some("Hello"));
    host.frame(false).unwrap();
    assert_eq!(host.label(), Some("Hello"));
    host.frame(true).unwrap();
    assert_eq!(host.label(), Some("ello"));
}

#[test]
fn trap_discards_staged_output_and_quarantines_only_this_instance() {
    let (_directory, mut host) =
        load_fixture("i32.const 1 i32.const 4 i32.const 128 call $label unreachable");
    let (_other_directory, mut healthy) = load_fixture("");
    assert!(host.frame(false).is_err());
    assert!(!host.is_active());
    assert!(host.label().is_none());
    host.frame(false).unwrap();
    healthy.frame(false).unwrap();
    assert_eq!(healthy.label(), Some("Hello"));
}

#[test]
fn fuel_stops_an_infinite_loop() {
    let (_directory, mut host) = load_fixture("(loop $forever br $forever)");
    assert!(host.frame(false).is_err());
    assert!(!host.is_active());
}

#[test]
fn excessive_hud_calls_trap_before_publishing() {
    let (_directory, mut host) =
        load_fixture("(loop $spam i32.const 0 i32.const 5 i32.const 128 call $label br $spam)");
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("HUD import budget exhausted"));
    assert!(host.label().is_none());
}

#[test]
fn memory_growth_is_bounded() {
    let (_directory, mut host) = load_fixture("i32.const 1024 memory.grow drop");
    assert!(host.frame(false).is_err());
    assert!(!host.is_active());
}

#[test]
fn reload_is_transactional_and_recovers_quarantined_guests() {
    let (directory, mut host) = load_fixture("unreachable");
    let path = directory.path().join("mod.wat");
    assert!(!host.reload_if_changed().unwrap());
    std::fs::write(&path, "broken").unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_eq!(host.label(), Some("Hello"));
    assert!(!host.reload_if_changed().unwrap());
    host.frame(false).unwrap_err();
    std::fs::write(&path, fixture("", "Reloaded")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert!(host.is_active());
    assert_eq!(host.label(), Some("Reloaded"));
}

#[test]
fn unknown_authority_and_oversized_packages_fail_admission() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.wat");
    let source =
        fixture("", "Hello").replacen("(component", "(component (import \"network\" (func))", 1);
    std::fs::write(&path, source).unwrap();
    assert!(ModHost::load(&path).is_err());
    let file = std::fs::File::create(&path).unwrap();
    file.set_len((MAX_COMPONENT_BYTES + 1) as u64).unwrap();
    assert!(ModHost::load(&path).is_err());
}

#[test]
fn oversized_initial_label_is_ignored() {
    let text = "x".repeat(MAX_LABEL_BYTES + 1);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.wat");
    std::fs::write(&path, fixture("", &text)).unwrap();
    let host = ModHost::load(&path).unwrap();
    assert!(host.label().is_none());
}

#[test]
fn invalid_updates_keep_the_committed_label() {
    for mutation in [
        "i32.const 0 i32.const 10 i32.store8",
        "i32.const 0 i32.const 194 i32.store8 i32.const 1 i32.const 167 i32.store8",
    ] {
        let (_directory, mut host) = load_fixture(&format!(
            "{mutation} i32.const 0 i32.const 5 i32.const 128 call $label"
        ));
        host.frame(false).unwrap();
        assert_eq!(host.label(), Some("Hello"));
    }
}

#[test]
fn initialization_and_core_start_share_the_fuel_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("loop.wat");
    for source in [
        fixture("", "Hello").replacen(
            "(export \"init\")",
            "(export \"init\") (loop $forever br $forever)",
            1,
        ),
        fixture("", "Hello").replacen(
            "(func (export \"frame\")",
            "(func $start (loop $forever br $forever)) (start $start) (func (export \"frame\")",
            1,
        ),
    ] {
        std::fs::write(&path, source).unwrap();
        let error = ModHost::load(&path).err().expect("startup must be bounded");
        assert!(format!("{error:#}").contains("fuel"), "{error:#}");
    }
}

#[test]
fn trapping_reload_initialization_retains_previous_output_and_instance() {
    let (directory, mut host) = load_fixture("");
    let candidate =
        fixture("", "Candidate").replacen("(export \"init\")", "(export \"init\") unreachable", 1);
    std::fs::write(directory.path().join("mod.wat"), candidate).unwrap();
    let error = host.reload_if_changed().unwrap_err();
    assert!(format!("{error:#}").contains("unreachable"), "{error:#}");
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.label(), Some("Hello"));
}

/// Runs the environment import and checks its canonical result discriminant.
fn time_call(ticks: Option<u32>, denied: bool) -> String {
    format!(
        "i32.const {} i32.const {} i32.const 128 call $time \
         i32.const 128 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(ticks.is_some()),
        ticks.unwrap_or_default(),
        u8::from(denied)
    )
}

/// Loads a component with an explicit environment grant and optional init write.
fn environment_fixture(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wat");
    let source = fixture(frame, "Time").replacen(
        "(export \"init\")",
        &format!("(export \"init\") {init}"),
        1,
    );
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            environment: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

#[test]
fn environment_grant_sets_and_none_clears_across_component_boundary() {
    let frame = format!(
        "call $pressed if {} else {} end",
        time_call(Some(1_000), false),
        time_call(None, false)
    );
    let (_directory, mut host) = environment_fixture("", &frame, true);
    host.frame(true).unwrap();
    assert_eq!(host.time_override(), Some(1_000));
    host.frame(false).unwrap();
    assert_eq!(host.time_override(), None);
}

#[test]
fn environment_denial_returns_error_without_quarantining_guest() {
    for ticks in [None, Some(1_000)] {
        let (_directory, mut host) = environment_fixture("", &time_call(ticks, true), false);
        host.frame(false).unwrap();
        assert_eq!(host.time_override(), None);
        assert!(host.is_active());
    }
}

#[test]
fn environment_rejects_out_of_range_and_accepts_both_boundaries() {
    for ticks in [mod_api::BEDROCK_DAY_TICKS, u32::MAX] {
        let (_directory, mut host) = environment_fixture(
            &time_call(Some(0), false),
            &time_call(Some(ticks), true),
            true,
        );
        host.frame(false).unwrap();
        assert_eq!(host.time_override(), Some(0));
    }
    let last = mod_api::BEDROCK_DAY_TICKS - 1;
    let (_directory, mut host) = environment_fixture("", &time_call(Some(last), false), true);
    host.frame(false).unwrap();
    assert_eq!(host.time_override(), Some(last));
}

#[test]
fn environment_trap_revokes_override_and_discards_pending_write() {
    let frame = format!("{} unreachable", time_call(Some(1_000), false));
    let (_directory, mut host) = environment_fixture(&time_call(Some(18_000), false), &frame, true);
    assert_eq!(host.time_override(), Some(18_000));
    assert!(host.frame(false).is_err());
    assert_eq!(host.time_override(), None);
}

#[test]
fn environment_import_budget_is_bounded() {
    let frame = format!("(loop $spam {} br $spam)", time_call(None, false));
    let (_directory, mut host) = environment_fixture("", &frame, true);
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("environment import budget exhausted"));
    assert_eq!(host.time_override(), None);
}

#[test]
fn environment_reload_preserves_grants_and_resets_committed_override() {
    let (directory, mut host) = environment_fixture(&time_call(Some(1_000), false), "", true);
    let path = directory.path().join("mod.wat");
    std::fs::write(&path, "broken").unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_eq!(host.time_override(), Some(1_000));
    std::fs::write(&path, fixture(&time_call(Some(18_000), false), "Reloaded")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.time_override(), None);
    host.frame(false).unwrap();
    assert_eq!(host.time_override(), Some(18_000));
}

#[test]
fn default_loader_denies_environment_authority() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wat");
    std::fs::write(&path, fixture(&time_call(Some(1_000), true), "Time")).unwrap();
    let mut host = ModHost::load(&path).unwrap();
    host.frame(false).unwrap();
    assert_eq!(host.time_override(), None);
    assert!(host.is_active());
}

#[test]
fn startup_loads_companion_and_reload_keeps_current_in_memory_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected.component.wat");
    let companion = path.with_extension("settings.json");
    std::fs::write(&path, fixture("", "Initial")).unwrap();
    std::fs::write(&companion, "{\"cps\":20}").unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(host.instance.settings(), "{\"cps\":20}");
    // The pending writer may leave an older disk value while current settings are committed.
    std::fs::write(&companion, "{\"cps\":12}").unwrap();
    std::fs::write(&path, fixture("", "Reloaded")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.instance.settings(), "{\"cps\":20}");
    assert_eq!(host.label(), Some("Reloaded"));
}
