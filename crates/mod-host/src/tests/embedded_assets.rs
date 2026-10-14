use super::*;

/// Adds a top-level source section using the SDK's generic static-resource names.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = vec![name.len() as u8];
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    let mut size = payload.len();
    loop {
        let byte = (size & 0x7f) as u8;
        size >>= 7;
        bytes.push(byte | if size == 0 { 0 } else { 0x80 });
        if size == 0 {
            break;
        }
    }
    bytes.extend(payload);
}

/// Guest init opens the existing panel; it succeeds only if loading prepared it first.
fn component(title: &str) -> Vec<u8> {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (namespace, version) = package.split_once('@').unwrap();
    let source = fixture("", "Hello")
        .replacen("(component", &format!("(component (import \"{namespace}/panel@{version}\" (instance $panel (export \"set-open\" (func (param \"open\" bool) (result (result (error string))))))) (alias export $panel \"set-open\" (func $set-open))"), 1)
        .replace("(core func $lower-label", "(core func $lower-open (canon lower (func $set-open) (memory $memory) (realloc $realloc))) (core func $lower-label")
        .replace("(import \"host\" \"time\"", "(import \"host\" \"open\" (func $open (param i32 i32))) (import \"host\" \"time\"")
        .replace("(export \"time\" (func $lower-time))", "(export \"open\" (func $lower-open)) (export \"time\" (func $lower-time))")
        .replacen("(func (export \"init\")", "(func (export \"init\") i32.const 1 i32.const 256 call $open", 1);
    let mut bytes = wat::parse_str(source).unwrap();
    let bundle = serde_json::json!({"panel":{"title":title,"toggle_key":"ShiftRight","dark":true,"controls":[]},"surfaces":{}}).to_string();
    section(&mut bytes, mod_api::PANEL_BUNDLE_SECTION, bundle.as_bytes());
    section(&mut bytes, mod_api::PANEL_FONT_SECTION, title.as_bytes());
    bytes
}

#[test]
fn embedded_bundle_is_available_to_init_without_granting_denied_controls() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let source = component("Local");
    ModHost::validate_component(&source).unwrap();
    let granted = ModGrants {
        controls: true,
        ..Default::default()
    };
    let host = ModHost::load_snapshot_with_grants(&path, &source, granted).unwrap();
    assert_eq!(host.panel().unwrap().title, "Local");
    assert!(host.panel_open(), "init saw the preloaded panel");
    assert_eq!(host.panel_font_source(), Some(b"Local".as_slice()));
    let denied = ModHost::load_snapshot_with_grants(&path, &source, ModGrants::default()).unwrap();
    assert!(denied.panel().is_none());
    assert!(!denied.panel_open());
    assert!(denied.panel_font_source().is_none());
    assert!(!denied.grants().controls);
}

#[test]
fn reload_changes_both_embedded_assets_and_invalid_candidates_keep_previous() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    std::fs::write(&path, component("First")).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::write(&path, component("Second")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.panel().unwrap().title, "Second");
    assert_eq!(host.panel_font_source(), Some(b"Second".as_slice()));
    assert!(host.panel_open());
    let mut invalid = component("Invalid");
    section(&mut invalid, mod_api::PANEL_FONT_SECTION, b"duplicate");
    std::fs::write(&path, invalid).unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_eq!(host.panel().unwrap().title, "Second");
    assert_eq!(host.panel_font_source(), Some(b"Second".as_slice()));
}
