//! Exercises the asset staging command used by browser bundle builds.

use assets::carriers::{
    self, Carrier,
    startup::delivery::{DeliveryManifest, MemoryCarriers, startup_carriers},
};
use std::{fs, process::Command};

/// Runs bundle staging against synthetic carriers and returns its directory and index.
fn stage(optional: &[&'static Carrier]) -> (tempfile::TempDir, DeliveryManifest) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    for carrier in startup_carriers().chain(optional.iter().copied()) {
        fs::write(source.join(carrier.output), carrier.name.as_bytes()).unwrap();
    }
    // Unsupported optional files must stay outside the browser bundle.
    fs::write(source.join(carriers::EQUIPMENT.output), b"unused").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_manifest"))
        .arg(&source)
        .arg("--copy-to")
        .arg(root.path().join("bundle"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: DeliveryManifest = serde_json::from_slice(&output.stdout).unwrap();
    manifest.validate().unwrap();
    (root, manifest)
}

#[test]
fn bundle_preserves_optional_assets_when_present() {
    for optional in [
        vec![],
        vec![&carriers::FONT],
        vec![&carriers::AUDIO],
        vec![&carriers::AUDIO_BANK],
        vec![&carriers::FONT, &carriers::AUDIO, &carriers::AUDIO_BANK],
    ] {
        let (root, manifest) = stage(&optional);
        let bundle = root.path().join("bundle");
        for carrier in startup_carriers().chain(optional.iter().copied()) {
            assert_eq!(
                fs::read(bundle.join(carrier.output))
                    .unwrap_or_else(|error| { panic!("bundle omitted {}: {error}", carrier.name) }),
                carrier.name.as_bytes(),
            );
        }
        assert!(!bundle.join(carriers::EQUIPMENT.output).exists());
        for carrier in [&carriers::FONT, &carriers::AUDIO] {
            assert_eq!(
                manifest
                    .entries
                    .iter()
                    .any(|entry| entry.name == carrier.name),
                optional
                    .iter()
                    .any(|selected| selected.name == carrier.name),
                "manifest selection for {}",
                carrier.name,
            );
        }
        assert!(
            manifest
                .entries
                .iter()
                .all(|entry| entry.name != carriers::AUDIO_BANK.name)
        );
        let mut delivered = MemoryCarriers::new(manifest).unwrap();
        let selected: Vec<_> = delivered.entries().collect();
        for carrier in selected {
            delivered
                .insert(carrier.name, fs::read(bundle.join(carrier.output)).unwrap())
                .unwrap();
        }
    }
}
