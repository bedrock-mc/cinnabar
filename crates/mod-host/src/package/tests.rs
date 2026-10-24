use super::*;
use experience_sdk::mod_manifest::API;

/// The smallest well-formed component: header only.
const COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";
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
            ("mod.wasm", COMPONENT),
            ("ui/overlay.json", TEMPLATE.as_bytes()),
            ("textures/arrow.png", b"png"),
        ],
        |_| {},
    );
    let package = Package::read(dir.path()).unwrap();
    assert_eq!(package.component, COMPONENT);
    assert_eq!(package.files.namespace, "demo");
    assert!(package.files.templates.contains_key("ui/overlay.json"));
    assert_eq!(package.files.textures[0].0, "textures/arrow.png");
}

#[test]
fn a_file_changed_after_hashing_is_refused() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |dir| {
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
            ("mod.wasm", COMPONENT),
            ("ui/overlay.json", foreign.as_bytes()),
        ],
        |_| {},
    );
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("namespace"), "{error}");
}

#[test]
fn a_missing_or_oversized_file_is_refused() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |dir| {
        std::fs::remove_file(dir.join("mod.wasm")).unwrap();
    });
    assert!(Package::read(dir.path()).is_err());
    let big = vec![b'{'; MAX_TEMPLATE_BYTES + 1];
    let dir = write_package(
        &[("mod.wasm", COMPONENT), ("ui/overlay.json", &big)],
        |_| {},
    );
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("byte limit"), "{error}");
}

#[test]
fn the_digest_follows_every_file() {
    let files: &[(&str, &[u8])] = &[
        ("mod.wasm", COMPONENT),
        ("ui/overlay.json", TEMPLATE.as_bytes()),
    ];
    let first = Package::read(write_package(files, |_| {}).path()).unwrap();
    let again = Package::read(write_package(files, |_| {}).path()).unwrap();
    assert_eq!(first.digest, again.digest);
    let other = TEMPLATE.replace("panel", "image");
    let changed = Package::read(
        write_package(
            &[
                ("mod.wasm", COMPONENT),
                ("ui/overlay.json", other.as_bytes()),
            ],
            |_| {},
        )
        .path(),
    )
    .unwrap();
    assert_ne!(first.digest, changed.digest);
}

#[test]
fn package_source_bytes_share_one_admission_budget() {
    let image = vec![0; MAX_TEXTURE_BYTES];
    let files = [
        ("mod.wasm", COMPONENT),
        ("textures/a.png", image.as_slice()),
        ("textures/b.png", image.as_slice()),
        ("textures/c.png", image.as_slice()),
        ("textures/d.png", image.as_slice()),
    ];
    assert!(image.len() * 4 >= MAX_PACKAGE_BYTES);
    let dir = write_package(&files, |_| {});
    let error = format!("{:#}", Package::read(dir.path()).err().unwrap());
    assert!(error.contains("package byte limit"), "{error}");
}

#[test]
#[cfg(unix)]
fn package_files_cannot_be_replaced_with_links() {
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("mod.wasm");
    std::fs::write(&target, COMPONENT).unwrap();
    let dir = write_package(&[("mod.wasm", COMPONENT)], |dir| {
        std::fs::remove_file(dir.join("mod.wasm")).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("mod.wasm")).unwrap();
    });
    assert!(Package::read(dir.path()).is_err());
}

/// Overwrites an existing fixture with a distinct modification stamp on coarse filesystems.
#[cfg(unix)]
fn write_changed(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let path = path.as_ref();
    let modified = std::fs::metadata(path)?.modified()?;
    std::fs::write(path, bytes)?;
    std::fs::File::options().write(true).open(path)?.set_times(
        std::fs::FileTimes::new().set_modified(modified + std::time::Duration::from_secs(1)),
    )
}

#[test]
#[cfg(unix)]
fn observations_share_verified_packages_until_any_selected_file_changes() {
    let dir = write_package(
        &[
            ("mod.wasm", COMPONENT),
            ("ui/overlay.json", TEMPLATE.as_bytes()),
            ("textures/arrow.png", b"old"),
        ],
        |_| {},
    );
    let mut observer = Observation::default();
    let mut now = std::time::Instant::now();
    let mut previous = observer
        .read_at(dir.path(), now, |opened, limit| opened.read(limit))
        .unwrap();
    for _ in 0..8 {
        assert!(Arc::ptr_eq(
            &previous,
            &observer
                .read_at(dir.path(), now, |opened, limit| opened.read(limit))
                .unwrap()
        ));
    }
    let mut component = COMPONENT.to_vec();
    component.extend([0, 2, 1, b'x']);
    let template = TEMPLATE.replace("panel", "image");
    for (path, bytes) in [
        ("mod.wasm", component.as_slice()),
        ("ui/overlay.json", template.as_bytes()),
        ("textures/arrow.png", b"new".as_slice()),
    ] {
        let manifest_path = dir.path().join(MANIFEST);
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let old = std::fs::read(dir.path().join(path)).unwrap();
        write_changed(dir.path().join(path), bytes).unwrap();
        now += crate::local_files::CONFIRMATION_DELAY;
        assert!(
            observer
                .read_at(dir.path(), now, |opened, limit| opened.read(limit))
                .is_err(),
            "a stale manifest must reject changed {path}"
        );
        write_changed(
            &manifest_path,
            manifest.replace(&hex(&Sha256::digest(&old)), &hex(&Sha256::digest(bytes))),
        )
        .unwrap();
        now += crate::local_files::CONFIRMATION_DELAY;
        let changed = observer
            .read_at(dir.path(), now, |opened, limit| opened.read(limit))
            .unwrap();
        assert_ne!(previous.digest, changed.digest);
        assert!(!Arc::ptr_eq(&previous, &changed));
        assert!(Arc::ptr_eq(
            &changed,
            &observer
                .read_at(dir.path(), now, |opened, limit| opened.read(limit))
                .unwrap()
        ));
        previous = changed;
    }
    let manifest = dir.path().join(MANIFEST);
    write_changed(
        &manifest,
        std::fs::read_to_string(&manifest)
            .unwrap()
            .replace("0.1.0", "0.1.1"),
    )
    .unwrap();
    now += crate::local_files::CONFIRMATION_DELAY;
    assert_ne!(
        previous.digest,
        observer
            .read_at(dir.path(), now, |opened, limit| opened.read(limit))
            .unwrap()
            .digest
    );
}

