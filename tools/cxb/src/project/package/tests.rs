use super::*;

/// Creates a small valid installation archive without compiling a guest.
fn package(extra: Option<(&str, &[u8])>) -> Vec<u8> {
    let server = b"fixture-server";
    let server_manifest = format!(
        "id = \"benergistics\"\n[files]\n\"{SERVER_WASM}\" = {:?}\n",
        crypto::digest(server)
    );
    let mut files = BTreeMap::from([
        (format!("server/{SERVER_WASM}"), server.to_vec()),
        (
            format!("server/{MANIFEST_FILE}"),
            server_manifest.into_bytes(),
        ),
        (
            "client/benergistics.cxb".into(),
            crate::fixtures::generate().unwrap().bundle,
        ),
        (PACK.into(), b"fixture-pack".to_vec()),
    ]);
    if let Some((path, data)) = extra {
        files.insert(path.into(), data.to_vec());
    }
    let index = Index {
        format: 1,
        id: "benergistics".into(),
        files: files
            .iter()
            .map(|(path, data)| (path.clone(), crypto::digest(data)))
            .collect(),
    };
    files.insert(INDEX.into(), serde_json::to_vec(&index).unwrap());
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in files {
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&data).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[test]
fn rejects_escaping_and_conflicting_paths_before_creating_an_installation() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("mod.cmod");
    for path in [
        "../escaped",
        "/absolute",
        "a/../escaped",
        "a\\escaped",
        "client",
    ] {
        fs::write(&source, package(Some((path, b"untrusted")))).unwrap();
        assert!(install(&source, root.path()).is_err(), "accepted {path}");
        assert!(!root.path().join("mods/benergistics").exists());
    }
}

#[test]
fn failed_replacement_keeps_the_installed_package() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("mod.cmod");
    fs::write(&source, package(None)).unwrap();
    let installed = install(&source, root.path()).unwrap();
    fs::write(installed.join("owned-marker"), b"previous").unwrap();
    assert!(install(&source, root.path()).is_err());
    fs::write(&source, b"invalid archive").unwrap();
    assert!(replace(&source, root.path()).is_err());
    assert_eq!(
        fs::read(installed.join("owned-marker")).unwrap(),
        b"previous"
    );
    fs::write(&source, package(None)).unwrap();
    replace(&source, root.path()).unwrap();
    assert!(!installed.join("owned-marker").exists());
}

#[test]
fn changed_indexed_file_is_rejected() {
    let mut bytes = package(None);
    let position = bytes
        .windows(b"fixture-server".len())
        .position(|part| part == b"fixture-server")
        .unwrap();
    bytes[position] ^= 1;
    assert!(verify(&bytes).is_err());
}
