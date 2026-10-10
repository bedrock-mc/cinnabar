use super::*;

fn source(init: &str, frame: &str, denied: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("fullbright.wat")
        .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
        .replace("$ERROR", if denied { "1" } else { "0" })
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}
fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fullbright.wat");
    std::fs::write(&path, source(init, frame, !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            fullbright: granted,
            environment: true,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}
#[test]
fn fullbright_is_denied_even_with_visual_time_authority() {
    let (_directory, mut host) = load("call $on", "call $off", false);
    host.frame(false).unwrap();
    assert!(!host.fullbright());
    assert!(host.is_active());
}
#[test]
fn fullbright_commits_after_success_and_retains_without_republication() {
    let (_directory, mut host) = load("", "call $on", true);
    assert!(!host.fullbright());
    host.frame(false).unwrap();
    assert!(host.fullbright());
    let (_directory, mut host) = load("call $on", "", true);
    assert!(host.fullbright());
    host.frame(false).unwrap();
    assert!(host.fullbright());
}
#[test]
fn fullbright_off_restores_default_after_success() {
    let (_directory, mut host) = load("call $on", "call $off", true);
    assert!(host.fullbright());
    host.frame(false).unwrap();
    assert!(!host.fullbright());
}
#[test]
fn fullbright_does_not_publish_failed_callback_output_and_revokes_on_trap() {
    let (_directory, mut host) = load("", "call $on unreachable", true);
    assert!(host.frame(false).is_err());
    assert!(!host.fullbright());
    assert!(!host.is_active());
    let (_directory, mut host) = load("call $on", "call $on unreachable", true);
    assert!(host.fullbright());
    assert!(host.frame(false).is_err());
    assert!(!host.fullbright());
}
#[test]
fn fullbright_resets_on_successful_reload_and_survives_rejected_reload() {
    let (_directory, mut host) = load("call $on", "", true);
    std::fs::write(
        _directory.path().join("fullbright.wat"),
        source("unreachable", "", false),
    )
    .unwrap();
    assert!(host.reload_if_changed().is_err());
    assert!(host.fullbright());
    std::fs::write(
        _directory.path().join("fullbright.wat"),
        source("", "", false),
    )
    .unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert!(!host.fullbright());
}
#[test]
fn fullbright_shares_the_environment_callback_budget_with_time_updates() {
    let frame = format!("{} {}", "call $on ".repeat(4), "call $clock ".repeat(4));
    let (_directory, mut host) = load("", &frame, true);
    host.frame(false).unwrap();
    assert!(host.fullbright());
    host.frame(false).unwrap();
    assert!(host.fullbright(), "the budget resets each frame");
    let frame = format!("{frame} call $off");
    let (_directory, mut host) = load("", &frame, true);
    assert!(host.frame(false).is_err());
    assert!(!host.fullbright());
    assert!(host.time_override().is_none());
}
#[test]
fn fullbright_permission_is_independent_of_time_and_render() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fullbright.wat");
    std::fs::write(&path, source("call $on", "", false)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            fullbright: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(host.fullbright());
    assert!(!host.grants().environment);
    assert!(!host.grants().render);
}
