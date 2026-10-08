use crate::common;

use std::fs;
use std::path::Path;

use common::{
    client_message, current_api, edit_manifest, epoch, focused, hello_wasm, interact, p, probe_dir,
    probe_dir_with, probe_wasm, rehash, send, tell, v0_1_dir, v0_2_dir, v0_3_dir,
};
use experience_runtime::callback::run;
use experience_runtime::limits::{MAX_COMPONENT_BYTES, MAX_MANIFEST_BYTES, MAX_VERSION_BYTES};
use experience_runtime::load::{engine, load};
use experience_runtime::manifest::{
    ASSETS_DIR, CLIENT_TABLE, MANIFEST_FILE, SERVER_WASM, read_manifest,
};
use experience_runtime::protocol::{BlockDef, Mining, Outcome, Scalar, Texture};
use tempfile::TempDir;

/// Loads `dir`, which must fail, and returns the error chain. Every load error names the
/// directory.
fn refusal(dir: &Path) -> String {
    let (engine, _ticker) = engine().unwrap();
    let error = match load(&engine, dir) {
        Ok(_) => panic!("{} loaded", dir.display()),
        Err(error) => format!("{error:#}"),
    };
    let shown = dir.display().to_string();
    assert!(
        error.contains(&shown),
        "error does not name {shown}: {error}"
    );
    error
}

/// A probe artifact whose `server.wasm` is `bytes`, with the index following.
fn with_server_wasm(bytes: Vec<u8>) -> TempDir {
    probe_dir_with(|dir| {
        fs::write(dir.join(SERVER_WASM), bytes).unwrap();
        rehash(dir);
    })
}

/// The probe's core module with a custom section appended so it is exactly `len` bytes.
fn padded_probe(len: usize) -> Vec<u8> {
    const NAME: &[u8] = b"padding";
    let mut module = probe_wasm().to_vec();
    // Section id 0, its size as a 5-byte LEB128, the name; zeros fill the rest.
    let size = u32::try_from(len - module.len() - 6).unwrap();
    module.push(0);
    for shift in [0, 7, 14, 21] {
        module.push(0x80 | ((size >> shift) & 0x7f) as u8);
    }
    module.push((size >> 28) as u8);
    module.push(NAME.len() as u8);
    module.extend_from_slice(NAME);
    module.resize(len, 0);
    module
}

/// Pads `experience.toml` with a trailing comment so it is exactly `len` bytes.
fn pad_manifest(dir: &Path, len: usize) {
    let path = dir.join(MANIFEST_FILE);
    let mut text = fs::read_to_string(&path).unwrap();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('#');
    text.push_str(&"x".repeat(len - text.len()));
    fs::write(&path, text).unwrap();
}

#[test]
fn probe_registers_counter_block() {
    let dir = probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert_eq!(loaded.manifest.id, "probe");
    assert_eq!(loaded.manifest.version, "0.1.0");
    let texture = dir.path().join(ASSETS_DIR).join("counter.png");
    assert_eq!(
        loaded.blocks,
        vec![BlockDef {
            id: "probe:counter".to_owned(),
            display_name: "Probe Counter".to_owned(),
            textures: vec![Texture {
                slot: "*".to_owned(),
                path: texture.to_str().unwrap().to_owned(),
            }],
            mining: Mining::Breakable { hardness: 1.0 },
        }]
    );
}

#[test]
fn foreign_namespace_is_refused() {
    let dir = probe_dir_with(|dir| {
        edit_manifest(dir, |manifest| {
            manifest.insert("id".to_owned(), "other".into());
        });
    });
    let error = refusal(dir.path());
    assert!(
        error.contains("probe:counter") && error.contains("namespace \"other:\""),
        "{error}"
    );
}

#[test]
fn hash_mismatch_is_refused() {
    let dir = probe_dir_with(|dir| {
        fs::write(dir.join(ASSETS_DIR).join("counter.png"), b"other bytes").unwrap();
    });
    let error = refusal(dir.path());
    let file = format!("{ASSETS_DIR}/counter.png");
    assert!(
        error.contains("hash mismatch") && error.contains(&file),
        "{error}"
    );
}

#[test]
fn unindexed_file_is_refused() {
    let dir = probe_dir_with(|dir| {
        fs::write(dir.join(ASSETS_DIR).join("unlisted.txt"), b"x").unwrap();
    });
    let error = refusal(dir.path());
    let unindexed = format!("unindexed file {ASSETS_DIR}/unlisted.txt");
    assert!(error.contains(&unindexed), "{error}");
}

