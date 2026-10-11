use std::{path::Path, process::Command};

#[test]
fn make_physics_assets_selects_the_compiled_geometry_carrier() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    if !matches!(Command::new("make").arg("--version").output(), Ok(output) if output.status.success())
    {
        eprintln!("skipping physics Makefile test: missing make executable");
        return;
    }
    let output = Command::new("make")
        .current_dir(root)
        .args(["-n", "physics-assets", "ASSETS_PREPARE=physics-compiler"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "physics-compiler --only physics"
    );
}
