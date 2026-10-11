use std::{fs, path::Path};

use architecture::check_repository;

/// Writes a one-crate repository whose manifest carries `manifest_extra`.
fn fixture(root: &Path, source: &str, manifest_extra: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname='sample'\nversion='0.1.0'\n{manifest_extra}\n"),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), source).unwrap();
    fs::write(
        root.join("policy.toml"),
        "production_rust_max=1000\nmodule_root_max=300\npowershell_max=800\ntest_max=1200\n[[crates]]\nname='sample'\npath='.'\n",
    )
    .unwrap();
}

fn findings(source: &str, manifest_extra: &str) -> Vec<String> {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), source, manifest_extra);
    check_repository(temp.path(), &temp.path().join("policy.toml"))
        .unwrap()
        .into_iter()
        .filter(|line| line.contains("platform-specific"))
        .collect()
}

const DECLARE_MACOS: &str = "[package.metadata.cinnabar]\nplatform-specific=['macos']";
const DECLARE_WINDOWS: &str = "[package.metadata.cinnabar]\nplatform-specific=['windows']";

#[test]
fn undeclared_macos_code_fails() {
    for source in [
        "#[cfg(target_os = \"macos\")] fn native() {}",
        "#[cfg_attr(target_vendor = \"apple\", inline)] fn native() {}",
        "#![cfg(any(target_os = \"macos\", target_os = \"linux\"))]",
        "fn native() -> bool { cfg!(target_os = \"macos\") }",
        "fn native() { assert!(cfg!(not(target_os = \"macos\"))); }",
        "use objc2_app_kit::NSView;",
        "fn native(handle: Handle) { if let Handle::AppKit(view) = handle {} }",
    ] {
        let lines = findings(source, "");
        assert_eq!(
            lines,
            [
                "sample: macos code in src/lib.rs needs `platform-specific = [\"macos\"]` under [package.metadata.cinnabar]"
            ],
            "{source}"
        );
        assert!(findings(source, DECLARE_MACOS).is_empty(), "{source}");
    }
}

#[test]
fn undeclared_windows_code_fails() {
    for source in [
        "#[cfg(windows)] fn native() {}",
        "#[cfg(all(test, not(windows)))] mod tests {}",
        "#[cfg(target_os = \"windows\")] fn native() {}",
        "fn native() -> bool { cfg!(windows) }",
        "use windows_sys::Win32::Foundation::HANDLE;",
        "fn native(path: &str) { let _ = std::os::windows::fs::symlink_file(path, path); }",
    ] {
        let lines = findings(source, "");
        assert_eq!(
            lines,
            [
                "sample: windows code in src/lib.rs needs `platform-specific = [\"windows\"]` under [package.metadata.cinnabar]"
            ],
            "{source}"
        );
        assert!(findings(source, DECLARE_WINDOWS).is_empty(), "{source}");
    }
}

#[test]
fn platform_manifest_tables_and_dependencies_need_declarations() {
    for manifest in [
        "[target.'cfg(windows)'.dependencies]\nlibc='1'",
        "[dependencies]\nwindows-sys='0.61'",
    ] {
        assert_eq!(
            findings("", manifest),
            [
                "sample: windows code in Cargo.toml needs `platform-specific = [\"windows\"]` under [package.metadata.cinnabar]"
            ],
            "{manifest}"
        );
    }
    assert_eq!(
        findings(
            "",
            "[target.'cfg(target_os = \"macos\")'.dependencies]\nobjc2='0.6'"
        )
        .len(),
        1
    );
}

#[test]
fn portable_code_needs_no_declaration() {
    for source in [
        "#[cfg(unix)] fn native() {}",
        "#[cfg(target_os = \"linux\")] fn native() {}",
        "#[cfg(target_arch = \"aarch64\")] fn native() {}",
        "const DOC: &str = \"#[cfg(windows)] use windows_sys::Win32;\";",
        "// #[cfg(target_os = \"macos\")]\nfn portable() { let windows = 2; let _ = windows; }",
    ] {
        assert!(findings(source, "").is_empty(), "{source}");
    }
}

#[test]
fn stale_and_unknown_declarations_fail() {
    assert_eq!(
        findings(
            "fn portable() {}",
            "[package.metadata.cinnabar]\nplatform-specific=['macos', 'windows']"
        ),
        [
            "sample: declares platform-specific \"macos\" but has no macos code",
            "sample: declares platform-specific \"windows\" but has no windows code",
        ]
    );
    assert_eq!(
        findings(
            "fn portable() {}",
            "[package.metadata.cinnabar]\nplatform-specific=['linux']"
        ),
        [
            "sample: unknown platform-specific value \"linux\"; expected one of [\"macos\", \"windows\"]"
        ]
    );
}

#[test]
fn matching_declarations_pass() {
    let source = "#[cfg(target_os = \"macos\")] fn mac() {} #[cfg(windows)] fn win() {}";
    assert!(
        findings(
            source,
            "[package.metadata.cinnabar]\nplatform-specific=['macos', 'windows']"
        )
        .is_empty()
    );
}
