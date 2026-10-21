use super::*;

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
