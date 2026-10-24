use super::*;
#[cfg(unix)]
use crate::local_worlds::{bds::BdsRunner, process::Runner, provision::install_dir};
use crate::local_worlds::{
    provision::RuntimeKind,
    test_support::{FakeMojang, scratch},
};

/// Original guest-library bytes with a matching immutable download pin.
fn library_fixture() -> (TranslationPin, Vec<u8>) {
    let library = b"original guest library fixture".to_vec();
    let pin = TranslationPin::fixture(&library, "https://example.invalid/".to_owned());
    (pin, library)
}

#[test]
fn pinned_guest_library_is_verified_without_an_archive() {
    let (pin, mut library) = library_fixture();
    assert!(pin.verify_library(&library).is_ok());
    library[0] ^= 1;
    assert!(pin.verify_library(&library).is_err());
    library.pop();
    assert!(pin.verify_library(&library).is_err());
}

#[test]
fn translated_server_uses_only_a_nonempty_exported_platform_trust_bundle() {
    let dir = scratch();
    let (pin, library) = library_fixture();
    let translation = Translation::new(
        dir.path().join("translator"),
        dir.path().join("runtime"),
        pin,
    );
    let guest = translation.cache.join(&translation.pin.revision);
    fs::create_dir_all(&guest).unwrap();
    fs::write(guest.join(&translation.pin.guest_library), library).unwrap();
    let service = FakeMojang::start("unused", Vec::new());
    let provisioner = service.provisioner(dir.path());
    let bundle = trust_path(&translation.cache);
    for bytes in [
        None,
        Some(&b""[..]),
        Some(&b"platform certificate fixture"[..]),
    ] {
        if let Some(bytes) = bytes {
            fs::write(&bundle, bytes).unwrap();
        }
        let command = translation
            .command(&dir.path().join("server"), &provisioner, &Cancel::never())
            .unwrap()
            .0;
        let exported = command
            .get_envs()
            .find(|(key, _)| *key == "SSL_CERT_FILE")
            .and_then(|(_, value)| value);
        assert_eq!(
            exported,
            bytes
                .filter(|bytes| !bytes.is_empty())
                .map(|_| bundle.as_os_str())
        );
    }
    assert_eq!(service.hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_missing_guest_downloads_once_and_then_works_from_cache() {
    let dir = scratch();
    let (mut pin, library) = library_fixture();
    let service = FakeMojang::start("unused", library.clone());
    pin.guest_base_url = format!("{}/bin-linux/", service.base);
    let translation = Translation::new(dir.path().join("unused"), dir.path().into(), pin);
    let provisioner = service.provisioner(dir.path());
    for _ in 0..2 {
        translation
            .ensure_library(&translation.pin, dir.path(), &provisioner, &Cancel::never())
            .unwrap();
        assert_eq!(fs::read(dir.path().join("libguest.so")).unwrap(), library);
    }
    assert_eq!(service.hits.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn wrong_download_lengths_preserve_the_previous_library_and_legacy_archive() {
    let (_, library) = library_fixture();
    for bytes in [
        library[..library.len() - 1].to_vec(),
        [library.as_slice(), b"extra"].concat(),
    ] {
        let dir = scratch();
        let (mut pin, _) = library_fixture();
        let service = FakeMojang::start("unused", bytes);
        pin.guest_base_url = format!("{}/bin-linux/", service.base);
        let translation = Translation::new(dir.path().join("unused"), dir.path().into(), pin);
        fs::write(dir.path().join("libguest.so"), b"previous").unwrap();
        fs::write(dir.path().join(LEGACY_SOURCE_ARCHIVE), b"legacy").unwrap();
        assert!(
            translation
                .ensure_library(
                    &translation.pin,
                    dir.path(),
                    &service.provisioner(dir.path()),
                    &Cancel::never()
                )
                .is_err()
        );
        assert_eq!(
            fs::read(dir.path().join("libguest.so")).unwrap(),
            b"previous"
        );
        assert_eq!(
            fs::read(dir.path().join(LEGACY_SOURCE_ARCHIVE)).unwrap(),
            b"legacy"
        );
    }
}

#[test]
fn bundled_runtime_names_are_unique_and_identify_one_loader() {
    use std::collections::HashSet;

    let pin = &android_runtime().bds_translation;
    let libraries = &pin.native_runtime.libraries;
    assert_eq!(libraries.iter().filter(|library| library.loader).count(), 1);
    let mut names = HashSet::from([pin.helper_library.as_str()]);
    for library in libraries {
        assert!(names.insert(library.packaged.as_str()));
        assert!(library.packaged.starts_with("lib") && library.packaged.ends_with(".so"));
        assert_eq!(Path::new(&library.packaged).components().count(), 1);
    }
}

#[test]
fn cancellation_preserves_the_previous_cache_file() {
    let dir = scratch();
    let path = dir.path().join("guest");
    fs::write(&path, b"previous").unwrap();
    let (handle, cancel) = super::super::cancel_pair();
    drop(handle);
    assert!(matches!(
        publish(&path, b"replacement", &cancel),
        Err(WorldError::Cancelled)
    ));
    assert_eq!(fs::read(path).unwrap(), b"previous");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn android_detection_requires_the_bundled_helper_and_does_not_change_desktop_hosts() {
    let dir = scratch();
    let core = dir.path().join("core");
    let platform = Platform {
        os: "android",
        arch: "arm64",
    };
    assert!(Translation::detect(platform, &core, dir.path().into()).is_none());
    fs::write(
        core.with_file_name(&android_runtime().bds_translation.helper_library),
        b"fixture",
    )
    .unwrap();
    assert!(Translation::detect(platform, &core, dir.path().into()).is_none());
    for library in &android_runtime().bds_translation.native_runtime.libraries {
        fs::write(core.with_file_name(&library.packaged), b"fixture").unwrap();
    }
    let translation = Translation::detect(platform, &core, dir.path().into()).unwrap();
    assert_eq!(translation.runtime().kind, RuntimeKind::Translated);
    assert_eq!(
        super::super::manager::default_backend(&translation.runtime()),
        bridge::Backend::Bds
    );
    for os in ["linux", "darwin", "windows"] {
        assert!(
            Translation::detect(Platform { os, arch: "arm64" }, &core, dir.path().into()).is_none()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_translated_world_uses_cached_guest_code_and_saves_on_stop() {
    use crate::local_worlds::test_support::{FAKE_BDS, installed_provisioner, spec, write_script};
    use crate::local_worlds::unpack::INSTALL_MANIFEST;
    use std::{os::unix::fs::PermissionsExt, sync::Arc};

    let dir = scratch();
    let platform = Platform {
        os: "android",
        arch: "arm64",
    };
    let server_script = String::from_utf8(FAKE_BDS.to_vec())
        .unwrap()
        .replace("    exit 0", "    printf saved > saved; exit 0");
    let provisioner = installed_provisioner(dir.path(), platform, server_script.as_bytes());
    provisioner.set_runtime(RuntimeInfo::translated());
    let server = provisioner.installed().unwrap().0;
    fs::set_permissions(&server, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(
        install_dir(&server).unwrap().join(INSTALL_MANIFEST),
        br#"{"zip_sha256":"archive fixture"}"#,
    )
    .unwrap();
    let helper = dir.path().join("translator");
    write_script(
        &helper,
        br#"#!/bin/sh
printf '%s' "$1" > "$0.guest"
printf '%s' "$BOX64_LD_LIBRARY_PATH" > "$0.paths"
printf '%s' "$BOX64_DYNAREC_STRONGMEM" > "$0.memory"
printf '%s\n' "${BOX64_DYNACACHE-unset} ${BOX64_NORCFILES-unset} ${BOX64_DYNACACHE_FOLDER+folder}" >> "$0.cache"
trap '[ "$BOX64_DYNACACHE" != 1 ] || printf code > "$BOX64_DYNACACHE_FOLDER/bedrock_server-1-2.box64"' EXIT
. "$1"
"#,
    );
    let loader = dir.path().join("loader");
    write_script(
        &loader,
        b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nshift 5\nexec \"$@\"\n",
    );
    let (mut pin, library) = library_fixture();
    let service = FakeMojang::start("unused", library.clone());
    pin.guest_base_url = format!("{}/bin-linux/", service.base);
    let cache = dir.path().join("runtime");
    let code_root = cache.join(CODE_CACHE_DIR);
    let guest_dir = cache.join(&pin.revision);
    fs::create_dir_all(&guest_dir).unwrap();
    fs::write(guest_dir.join(&pin.guest_library), &library).unwrap();
    fs::write(guest_dir.join(LEGACY_SOURCE_ARCHIVE), b"legacy archive").unwrap();
    let cache_launches = || {
        fs::read_to_string(helper.with_extension("cache"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let mut runner = BdsRunner::new(Arc::new(provisioner), String::new());
    runner.translation = Some(Arc::new(Translation::new(helper.clone(), cache, pin)));
    runner.start_timeout = Some(test_time::DEFAULT_TIMEOUT);
    let world = spec(&dir.path().join("world"));
    let instance = runner.start(&world, &Cancel::never()).unwrap();
    // The new world's create run writes the code cache at its stop, and the final run only reads it.
    assert_eq!(cache_launches(), ["1 1 folder", "2 1 folder"]);
    let generations: Vec<_> = fs::read_dir(&code_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(generations.len(), 1);
    assert_eq!(
        fs::read(generations[0].join("bedrock_server-1-2.box64")).unwrap(),
        b"code"
    );
    assert_eq!(
        fs::read_to_string(loader.with_extension("args"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        vec![
            "--inhibit-cache".to_owned(),
            "--library-path".to_owned(),
            dir.path().to_string_lossy().into_owned(),
            "--preload".to_owned(),
            dir.path()
                .join("libnative.so")
                .to_string_lossy()
                .into_owned(),
            helper.to_string_lossy().into_owned(),
            server.to_string_lossy().into_owned(),
        ]
    );
    assert_eq!(
        fs::read_to_string(helper.with_extension("memory")).unwrap(),
        STRONG_MEMORY
    );
    let paths = fs::read_to_string(helper.with_extension("paths")).unwrap();
    assert_eq!(
        std::env::split_paths(&paths).collect::<Vec<_>>(),
        vec![
            install_dir(&server).unwrap().to_path_buf(),
            guest_dir.clone()
        ]
    );
    assert_eq!(
        fs::read_to_string(helper.with_extension("guest")).unwrap(),
        server.to_string_lossy()
    );
    assert_eq!(
        fs::read(guest_dir.join("libguest.so")).unwrap(),
        b"original guest library fixture"
    );
    assert!(!instance.can_pause());
    instance.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    let install = install_dir(&server).unwrap();
    assert_eq!(fs::read(install.join("saved")).unwrap(), b"saved");
    assert!(fs::symlink_metadata(install.join("worlds").join(&world.world.id)).is_err());
    assert!(world.dir.join(super::super::store::LEVEL_DIR).is_dir());

    assert!(!guest_dir.join(LEGACY_SOURCE_ARCHIVE).exists());
    assert_eq!(service.hits.load(std::sync::atomic::Ordering::SeqCst), 0);

    // A changed cached library is replaced only by the verified direct download.
    fs::write(guest_dir.join("libguest.so"), b"damaged").unwrap();
    let reopened = runner.start(&world, &Cancel::never()).unwrap();
    assert_eq!(
        fs::read(guest_dir.join("libguest.so")).unwrap(),
        b"original guest library fixture"
    );
    assert_eq!(service.hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    reopened.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    assert_eq!(cache_launches(), ["1 1 folder", "2 1 folder", "2 1 folder"]);
    assert_eq!(
        fs::read(generations[0].join("bedrock_server-1-2.box64")).unwrap(),
        b"code"
    );
}

#[test]
fn the_code_cache_identity_follows_the_server_install_translator_bytes_and_pins() {
    use crate::local_worlds::unpack::INSTALL_MANIFEST;

    let dir = scratch();
    let (pin, _) = library_fixture();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let helper = dir.path().join("helper");
    fs::write(&helper, b"translator").unwrap();
    let generation = |pin: &TranslationPin| {
        Translation::new(helper.clone(), dir.path().join("runtime"), pin.clone())
            .code_cache(&install.join("bedrock_server"))
            .map(|cache| cache.generation())
    };
    let record = |digest: &str| {
        let manifest = serde_json::json!({ "zip_sha256": digest }).to_string();
        fs::write(install.join(INSTALL_MANIFEST), manifest).unwrap();
    };
    assert_eq!(generation(&pin), None, "an install without a manifest");
    record("first");
    let first = generation(&pin).unwrap();
    assert!(first.starts_with(dir.path().join("runtime").join(CODE_CACHE_DIR)));
    assert_eq!(generation(&pin).unwrap(), first);
    let mut changed = vec![];
    record("second");
    changed.push(generation(&pin));
    record("first");
    let mut native = pin.clone();
    native.native_runtime.sha256 = "other native runtime".to_owned();
    changed.push(generation(&native));
    let mut guest = pin.clone();
    guest.guest_sha256 = "other guest".to_owned();
    changed.push(generation(&guest));
    fs::write(&helper, b"patched translator").unwrap();
    changed.push(generation(&pin));
    for other in &changed {
        assert_ne!(other.as_ref().unwrap(), &first);
    }
    fs::remove_file(&helper).unwrap();
    assert_eq!(generation(&pin), None, "an unreadable translator");
}

#[cfg(unix)]
#[test]
fn runtime_setup_failure_removes_the_world_link_and_preserves_its_data() {
    use crate::local_worlds::test_support::{FAKE_BDS, installed_provisioner, spec};
    use std::sync::Arc;

    let dir = scratch();
    let platform = Platform {
        os: "android",
        arch: "arm64",
    };
    let provisioner = installed_provisioner(dir.path(), platform, FAKE_BDS);
    provisioner.set_runtime(RuntimeInfo::translated());
    let server = provisioner.installed().unwrap().0;
    let (mut pin, library) = library_fixture();
    let service = FakeMojang::start("unused", vec![0; library.len()]);
    pin.guest_base_url = format!("{}/bin-linux/", service.base);
    let cache = dir.path().join("runtime");
    let guest_dir = cache.join(&pin.revision);
    fs::create_dir_all(&guest_dir).unwrap();
    fs::write(
        guest_dir.join(&pin.guest_library),
        b"previous invalid library",
    )
    .unwrap();
    let mut runner = BdsRunner::new(Arc::new(provisioner), String::new());
    runner.translation = Some(Arc::new(Translation::new(
        dir.path().join("unused"),
        cache,
        pin,
    )));
    let world = spec(&dir.path().join("world"));
    assert!(runner.start(&world, &Cancel::never()).is_err());
    assert_eq!(
        fs::read(guest_dir.join("libguest.so")).unwrap(),
        b"previous invalid library"
    );
    let link = install_dir(&server)
        .unwrap()
        .join("worlds")
        .join(&world.world.id);
    assert!(fs::symlink_metadata(link).is_err());
    assert!(world.dir.join(super::super::store::LEVEL_DIR).is_dir());
}
