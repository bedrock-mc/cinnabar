use std::{fs, process::Command};

#[test]
fn pcm_cli_requires_reviewed_inputs_and_never_overwrites_input_aliases() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let catalog = root.join("catalog.bin");
    let manifest = root.join("manifest.json");
    let output = root.join("output.bin");
    let report = root.join("report.json");
    fs::write(&catalog, b"synthetic invalid catalog").unwrap();
    fs::write(&manifest, b"{}").unwrap();
    let run = |out: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_assetc"))
            .arg("audio-pcm-assets")
            .arg("--pack")
            .arg(root)
            .arg("--catalog")
            .arg(&catalog)
            .arg("--source-manifest")
            .arg(&manifest)
            .arg("--out")
            .arg(out)
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap()
    };
    assert!(!run(&output).status.success());
    assert!(!output.exists());
    assert!(!report.exists());
    let alias = run(&catalog);
    assert!(!alias.status.success());
    assert!(String::from_utf8_lossy(&alias.stderr).contains("distinct files"));
    assert_eq!(fs::read(&catalog).unwrap(), b"synthetic invalid catalog");
    let alias = run(&manifest);
    assert!(!alias.status.success());
    assert_eq!(fs::read(&manifest).unwrap(), b"{}");
}

#[test]
fn pcm_cli_help_exposes_generation_without_playback_activation() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["audio-pcm-assets", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--catalog"));
    assert!(help.contains("does not activate playback"));
}
