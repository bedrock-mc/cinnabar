use super::*;

fn source(init: &str, frame: &str, denied: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("block_highlights.wat")
        .replace("$RENDER", &format!("{name}/render@{version}"))
        .replace("$ERROR", if denied { "1" } else { "0" })
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}
fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("highlight.wat");
    std::fs::write(&path, source(init, frame, !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            block_highlights: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}
#[test]
fn block_highlights_cross_the_component_boundary_and_require_permission() {
    let (_directory, mut denied) = load("call $selection", "call $selection", false);
    denied.frame(false).unwrap();
    assert!(denied.block_highlights().is_none());
    let (_directory, mut host) = load("call $selection", "", true);
    let selected = host.block_highlights().unwrap().clone();
    assert_eq!(selected.identifiers, ["minecraft:ancient_debris"]);
    host.frame(false).unwrap();
    assert_eq!(host.block_highlights(), Some(&selected));
}
#[test]
fn block_highlights_reset_on_trap_and_successful_reload() {
    let (_directory, mut host) = load("call $selection", "call $clear unreachable", true);
    assert!(host.block_highlights().is_some());
    assert!(host.frame(false).is_err());
    assert!(!host.is_active());
    assert!(host.block_highlights().is_none());
    let (_directory, mut host) = load("call $selection", "", true);
    std::fs::write(
        _directory.path().join("highlight.wat"),
        source("", "", false),
    )
    .unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert!(host.block_highlights().is_none());
}
#[test]
fn block_highlights_do_not_publish_before_a_callback_succeeds() {
    let (_directory, mut host) = load("", "call $selection unreachable", true);
    assert!(host.frame(false).is_err());
    assert!(host.block_highlights().is_none());
    let (_directory, mut host) = load("call $selection", "call $clear", true);
    host.frame(false).unwrap();
    assert!(host.block_highlights().is_none());
}
