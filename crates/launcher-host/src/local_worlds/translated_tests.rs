use super::*;
#[cfg(unix)]
use crate::local_worlds::{bds::BdsRunner, process::Runner, provision::install_dir};
use crate::local_worlds::{
    provision::RuntimeKind,
    test_support::{build_zip, scratch},
};

/// An original archive fixture with a single guest library and matching source pin.
fn archive_fixture() -> (TranslationPin, Vec<u8>) {
    let revision = "fixture".to_owned();
    let guest_library = "libguest.so".to_owned();
    let entry = format!("box64-{revision}/x64lib/{guest_library}");
    let archive = build_zip(&[(&entry, b"original guest library fixture")]);
    let pin = TranslationPin {
        helper_library: "helper".to_owned(),
        archive_base_url: "https://example.invalid/".to_owned(),
        revision,
        archive_sha256: format!("{:x}", Sha256::digest(&archive)),
        archive_bytes: archive.len() as u64,
        guest_library,
        native_runtime: NativeRuntime {
            libraries: vec![
                NativeLibrary {
                    packaged: "loader".to_owned(),
                    loader: true,
                },
                NativeLibrary {
                    packaged: "libnative.so".to_owned(),
                    loader: false,
                },
            ],
        },
    };
    (pin, archive)
}

#[test]
fn guest_code_is_extracted_only_after_its_complete_archive_is_verified() {
    let (pin, mut archive) = archive_fixture();
    assert_eq!(
        pin.guest_bytes(&archive).unwrap(),
        b"original guest library fixture"
    );
    archive[0] ^= 1;
    assert!(pin.guest_bytes(&archive).is_err());
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
    let helper = dir.path().join("translator");
    write_script(
        &helper,
        br#"#!/bin/sh
printf '%s' "$1" > "$0.guest"
printf '%s' "$BOX64_LD_LIBRARY_PATH" > "$0.paths"
printf '%s' "$BOX64_DYNAREC_STRONGMEM" > "$0.memory"
. "$1"
"#,
    );
    let loader = dir.path().join("loader");
    write_script(
        &loader,
        b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nshift 5\nexec \"$@\"\n",
    );
    let (pin, archive) = archive_fixture();
    let cache = dir.path().join("runtime");
    let guest_dir = cache.join(&pin.revision);
    fs::create_dir_all(&guest_dir).unwrap();
    fs::write(guest_dir.join(SOURCE_ARCHIVE), archive).unwrap();
    let mut runner = BdsRunner::new(Arc::new(provisioner), String::new());
    runner.translation = Some(Translation {
        helper: helper.clone(),
        cache,
        pin,
    });
    runner.start_timeout = Some(test_time::DEFAULT_TIMEOUT);
    let world = spec(&dir.path().join("world"));
    let instance = runner.start(&world, &Cancel::never()).unwrap();
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

    // Cached archives restore modified guest files without another download.
    fs::write(guest_dir.join("libguest.so"), b"damaged").unwrap();
    let reopened = runner.start(&world, &Cancel::never()).unwrap();
    assert_eq!(
        fs::read(guest_dir.join("libguest.so")).unwrap(),
        b"original guest library fixture"
    );
    reopened.stop(test_time::DEFAULT_TIMEOUT).unwrap();
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
    let (mut pin, archive) = archive_fixture();
    pin.guest_library = "missing-library".into();
    let cache = dir.path().join("runtime");
    let guest_dir = cache.join(&pin.revision);
    fs::create_dir_all(&guest_dir).unwrap();
    fs::write(guest_dir.join(SOURCE_ARCHIVE), archive).unwrap();
    let mut runner = BdsRunner::new(Arc::new(provisioner), String::new());
    runner.translation = Some(Translation {
        helper: dir.path().join("unused"),
        cache,
        pin,
    });
    let world = spec(&dir.path().join("world"));
    assert!(runner.start(&world, &Cancel::never()).is_err());
    let link = install_dir(&server)
        .unwrap()
        .join("worlds")
        .join(&world.world.id);
    assert!(fs::symlink_metadata(link).is_err());
    assert!(world.dir.join(super::super::store::LEVEL_DIR).is_dir());
}
