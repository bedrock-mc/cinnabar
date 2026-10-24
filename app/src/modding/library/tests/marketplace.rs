//! The exact browser/API component enters the existing Available, enable and consent controls.

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

#[test]
fn website_component_is_available_without_grants_then_runs_with_explicit_consent() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../crates/launcher-host/src/marketplace/fixtures/website-send.json"
    ))
    .unwrap();
    let bytes = STANDARD
        .decode(fixture["artifact"].as_str().unwrap())
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_str(fixture["release_response"].as_str().unwrap()).unwrap();
    let manifest: launcher_host::marketplace::Manifest =
        serde_json::from_str(response["release"]["statement"].as_str().unwrap()).unwrap();
    mod_host::validate_source(&bytes).unwrap();
    crate::modding::marketplace::declarations(&bytes, &manifest).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let registration = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join("marketplace-synthetic.wasm");
    fs::write(&path, &bytes).unwrap();
    let library = scan(&mods, &registration, String::new());
    assert!(library.active.is_empty());
    assert!(registered(&registration).is_empty());
    let item = &library.installed[0];
    let id = item.id;
    let hud = item
        .permissions
        .iter()
        .find(|permission| permission.name == "HUD")
        .unwrap();
    assert_eq!(hud.requested, Some(true));
    assert!(!hud.enabled);
    let mut requests = 0;
    let mut act = |action| {
        perform(
            action,
            &mods,
            &registration,
            &mut requests,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap()
    };
    act(Action::Enable(id));
    assert_eq!(
        registered(&registration)[0].entry.grants,
        mod_host::ModGrants::default()
    );
    let capability = mod_host::ModGrants::CAPABILITIES
        .iter()
        .position(|value| value.name == "hud")
        .unwrap();
    act(Action::Permission(id, capability, true));
    let granted = registered(&registration)[0].entry.grants.clone();
    assert!(granted.hud);
    let mut host = ModHost::load_snapshot_with_grants(&path, &bytes, granted).unwrap();
    host.frame(false).unwrap();
    assert_eq!(host.label(), Some("Hello"));
    act(Action::Disable(id));
    assert!(!registered(&registration)[0].entry.enabled);
}
