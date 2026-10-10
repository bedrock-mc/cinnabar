use super::*;
const CONTENT: &str = r#"{"cards":[{"id":"equipment","title":"Equipment","rows":[{"label":"Helmet","value":"85%","item":"minecraft:diamond_helmet","progress":0.85}]}]}"#;
const CROSSHAIR: &str = r#"{"shape":"circle","color":[0.3,1,0.5,1]}"#;
fn escape(text: &str) -> String {
    text.as_bytes()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect()
}
fn source(content: &str, crosshair: &str, init: &str, frame: &str, error: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("hud.wat")
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$CONTENT_LENGTH", &content.len().to_string())
        .replace("$CROSSHAIR_LENGTH", &crosshair.len().to_string())
        .replace("$CONTENT", &escape(content))
        .replace("$CROSSHAIR", &escape(crosshair))
        .replace("$INIT", init)
        .replace("$FRAME", frame)
        .replace("$ERROR", if error { "1" } else { "0" })
}
fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hud.wat");
    std::fs::write(&path, source(CONTENT, CROSSHAIR, init, frame, !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            hud: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (dir, host)
}
#[test]
fn retained_presentation_requires_its_own_authority() {
    let (_dir, mut host) = load(
        "call $cards call $cursor",
        "call $cards call $cursor",
        false,
    );
    host.frame(false).unwrap();
    assert!(host.hud().is_none());
    assert!(host.crosshair().is_none());
    assert!(host.is_active());
}
#[test]
fn cards_and_cursor_commit_retain_and_clear_together() {
    let (_dir, mut host) = load("call $cards call $cursor", "", true);
    let hud = host.hud().unwrap().clone();
    let cursor = host.crosshair().unwrap().clone();
    host.frame(false).unwrap();
    assert_eq!(host.hud(), Some(&hud));
    assert_eq!(host.crosshair(), Some(&cursor));
    let (_dir, mut host) = load("call $cards call $cursor", "call $clear", true);
    host.frame(false).unwrap();
    assert!(host.hud().is_none());
    assert!(host.crosshair().is_none());
}
#[test]
fn trap_revokes_staged_and_retained_presentation() {
    for init in ["", "call $cards call $cursor"] {
        let (_dir, mut host) = load(init, "call $cards call $cursor unreachable", true);
        assert!(host.frame(false).is_err());
        assert!(!host.is_active());
        assert!(host.hud().is_none());
        assert!(host.crosshair().is_none());
    }
}
#[test]
fn invalid_payload_does_not_replace_committed_presentation() {
    let invalid_frame = "i32.const 0 i32.const 1 i32.const 16384 call $content
        i32.const 16384 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 4096 i32.const 1 i32.const 16384 call $crosshair
        i32.const 16384 i32.load8_u i32.const 1 i32.ne if unreachable end";
    let (_dir, mut host) = load("call $cards call $cursor", invalid_frame, true);
    let hud = host.hud().unwrap().clone();
    let cursor = host.crosshair().unwrap().clone();
    host.frame(false).unwrap();
    assert_eq!(host.hud(), Some(&hud));
    assert_eq!(host.crosshair(), Some(&cursor));
}
#[test]
fn successful_reload_drops_removed_surfaces_and_rejected_reload_keeps_them() {
    let (dir, mut host) = load("call $cards call $cursor", "", true);
    std::fs::write(
        dir.path().join("hud.wat"),
        source(CONTENT, CROSSHAIR, "unreachable", "", false),
    )
    .unwrap();
    assert!(host.reload_if_changed().is_err());
    assert!(host.hud().is_some());
    assert!(host.crosshair().is_some());
    std::fs::write(
        dir.path().join("hud.wat"),
        source(CONTENT, CROSSHAIR, "", "", false),
    )
    .unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert!(host.hud().is_none());
    assert!(host.crosshair().is_none());
}
#[test]
fn presentation_writes_share_the_label_budget() {
    let (_dir, mut host) = load("", &"call $cards call $cursor ".repeat(5), true);
    assert!(host.frame(false).is_err());
    assert!(host.hud().is_none());
    assert!(host.crosshair().is_none());
}
