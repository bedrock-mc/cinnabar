#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// Writes an executable fixture without changing the parent test process's environment.
fn executable(path: &Path, source: &str) {
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Provides fake compilers with real output files and a stale global binding CLI.
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for dir in ["global-bin", "web/site/pkg", "compiled"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    fs::write(
        root.path().join("Cargo.lock"),
        "[[package]]\nname='wasm-bindgen'\nversion='0.2.127'\n",
    )
    .unwrap();
    fs::write(root.path().join("web/site/index.html"), "page").unwrap();
    fs::write(root.path().join("web/site/edges.mjs"), "edges").unwrap();
    fs::write(root.path().join("web/site/edges.test.mjs"), "tests").unwrap();
    fs::write(root.path().join("web/site/pkg/obsolete.js"), "old bindings").unwrap();
    fs::write(root.path().join("compiled/carrier.cxb"), "fixture asset").unwrap();
    executable(
        &root.path().join("global-bin/cargo"),
        r#"#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
target = pathlib.Path('target').resolve()
with open('commands.jsonl', 'a') as log:
    log.write(json.dumps(args) + '\n')
if os.environ.get('WEB_TEST_FAIL') == args[0]:
    sys.exit(42)
if args[0] == 'metadata':
    print(json.dumps({'target_directory': str(target)}))
elif args[0] == 'install':
    assert args[args.index('--version') + 1] == '=0.2.127'
    destination = pathlib.Path(args[args.index('--root') + 1]) / 'bin'
    destination.mkdir(parents=True)
    shutil.copy2('global-bin/wasm-bindgen', destination)
elif args[0] == 'run':
    source = pathlib.Path(args[args.index('--') + 1])
    destination = pathlib.Path(args[args.index('--copy-to') + 1])
    shutil.copytree(source, destination)
    print(json.dumps({'version': 1, 'entries': [{'name': 'carrier', 'bytes': 13}]}))
elif args[0] == 'build':
    destination = pathlib.Path(args[args.index('--target-dir') + 1]) / 'wasm32-unknown-unknown' / ('release' if '--release' in args else 'debug')
    destination.mkdir(parents=True)
    (destination / 'web_client.wasm').write_text('compiled wasm')
else:
    sys.exit(43)
"#,
    );
    executable(
        &root.path().join("global-bin/wasm-bindgen"),
        r#"#!/usr/bin/env python3
import pathlib, sys
if '--version' in sys.argv:
    print('wasm-bindgen ' + ('0.0.0' if pathlib.Path(sys.argv[0]).parent.name == 'global-bin' else '0.2.127'))
else:
    assert pathlib.Path(sys.argv[-1]).read_text() == 'compiled wasm'
    output = pathlib.Path(sys.argv[sys.argv.index('--out-dir') + 1])
    output.mkdir()
    (output / 'web_client.js').write_text('generated bindings')
    (output / 'web_client_bg.wasm').write_text('bound wasm')
"#,
    );
    root
}

/// Runs the public packaging command with child-only tool overrides.
fn build(root: &Path, fail: &str) -> std::process::Output {
    let path = std::env::join_paths(
        std::iter::once(root.join("global-bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_devtool"))
        .current_dir(root)
        .env("PATH", path)
        .env("WEB_TEST_FAIL", fail)
        .args([
            "build-web",
            "--assets",
            "compiled",
            "--out",
            "bundle",
            "--release",
        ])
        .output()
        .unwrap()
}

#[test]
fn web_bundle_uses_locked_bindings_and_delivers_only_complete_output() {
    let root = fixture();
    let output = build(root.path(), "");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for path in [
        "index.html",
        "edges.mjs",
        "pkg/web_client.js",
        "pkg/web_client_bg.wasm",
        "assets/carrier.cxb",
        "assets/manifest.json",
    ] {
        assert!(
            root.path().join("bundle").join(path).is_file(),
            "missing {path}"
        );
    }
    assert!(!root.path().join("bundle/pkg/obsolete.js").exists());
    assert!(!root.path().join("bundle/edges.test.mjs").exists());
    let commands = fs::read_to_string(root.path().join("commands.jsonl")).unwrap();
    assert!(commands.contains("install"));
    assert_eq!(
        fs::read_to_string(root.path().join("bundle/assets/carrier.cxb")).unwrap(),
        "fixture asset"
    );
    let second = build(root.path(), "");
    assert!(!second.status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("commands.jsonl")).unwrap(),
        commands
    );
    assert_eq!(
        fs::read_to_string(root.path().join("bundle/index.html")).unwrap(),
        "page"
    );
}

#[test]
fn web_bundle_failure_leaves_no_partial_site() {
    for step in ["run", "build", "install"] {
        let root = fixture();
        assert!(!build(root.path(), step).status.success());
        assert!(!root.path().join("bundle").exists());
        assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".web-bundle-")
        }));
    }
}
