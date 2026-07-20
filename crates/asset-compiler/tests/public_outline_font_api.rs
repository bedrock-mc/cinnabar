use std::{fs, path::Path, process::Command};

use asset_compiler::{FontCompileError, OutlineFontConfig, compile_outline_font};
use sha2::{Digest, Sha256};

#[test]
fn invalid_outline_font_fails_closed() {
    let error = compile_outline_font(
        Path::new("font/Inter.ttf"),
        b"not a font",
        [0x42; 32],
        OutlineFontConfig::default(),
    )
    .unwrap_err();

    assert!(matches!(error, FontCompileError::OutlineFont { .. }));
}

#[test]
fn outline_font_config_rejects_unbounded_or_incomplete_ranges() {
    let missing_replacement = OutlineFontConfig {
        replacement_codepoint: 'A',
        ..OutlineFontConfig::default()
    };
    assert!(matches!(
        compile_outline_font(
            Path::new("font/Inter.ttf"),
            b"not a font",
            [0x42; 32],
            missing_replacement,
        ),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));

    let oversized = OutlineFontConfig {
        pixel_height: 4_097,
        ..OutlineFontConfig::default()
    };
    assert!(matches!(
        compile_outline_font(
            Path::new("font/Inter.ttf"),
            b"not a font",
            [0x42; 32],
            oversized,
        ),
        Err(FontCompileError::InvalidDescriptor { .. })
    ));
}

#[test]
fn outline_font_cli_rejects_bytes_outside_the_manifest_pin_before_rasterization() {
    let directory = tempfile::tempdir().unwrap();
    let font = directory.path().join("tampered.ttf");
    let manifest = directory.path().join("ui-font-source.json");
    let out = directory.path().join("font.mcbefont");
    let report = directory.path().join("font.json");
    let tampered = b"not the pinned font";
    fs::write(&font, tampered).unwrap();
    fs::write(
        &manifest,
        format!(
            concat!(
                "{{\n",
                "  \"font_size_bytes\": {},\n",
                "  \"font_sha256\": \"{}\",\n",
                "  \"rasterization\": {{\n",
                "    \"pixel_height\": 32,\n",
                "    \"atlas_side\": 2048,\n",
                "    \"replacement_codepoint\": 65533\n",
                "  }}\n",
                "}}\n"
            ),
            tampered.len(),
            "00".repeat(32)
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["outline-font-assets", "--font"])
        .arg(&font)
        .arg("--source-manifest")
        .arg(&manifest)
        .arg("--out")
        .arg(&out)
        .arg("--report")
        .arg(&report)
        .output()
        .expect("run outline-font-assets");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("outline font SHA-256 does not match the source manifest"),
        "unexpected rejection:\n{stderr}"
    );
    assert!(!out.exists());
    assert!(!report.exists());
}

#[test]
fn tracked_inter_license_is_the_exact_pinned_upstream_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("assets/licenses/Inter-OFL-1.1.txt");
    let bytes = fs::read(path).unwrap();

    assert_eq!(bytes.len(), 4_377);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "5b9321a4298cfeb6b34354164a1c3afc3db114569984c502b9b35d988fd58c57"
    );
    let attributes = fs::read_to_string(root.join(".gitattributes"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        attributes.lines().any(|line| {
            line == concat!(
                "assets/licenses/Inter-OFL-1.1.txt text eol=lf ",
                "whitespace=-blank-at-eol"
            )
        }),
        "the byte-exact license must retain LF bytes in fresh Windows checkouts"
    );
}