#[test]
fn escaping_path_is_refused() {
    let escaping = [
        "../escape.txt",
        "assets/../../escape.txt",
        "/escape.txt",
        "C:/escape.txt",
        "..\\escape.txt",
    ];
    for key in escaping {
        let dir = probe_dir_with(|dir| {
            edit_manifest(dir, |manifest| {
                let files = manifest["files"].as_table_mut().unwrap();
                files.insert(key.to_owned(), "0".repeat(64).into());
            });
        });
        let error = refusal(dir.path());
        assert!(
            error.contains(&format!("invalid path \"{key}\"")),
            "{key}: {error}"
        );
    }
}

#[test]
fn wrong_api_is_refused() {
    let dir = probe_dir_with(|dir| {
        edit_manifest(dir, |manifest| {
            manifest.insert("api".to_owned(), "0.0".into());
        });
    });
    let error = refusal(dir.path());
    assert!(error.contains("unsupported api \"0.0\""), "{error}");
}

/// The probe's manifest declares a client part, which the runtime ignores whatever it holds; an
/// unknown key outside it is still refused.
#[test]
fn only_the_client_table_is_ignored() {
    let anything = probe_dir_with(|dir| {
        edit_manifest(dir, |manifest| {
            let client = manifest[CLIENT_TABLE].as_table_mut().unwrap();
            client.insert("anything".to_owned(), 1.into());
        });
    });
    read_manifest(anything.path()).unwrap();

    let unknown = probe_dir_with(|dir| {
        edit_manifest(dir, |manifest| {
            manifest.insert("clients".to_owned(), toml::Table::new().into());
        });
    });
    let error = refusal(unknown.path());
    assert!(error.contains("clients"), "{error}");
}

/// The limit is inclusive: a manifest padded to exactly `MAX_MANIFEST_BYTES` is read, and one
/// byte more is refused.
#[test]
fn oversized_manifest_is_refused() {
    let at_limit = probe_dir_with(|dir| pad_manifest(dir, MAX_MANIFEST_BYTES));
    read_manifest(at_limit.path()).unwrap();

    let over = probe_dir_with(|dir| pad_manifest(dir, MAX_MANIFEST_BYTES + 1));
    let error = refusal(over.path());
    let limit = format!("{MANIFEST_FILE} exceeds {MAX_MANIFEST_BYTES} bytes");
    assert!(error.contains(&limit), "{error}");
}

/// A version has 1 to `MAX_VERSION_BYTES` bytes, counted as bytes rather than characters, and
/// no control characters.
#[test]
fn version_is_bounded() {
    let with_version = |version: &str| {
        probe_dir_with(|dir| {
            edit_manifest(dir, |manifest| {
                manifest.insert("version".to_owned(), version.into());
            });
        })
    };
    let valid = [
        "1".to_owned(),
        "9".repeat(MAX_VERSION_BYTES),
        "é".repeat(MAX_VERSION_BYTES / 2),
    ];
    for version in valid {
        let dir = with_version(&version);
        if let Err(error) = read_manifest(dir.path()) {
            panic!("{version:?} is refused: {error:#}");
        }
    }
    let invalid = [
        String::new(),
        "9".repeat(MAX_VERSION_BYTES + 1),
        "é".repeat(MAX_VERSION_BYTES / 2 + 1),
        "1.0\n".to_owned(),
        "1.0\u{7f}".to_owned(),
    ];
    for version in invalid {
        let dir = with_version(&version);
        let error = refusal(dir.path());
        assert!(error.contains("invalid version"), "{version:?}: {error}");
    }
}

/// Loads a probe artifact whose `server.wasm` is `module` and checks that it is refused as not
/// being a server component.
fn assert_role_refused(module: Vec<u8>) {
    let dir = with_server_wasm(module);
    let error = refusal(dir.path());
    assert!(
        error.contains("is not a") && error.contains("server component"),
        "{error}"
    );
}

#[test]
fn core_module_without_world_is_refused() {
    assert_role_refused(wat::parse_str("(module)").unwrap());
}

#[test]
fn client_component_is_refused() {
    assert_role_refused(hello_wasm().to_vec());
}

#[test]
fn wasi_import_is_refused() {
    let wasi = r#"(module (import "wasi_snapshot_preview1" "fd_write"
        (func (param i32 i32 i32 i32) (result i32))))"#;
    assert_role_refused(wat::parse_str(wasi).unwrap());
}

/// The limit is inclusive: the probe padded to exactly `MAX_COMPONENT_BYTES` loads, and one
/// byte more is refused.
#[test]
fn oversized_component_is_refused() {
    let at_limit = with_server_wasm(padded_probe(MAX_COMPONENT_BYTES));
    let (engine, _ticker) = engine().unwrap();
    load(&engine, at_limit.path()).unwrap();

    let over = with_server_wasm(padded_probe(MAX_COMPONENT_BYTES + 1));
    let error = refusal(over.path());
    let limit = format!("{SERVER_WASM} exceeds {MAX_COMPONENT_BYTES} bytes");
    assert!(error.contains(&limit), "{error}");
}

