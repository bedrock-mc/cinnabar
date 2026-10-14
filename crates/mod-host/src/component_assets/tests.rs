use super::*;

/// Writes a standard unsigned section length for the original binary fixtures.
fn length(mut value: usize, bytes: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        bytes.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            break;
        }
    }
}

/// Adds caller-owned data without invoking a guest or embedding a filesystem path.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = Vec::new();
    length(name.len(), &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    length(payload.len(), bytes);
    bytes.extend(payload);
}

/// A valid existing bundle with no product layout or assets.
fn bundle(title: &str) -> String {
    serde_json::json!({"panel":{"title":title,"toggle_key":"ShiftRight","dark":true,"controls":[]},"surfaces":{}}).to_string()
}

#[test]
fn static_assets_leave_executable_and_unrelated_metadata_bytes_unchanged() {
    let mut original = wat::parse_str("(component)").unwrap();
    section(&mut original, "example:notice", b"original license text");
    let mut source = original.clone();
    section(
        &mut source,
        mod_api::PANEL_BUNDLE_SECTION,
        bundle("Local").as_bytes(),
    );
    section(
        &mut source,
        mod_api::PANEL_FONT_SECTION,
        b"bounded font source",
    );
    let assets = ComponentAssets::read(&source).unwrap();
    assert_eq!(assets.executable.as_ref(), original);
    assert_eq!(assets.panel_bundle.as_ref().unwrap().panel.title, "Local");
    assert_eq!(
        assets.font.as_deref(),
        Some(b"bounded font source".as_slice())
    );
    assert!(
        ComponentAssets::read(&original)
            .unwrap()
            .panel_bundle
            .is_none()
    );
}

#[test]
fn known_sections_are_singletons_top_level_and_versioned() {
    for name in mod_api::COMPONENT_ASSET_SECTIONS {
        let data = if name == mod_api::PANEL_BUNDLE_SECTION {
            bundle("Local").into_bytes()
        } else {
            b"font".to_vec()
        };
        let mut duplicate = wat::parse_str("(component)").unwrap();
        section(&mut duplicate, name, &data);
        section(&mut duplicate, name, &data);
        assert!(ComponentAssets::read(&duplicate).is_err());
        for (id, text) in [(1, "(module)"), (4, "(component)")] {
            let mut nested = wat::parse_str(text).unwrap();
            section(&mut nested, name, &data);
            let mut outer = wat::parse_str("(component)").unwrap();
            outer.push(id);
            length(nested.len(), &mut outer);
            outer.extend(nested);
            assert!(ComponentAssets::read(&outer).is_err());
        }
        let mut core = wat::parse_str("(module)").unwrap();
        section(&mut core, name, &data);
        assert!(ComponentAssets::read(&core).is_err());
        let mut unknown_version = wat::parse_str("(component)").unwrap();
        section(&mut unknown_version, &name.replace("@1", "@2"), &data);
        assert!(ComponentAssets::read(&unknown_version).is_err());
    }
}

#[test]
fn invalid_utf8_json_and_truncated_binary_fail_admission() {
    for data in [b"\xff".as_slice(), b"not JSON", b"{}"] {
        let mut bytes = wat::parse_str("(component)").unwrap();
        section(&mut bytes, mod_api::PANEL_BUNDLE_SECTION, data);
        assert!(ComponentAssets::read(&bytes).is_err());
    }
    let mut bytes = wat::parse_str("(component)").unwrap();
    section(&mut bytes, mod_api::PANEL_FONT_SECTION, b"font");
    bytes.pop();
    assert!(ComponentAssets::read(&bytes).is_err());
}

