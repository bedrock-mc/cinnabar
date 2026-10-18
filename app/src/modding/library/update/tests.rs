use super::*;
use crate::modding::registration::selection;

const SOURCE: &str = "https://github.com/example/zoom/releases/latest/download/zoom.wasm";

/// Builds a valid host-world component with versioned, root-level update metadata.
fn component(version: &str, source: &str, valid: bool) -> Vec<u8> {
    let code = if valid {
        "(component (core module $m (func (export \"init\")) (func (export \"frame\"))) (core instance $i (instantiate $m)) (func (export \"init\") (canon lift (core func $i \"init\"))) (func (export \"frame\") (canon lift (core func $i \"frame\"))))"
    } else {
        "(component)"
    };
    let mut bytes = wat::parse_str(code).unwrap();
    for (name, data) in [
        ("version", version),
        (mod_api::UPDATE_URL_SECTION, source),
        (
            mod_host::REQUESTED_GRANTS_SECTION,
            "{\"camera\":true,\"settings\":true}",
        ),
    ] {
        section(&mut bytes, name, data.as_bytes());
    }
    bytes
}

/// Appends a custom section without depending on a second production encoder.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    fn length(mut value: usize, bytes: &mut Vec<u8>) {
        loop {
            let byte = (value & 127) as u8;
            value >>= 7;
            bytes.push(byte | if value == 0 { 0 } else { 128 });
            if value == 0 {
                break;
            }
        }
    }
    let mut payload = Vec::new();
    length(name.len(), &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    length(payload.len(), bytes);
    bytes.extend(payload);
}

#[test]
fn automatic_install_preserves_registration_settings_and_existing_authority() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    fs::create_dir(&mods).unwrap();
    let path = mods.join("renamed.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    let config = directory.path().join("config");
    selection::select(&config, &path, true, "enabled".into()).unwrap();
    let before = selection::registered(&config).remove(0).entry;
    let companion = mod_host::settings_companion(&path);
    fs::write(&companion, b"{\"volume\":17}").unwrap();
    let new = component("1.1.0", SOURCE, true);
    let download = Downloaded::fixture(
        installed(&mods)[0].0.id,
        "v1.1.0",
        "example/zoom",
        "zoom.wasm",
        &new,
    );
    let message = install(&mods, &download).unwrap();
    assert!(message.contains("Updated renamed.wasm to v1.1.0"));
    assert_eq!(fs::read(&path).unwrap(), new);
    assert_eq!(selection::registered(&config)[0].entry, before);
    assert_eq!(fs::read(&companion).unwrap(), b"{\"volume\":17}");
    assert_eq!(fs::read_dir(&mods).unwrap().count(), 2);
}

#[test]
fn wrong_versions_sources_invalid_components_and_cancellation_leave_original_intact() {
    for (version, source, valid, cancel) in [
        ("1.0.0", SOURCE, true, false),
        ("1.0.1", SOURCE, true, false),
        (
            "1.1.0",
            "https://github.com/other/zoom/releases/latest/download/zoom.wasm",
            true,
            false,
        ),
        ("1.1.0", SOURCE, false, false),
        ("1.1.0", SOURCE, true, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("zoom.wasm");
        let original = component("1.0.0", SOURCE, true);
        fs::write(&path, &original).unwrap();
        let download = Downloaded::fixture(
            installed(directory.path())[0].0.id,
            "1.1.0",
            "example/zoom",
            "zoom.wasm",
            &component(version, source, valid),
        );
        if cancel {
            download.cancel_fixture();
        }
        assert!(
            install(directory.path(), &download).is_err(),
            "{version}, {source}, {valid}, {cancel}"
        );
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn replaced_or_removed_sources_cannot_receive_a_stale_download() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("zoom.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    let download = Downloaded::fixture(
        installed(directory.path())[0].0.id,
        "1.1.0",
        "example/zoom",
        "zoom.wasm",
        &component("1.1.0", SOURCE, true),
    );
    fs::write(&path, b"replacement selected by the user").unwrap();
    assert!(install(directory.path(), &download).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        b"replacement selected by the user"
    );
    fs::remove_file(&path).unwrap();
    assert!(install(directory.path(), &download).is_err());
    assert!(!path.exists());
}