/// A guest built against server WIT 0.1 still loads, and its callbacks run through the 0.1
/// imports. 0.1 has neither `client-message` nor `epoch`, so a client message or an epoch for it
/// is rejected unrun.
#[test]
fn v0_1_artifact_loads_and_runs() {
    let dir = v0_1_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    let texture = dir.path().join(ASSETS_DIR).join("counter.png");
    assert_eq!(
        loaded.blocks,
        vec![BlockDef {
            id: "probe:counter".to_owned(),
            display_name: "Legacy".to_owned(),
            textures: vec![Texture {
                slot: "*".to_owned(),
                path: texture.to_str().unwrap().to_owned(),
            }],
            mining: Mining::Breakable { hardness: 1.0 },
        }]
    );
    assert_eq!(
        run(&engine, &loaded, &interact(0)),
        Outcome::Committed {
            ops: vec![tell("v0.1")]
        }
    );
    for request in [client_message("probe.echo", 1, vec![]), epoch()] {
        let outcome = run(&engine, &loaded, &request);
        assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    }
}

/// A guest built against server WIT 0.2 still loads, and its callbacks run through the 0.2
/// imports: its client messages and sends hold scalars. 0.2 has no list or record values and no
/// `epoch`, so a client message holding one, or an epoch, is rejected unrun.
#[test]
fn v0_2_artifact_loads_and_runs() {
    let dir = v0_2_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert_eq!(
        run(&engine, &loaded, &interact(0)),
        Outcome::Committed {
            ops: vec![tell("v0.2")]
        }
    );
    let scalars = vec![
        Scalar::Bool(true),
        Scalar::Integer(-42),
        Scalar::Text("ack".to_owned()),
        Scalar::Choice(3),
    ];
    assert_eq!(
        run(
            &engine,
            &loaded,
            &client_message("probe.echo", 7, scalars.clone())
        ),
        Outcome::Committed {
            ops: vec![send("probe.echo", 7, scalars)]
        }
    );
    let nested = |value| client_message("probe.echo", 1, vec![Scalar::Bool(true), value]);
    for request in [
        nested(Scalar::List(Vec::new())),
        nested(Scalar::Record(vec![Scalar::Integer(1)])),
        epoch(),
    ] {
        let outcome = run(&engine, &loaded, &request);
        assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    }
}

/// A guest built against server WIT 0.3 still loads, and its callbacks run through the 0.3
/// imports: client messages with lists and records, and epochs, exactly as before 0.4. 0.3 has
/// no focus, so the artifact does not take one, and a client message or an epoch with a focus is
/// rejected unrun.
#[test]
fn v0_3_artifact_loads_and_runs() {
    let dir = v0_3_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert!(!loaded.focus);
    assert_eq!(
        run(&engine, &loaded, &interact(0)),
        Outcome::Committed {
            ops: vec![tell("v0.3")]
        }
    );
    let nested = vec![
        Scalar::List(vec![Scalar::Record(vec![Scalar::Integer(1)])]),
        Scalar::Text("ack".to_owned()),
    ];
    assert_eq!(
        run(
            &engine,
            &loaded,
            &client_message("probe.echo", 3, nested.clone())
        ),
        Outcome::Committed {
            ops: vec![send("probe.echo", 3, nested)]
        }
    );
    assert_eq!(
        run(&engine, &loaded, &epoch()),
        Outcome::Committed { ops: vec![] }
    );
    for request in [
        focused(client_message("probe.echo", 1, vec![]), p(0)),
        focused(epoch(), p(0)),
    ] {
        let outcome = run(&engine, &loaded, &request);
        assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    }
}

/// The probe targets the current world, which takes a focus; every older one does not.
#[test]
fn only_the_current_api_takes_a_focus() {
    let (engine, _ticker) = engine().unwrap();
    let probe = probe_dir();
    let loaded = load(&engine, probe.path()).unwrap();
    assert_eq!(loaded.manifest.api, current_api());
    assert!(loaded.focus);
    for dir in [v0_1_dir(), v0_2_dir()] {
        assert!(!load(&engine, dir.path()).unwrap().focus);
    }
}

/// The manifest's `api` names the world that `server.wasm` must target.
#[test]
fn api_must_match_the_component() {
    let with_api = |dir: TempDir, api: &str| {
        edit_manifest(dir.path(), |manifest| {
            manifest.insert("api".to_owned(), api.into());
        });
        dir
    };
    for dir in [
        with_api(v0_1_dir(), "0.2"),
        with_api(v0_2_dir(), "0.3"),
        with_api(v0_3_dir(), current_api()),
        with_api(probe_dir(), "0.3"),
        with_api(probe_dir(), "0.2"),
        with_api(probe_dir(), "0.1"),
    ] {
        let error = refusal(dir.path());
        assert!(
            error.contains("is not a") && error.contains("server component"),
            "{error}"
        );
    }
}