#[test]
fn source_asset_and_executable_ceilings_are_independent() {
    assert!(ComponentAssets::read(&vec![0; MAX_COMPONENT_SOURCE_BYTES + 1]).is_err());
    let mut source = wat::parse_str("(component)").unwrap();
    section(
        &mut source,
        mod_api::PANEL_FONT_SECTION,
        &vec![0; mod_api::MAX_PANEL_FONT_BYTES],
    );
    assert_eq!(ComponentAssets::read(&source).unwrap().executable.len(), 8);
    let mut over = wat::parse_str("(component)").unwrap();
    section(
        &mut over,
        mod_api::PANEL_FONT_SECTION,
        &vec![0; mod_api::MAX_PANEL_FONT_BYTES + 1],
    );
    assert!(ComponentAssets::read(&over).is_err());
    let mut over = wat::parse_str("(component)").unwrap();
    section(
        &mut over,
        mod_api::PANEL_BUNDLE_SECTION,
        &vec![b' '; ui::mod_panel::MAX_PANEL_BUNDLE_BYTES + 1],
    );
    assert!(ComponentAssets::read(&over).is_err());
    section(
        &mut source,
        "unknown:padding",
        &vec![0; MAX_COMPONENT_BYTES],
    );
    assert!(source.len() < MAX_COMPONENT_SOURCE_BYTES);
    assert!(ComponentAssets::read(&source).is_err());
}

#[test]
fn stripping_preserves_nonminimal_valid_section_framing() {
    let mut original = wat::parse_str("(component)").unwrap();
    // A one-byte custom-section name uses an overlong but valid u32 section length.
    original.extend_from_slice(&[0, 0x82, 0x00, 1, b'x']);
    let mut source = original.clone();
    section(&mut source, mod_api::PANEL_FONT_SECTION, b"font");
    assert_eq!(
        ComponentAssets::read(&source).unwrap().executable.as_ref(),
        original
    );
}

#[test]
fn branding_and_notices_are_bounded_utf8_data_stripped_without_product_schema() {
    let original = wat::parse_str("(component)").unwrap();
    let mut source = original.clone();
    section(
        &mut source,
        mod_api::MENU_LOGO_SECTION,
        b"bounded PNG source",
    );
    section(&mut source, mod_api::MENU_TITLE_SECTION, b"EXAMPLE");
    section(
        &mut source,
        mod_api::NOTICES_SECTION,
        b"Original attribution\nLicense terms",
    );
    let assets = ComponentAssets::read(&source).unwrap();
    assert_eq!(assets.executable.as_ref(), original);
    assert_eq!(assets.title.as_deref(), Some("EXAMPLE"));
    assert_eq!(
        assets.notices.as_deref(),
        Some("Original attribution\nLicense terms")
    );
    for (name, max) in [
        (mod_api::MENU_LOGO_SECTION, mod_api::MAX_MENU_LOGO_BYTES),
        (mod_api::MENU_TITLE_SECTION, mod_api::MAX_MENU_TITLE_BYTES),
        (mod_api::NOTICES_SECTION, mod_api::MAX_NOTICES_BYTES),
    ] {
        let mut bytes = original.clone();
        section(&mut bytes, name, &vec![b'x'; max + 1]);
        assert!(ComponentAssets::read(&bytes).is_err());
    }
    for (name, data) in [
        (mod_api::MENU_TITLE_SECTION, b"\xff".as_slice()),
        (mod_api::MENU_TITLE_SECTION, b"two\nlines"),
        (mod_api::NOTICES_SECTION, b"\xff"),
    ] {
        let mut bytes = original.clone();
        section(&mut bytes, name, data);
        assert!(ComponentAssets::read(&bytes).is_err());
    }
}

#[test]
fn directory_packages_retain_plain_components_and_reject_single_file_static_resources() {
    let mut plain = wat::parse_str("(component)").unwrap();
    section(&mut plain, "example:metadata", b"ordinary metadata");
    assert!(ComponentAssets::read_package(&plain).is_ok());
    for name in mod_api::COMPONENT_ASSET_SECTIONS {
        let mut bytes = plain.clone();
        let data = if name == mod_api::PANEL_BUNDLE_SECTION {
            bundle("Local").into_bytes()
        } else {
            b"resource".to_vec()
        };
        section(&mut bytes, name, &data);
        assert!(ComponentAssets::read(&bytes).is_ok());
        assert!(ComponentAssets::read_package(&bytes).is_err());
    }
}
