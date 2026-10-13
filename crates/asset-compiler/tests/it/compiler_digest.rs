use std::{collections::BTreeSet, fs, path::Path};

use sha2::{Digest, Sha256};

#[path = "../../build_support/sources.rs"]
mod sources;

/// Creates a fixture input and its parent directories.
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Hashes each reached package and every literal input its sources embed.
fn digest(root: &Path, packages: &[&str]) -> [u8; 32] {
    let mut files = BTreeSet::new();
    for package in packages {
        let package = root.join(package).canonicalize().unwrap();
        sources::collect_files(&package, &package, &mut files);
    }
    let embedded = files
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .flat_map(|path| sources::embedded_files(path))
        .collect::<Vec<_>>();
    files.extend(embedded);
    let mut hasher = Sha256::new();
    for path in files {
        hasher.update(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(fs::read(path).unwrap());
        hasher.update([0]);
    }
    hasher.finalize().into()
}

#[test]
fn nested_packages_and_their_tests_do_not_leak_into_the_parent_digest() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for package in ["compiler", "compiler/entity", "compiler/unreached"] {
        write(&root, &format!("{package}/Cargo.toml"), b"[package]");
        write(
            &root,
            &format!("{package}/src/lib.rs"),
            b"pub struct Input;",
        );
        write(&root, &format!("{package}/tests/it.rs"), b"old test");
    }
    let packages = ["compiler", "compiler/entity"];
    let original = digest(&root, &packages);
    write(&root, "compiler/entity/tests/it.rs", b"changed test");
    write(&root, "compiler/unreached/src/lib.rs", b"unrelated source");
    assert_eq!(digest(&root, &packages), original);
    write(&root, "compiler/entity/src/lib.rs", b"pub struct Changed;");
    assert_ne!(digest(&root, &packages), original);
}

#[test]
fn embedded_inputs_are_kept_even_when_their_directory_is_excluded() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root, "compiler/Cargo.toml", b"[package]");
    write(
        &root,
        "compiler/src/lib.rs",
        b"const INPUT: &[u8] = include_bytes!(\"../entity/tests/input.bin\");",
    );
    write(&root, "compiler/entity/Cargo.toml", b"[package]");
    write(&root, "compiler/entity/tests/input.bin", b"original input");
    let original = digest(&root, &["compiler"]);
    write(&root, "compiler/entity/tests/input.bin", b"changed input");
    assert_ne!(digest(&root, &["compiler"]), original);
}