#[test]
fn observations_reject_removed_or_linked_package_files_and_recover_after_repair() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |_| {});
    let mut observer = Observation::default();
    let first = observer.read(dir.path()).unwrap();
    let path = dir.path().join("mod.wasm");
    std::fs::remove_file(&path).unwrap();
    assert!(observer.read(dir.path()).is_err());
    #[cfg(unix)]
    {
        let target = dir.path().join("outside.wasm");
        std::fs::write(&target, COMPONENT).unwrap();
        std::os::unix::fs::symlink(target, &path).unwrap();
        assert!(observer.read(dir.path()).is_err());
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::write(&path, COMPONENT).unwrap();
    let repaired = observer.read(dir.path()).unwrap();
    assert_eq!(first.digest, repaired.digest);
    #[cfg(unix)]
    assert!(Arc::ptr_eq(&repaired, &observer.read(dir.path()).unwrap()));
}

#[test]
#[cfg(unix)]
fn rejected_package_observations_do_not_reread_bytes_and_repairs_retry() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |_| {});
    let path = dir.path().join("mod.wasm");
    std::fs::write(&path, b"wrong hash").unwrap();
    let mut observer = Observation::default();
    let mut reads = 0;
    let now = std::time::Instant::now();
    for _ in 0..8 {
        assert!(
            observer
                .read_at(dir.path(), now, |opened, limit| {
                    reads += 1;
                    opened.read(limit)
                })
                .is_err()
        );
    }
    assert_eq!(reads, 2, "only the manifest and changed component are read");
    std::fs::write(&path, COMPONENT).unwrap();
    assert!(
        observer
            .read_at(dir.path(), now, |opened, limit| {
                reads += 1;
                opened.read(limit)
            })
            .is_ok()
    );
    assert_eq!(reads, 4);
    std::fs::remove_file(&path).unwrap();
    for _ in 0..8 {
        assert!(
            observer
                .read_at(dir.path(), now, |opened, limit| {
                    reads += 1;
                    opened.read(limit)
                })
                .is_err()
        );
    }
    assert_eq!(
        reads, 5,
        "a missing companion must not cause manifest rereads"
    );
    std::fs::write(&path, COMPONENT).unwrap();
    assert!(observer.read(dir.path()).is_ok());
}

#[test]
fn package_observations_retry_transient_read_errors_without_a_source_edit() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |_| {});
    let mut observer = Observation::default();
    assert!(
        observer
            .read_with(dir.path(), |_, _| {
                Err(std::io::Error::other("temporary read failure"))
            })
            .is_err()
    );
    assert_eq!(observer.read(dir.path()).unwrap().component, COMPONENT);
}

#[test]
#[cfg(unix)]
fn package_observations_confirm_new_stamps_once_without_periodic_reads() {
    let dir = write_package(&[("mod.wasm", COMPONENT)], |_| {});
    let now = std::time::Instant::now();
    let mut observer = Observation::default();
    let reads = std::cell::Cell::new(0);
    let observe = |observer: &mut Observation, now| {
        observer
            .read_at(dir.path(), now, |opened, limit| {
                reads.set(reads.get() + 1);
                opened.read(limit)
            })
            .unwrap()
    };
    let first = observe(&mut observer, now);
    assert!(Arc::ptr_eq(&first, &observe(&mut observer, now)));
    assert_eq!(reads.get(), 2);
    assert!(Arc::ptr_eq(
        &first,
        &observe(&mut observer, now + crate::local_files::CONFIRMATION_DELAY)
    ));
    assert_eq!(reads.get(), 4);
    for seconds in [2, 10, 100, 1000] {
        assert!(Arc::ptr_eq(
            &first,
            &observe(&mut observer, now + std::time::Duration::from_secs(seconds))
        ));
    }
    assert_eq!(reads.get(), 4, "package confirmation is not periodic");
}

#[test]
fn failed_package_observations_release_the_previous_verified_payload() {
    for missing_root in [false, true] {
        let dir = write_package(&[("mod.wasm", COMPONENT)], |_| {});
        let mut observer = Observation::default();
        let accepted = observer.read(dir.path()).unwrap();
        let retained = Arc::downgrade(&accepted);
        drop(accepted);
        if missing_root {
            std::fs::remove_dir_all(dir.path()).unwrap();
        } else {
            std::fs::write(dir.path().join("mod.wasm"), b"invalid replacement").unwrap();
        }
        assert!(observer.read(dir.path()).is_err());
        assert!(
            retained.upgrade().is_none(),
            "failed verification must release the old package buffers"
        );
    }
}
