use super::*;
use experience_sdk::mod_manifest::API;

const TEMPLATE: &str = r#"{"namespace":"demo","overlay":{"type":"panel"}}"#;

/// Writes a package of `files` with a manifest hashing each one, then applies `edit`.
fn write_package(files: &[(&str, &[u8])], edit: impl FnOnce(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut hashes = String::from("[files]\n");
    let (mut templates, mut textures) = (Vec::new(), Vec::new());
    for (path, bytes) in files {
        let target = package_path(dir.path(), path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
        hashes.push_str(&format!(
            "\"{path}\" = \"{}\"\n",
            hex(&Sha256::digest(bytes))
        ));
        if path.starts_with("ui/") {
            templates.push(format!("\"{path}\""));
        } else if path.starts_with("textures/") {
            textures.push(format!("\"{path}\""));
        }
    }
    std::fs::write(
        dir.path().join(MANIFEST),
        format!(
            "id = \"demo\"\nversion = \"0.1.0\"\napi = \"{API}\"\npermissions = [\"screen\"]\n\
             templates = [{}]\ntextures = [{}]\n{hashes}",
            templates.join(","),
            textures.join(",")
        ),
    )
    .unwrap();
    edit(dir.path());
    dir
}

#[test]
fn a_verified_package_carries_its_screen_files() {
    let dir = write_package(
        &[
            ("mod.wasm", b"\0asm"),
            ("ui/overlay.json", TEMPLATE.as_bytes()),
            ("textures/arrow.png", b"png"),
        ],
        |_| {},
    );
    let package = Package::read(dir.path()).unwrap();
    assert_eq!(package.component, b"\0asm");
    assert_eq!(package.files.namespace, "demo");
    assert!(package.files.templates.contains_key("ui/overlay.json"));
    assert_eq!(package.files.textures[0].0, "textures/arrow.png");
}

#[test]
fn a_file_changed_after_hashing_is_refused() {
    let dir = write_package(&[("mod.wasm", b"\0asm")], |dir| {
        std::fs::write(dir.join("mod.wasm"), b"\0asm-changed").unwrap();
    });
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("mod.wasm does not match"), "{error}");
}

#[test]
fn a_template_of_another_namespace_is_refused() {
    let foreign = TEMPLATE.replace("\"demo\"", "\"common\"");
    let dir = write_package(
        &[
            ("mod.wasm", b"\0asm"),
            ("ui/overlay.json", foreign.as_bytes()),
        ],
        |_| {},
    );
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("namespace"), "{error}");
}

#[test]
fn a_missing_or_oversized_file_is_refused() {
    let dir = write_package(&[("mod.wasm", b"\0asm")], |dir| {
        std::fs::remove_file(dir.join("mod.wasm")).unwrap();
    });
    assert!(Package::read(dir.path()).is_err());
    let big = vec![b'{'; MAX_TEMPLATE_BYTES + 1];
    let dir = write_package(&[("mod.wasm", b"\0asm"), ("ui/overlay.json", &big)], |_| {});
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("byte limit"), "{error}");
}

#[test]
fn the_digest_follows_every_file() {
    let files: &[(&str, &[u8])] = &[
        ("mod.wasm", b"\0asm"),
        ("ui/overlay.json", TEMPLATE.as_bytes()),
    ];
    let first = Package::read(write_package(files, |_| {}).path()).unwrap();
    let again = Package::read(write_package(files, |_| {}).path()).unwrap();
    assert_eq!(first.digest, again.digest);
    let other = TEMPLATE.replace("panel", "image");
    let changed = Package::read(
        write_package(
            &[
                ("mod.wasm", b"\0asm"),
                ("ui/overlay.json", other.as_bytes()),
            ],
            |_| {},
        )
        .path(),
    )
    .unwrap();
    assert_ne!(first.digest, changed.digest);
}
