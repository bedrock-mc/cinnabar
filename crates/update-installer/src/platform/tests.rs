use super::*;

#[cfg(target_os = "linux")]
#[test]
fn extracted_linux_installs_prepare_a_complete_appdir_before_replacement() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("installed");
    fs::create_dir(&target).unwrap();
    let artifact = root.path().join("payload");
    fs::write(&artifact, crate::fixtures::linux_payload()).unwrap();
    let prepared = candidate(&artifact, root.path(), &target).unwrap();
    assert!(prepared.is_dir());
    assert!(crate::target::linux_executable(&prepared).is_file());
    assert_ne!(
        fs::metadata(prepared.join(crate::target::APP_RUN))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert!(fs::read_dir(&target).unwrap().next().is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn failed_or_incomplete_extraction_never_prepares_an_appdir() {
    use std::fs;
    for script in [
        b"#!/bin/sh\nexit 17\n".as_slice(),
        b"#!/bin/sh\nmkdir squashfs-root\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("installed");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("retained"), "old").unwrap();
        let artifact = root.path().join("payload");
        fs::write(&artifact, script).unwrap();
        assert!(candidate(&artifact, root.path(), &target).is_err());
        assert_eq!(fs::read(target.join("retained")).unwrap(), b"old");
    }
}

#[test]
fn an_upgrade_keeps_a_custom_install_folder_with_spaces() {
    let msi = Path::new(r"C:\Users\Jo Smith\AppData\Local\Cinnabar\update\update.msi");
    let target = Path::new(r"D:\My Games\Cinnabar");
    assert_eq!(
        msiexec_arguments(msi, target),
        [
            "/i",
            r#""C:\Users\Jo Smith\AppData\Local\Cinnabar\update\update.msi""#,
            "/passive",
            "/norestart",
            "REBOOT=ReallySuppress",
            "MSIDISABLERMRESTART=1",
            r#"INSTALLFOLDER="D:\My Games\Cinnabar""#,
        ]
    );
}

#[test]
fn no_trailing_backslash_escapes_the_closing_quote() {
    for (target, folder) in [
        (
            r"D:\My Games\Cinnabar\",
            r#"INSTALLFOLDER="D:\My Games\Cinnabar""#,
        ),
        (r"D:\", r#"INSTALLFOLDER="D:\\""#),
        (r"D:", r#"INSTALLFOLDER="D:\\""#),
    ] {
        let arguments = msiexec_arguments(Path::new("update.msi"), Path::new(target));
        assert_eq!(arguments.last().unwrap(), folder, "{target}");
    }
}
