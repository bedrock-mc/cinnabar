use super::*;
use wasm_metadata::{AddMetadata, AddMetadataField};

/// Encodes an unsigned section length with standard LEB128 framing.
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

/// Appends one top-level custom section with standard length framing.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = Vec::new();
    length(name.len(), &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    length(payload.len(), bytes);
    bytes.extend(payload);
}

/// Encodes an opaque RGBA PNG of the given size.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&vec![200; (width * height * 4) as usize])
        .unwrap();
    writer.finish().unwrap();
    bytes
}

/// Writes `bytes` as a component file and reads its declarations.
fn read(bytes: &[u8]) -> Declarations {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wasm");
    std::fs::write(&path, bytes).unwrap();
    declarations(&path).unwrap()
}

/// The same component-name section `wasm-tools metadata add --name` writes.
fn named(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut add = AddMetadata::default();
    add.name = AddMetadataField::Set(name.into());
    add.to_wasm(bytes).unwrap()
}

#[test]
fn standard_sections_are_read_from_a_component_that_would_trap_if_run() {
    let trapping = wat::parse_str(
        "(component
            (core module $m (func $trap unreachable) (start $trap))
            (core instance (instantiate $m)))",
    )
    .unwrap();
    let engine = wasmtime::Engine::default();
    let component = wasmtime::component::Component::new(&engine, &trapping).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    assert!(
        wasmtime::component::Linker::<()>::new(&engine)
            .instantiate(&mut store, &component)
            .is_err()
    );
    let mut bytes = named(&trapping, "Zoom Plus");
    section(&mut bytes, "description", b"Smooth zoom\nfor any key.");
    section(&mut bytes, "version", b"1.2.0");
    section(&mut bytes, "authors", b"Alex <alex@example.com>");
    section(&mut bytes, "licenses", b"MIT OR Apache-2.0");
    let icon = png(16, 16);
    section(&mut bytes, mod_api::ICON_SECTION, &icon);
    let metadata = read(&bytes).metadata;
    assert_eq!(metadata.name.as_deref(), Some("Zoom Plus"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Smooth zoom for any key.")
    );
    assert_eq!(metadata.version.as_deref(), Some("1.2.0"));
    assert_eq!(metadata.authors.as_deref(), Some("Alex <alex@example.com>"));
    assert_eq!(metadata.licenses.as_deref(), Some("MIT OR Apache-2.0"));
    assert_eq!(metadata.icon.as_deref(), Some(icon.as_slice()));
}

#[test]
fn missing_malformed_or_oversized_fields_are_empty_and_last_wins() {
    let empty = wat::parse_str("(component)").unwrap();
    assert_eq!(read(&empty).metadata, ModMetadata::default());
    assert_eq!(read(b"(component)").metadata, ModMetadata::default());
    for (name, max) in [
        ("description", MAX_DESCRIPTION_BYTES),
        ("version", MAX_VERSION_BYTES),
        ("authors", MAX_AUTHORS_BYTES),
        ("licenses", MAX_LICENSES_BYTES),
    ] {
        for data in [
            b"\xff".to_vec(),
            b"bell\x07".to_vec(),
            b" \n\t ".to_vec(),
            vec![b'x'; max + 1],
        ] {
            let mut bytes = empty.clone();
            section(&mut bytes, name, &data);
            assert_eq!(read(&bytes).metadata, ModMetadata::default(), "{name}");
        }
        let mut bytes = empty.clone();
        section(&mut bytes, name, &vec![b'x'; max]);
        assert!(read(&bytes).metadata != ModMetadata::default(), "{name}");
    }
    let long = "x".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(read(&named(&empty, &long)).metadata.name, None);
    let mut bytes = empty.clone();
    section(&mut bytes, "version", b"1.0.0");
    section(&mut bytes, "version", b"2.0.0");
    assert_eq!(read(&bytes).metadata.version.as_deref(), Some("2.0.0"));
    let mut nested = wat::parse_str("(component)").unwrap();
    section(&mut nested, "version", b"9.9.9");
    let mut outer = empty.clone();
    outer.push(4);
    outer.push(nested.len() as u8);
    outer.extend(nested);
    assert_eq!(read(&outer).metadata.version, None);
}

#[test]
fn icons_must_be_one_bounded_png_that_decodes() {
    let empty = wat::parse_str("(component)").unwrap();
    let side = mod_api::MAX_ICON_SIDE;
    let largest = png(side, side);
    let mut padded = png(1, 1);
    padded.resize(mod_api::MAX_ICON_BYTES + 1, 0);
    let truncated = largest[..largest.len() / 2].to_vec();
    for (icon, valid) in [
        (largest.clone(), true),
        (png(side + 1, 1), false),
        (padded, false),
        (truncated, false),
        (b"not a PNG".to_vec(), false),
    ] {
        let mut bytes = empty.clone();
        section(&mut bytes, mod_api::ICON_SECTION, &icon);
        assert_eq!(read(&bytes).metadata.icon.is_some(), valid);
    }
    let mut duplicate = empty.clone();
    section(&mut duplicate, mod_api::ICON_SECTION, &largest);
    section(&mut duplicate, mod_api::ICON_SECTION, &largest);
    assert_eq!(read(&duplicate).metadata.icon, None);
}

#[test]
fn requests_are_optional_validated_and_never_hide_metadata() {
    let empty = wat::parse_str("(component)").unwrap();
    assert!(read(&empty).requested.unwrap().is_none());
    let mut bytes = empty.clone();
    section(
        &mut bytes,
        REQUESTED_GRANTS_SECTION,
        br#"{"camera":true,"commands":["ability"]}"#,
    );
    let request = read(&bytes).requested.unwrap().unwrap();
    assert!(request.camera);
    assert_eq!(request.commands, ["ability"]);
    assert!(!ModGrants::default().camera);
    for invalid in [r#"{"root":true}"#, r#"{"commands":["/ability"]}"#] {
        let mut bytes = named(&empty, "Named");
        section(&mut bytes, REQUESTED_GRANTS_SECTION, invalid.as_bytes());
        let declared = read(&bytes);
        assert!(declared.requested.is_err());
        assert_eq!(declared.metadata.name.as_deref(), Some("Named"));
    }
}

#[test]
fn update_repository_is_optional_top_level_and_never_breaks_other_metadata() {
    let empty = wat::parse_str("(component)").unwrap();
    assert!(read(&empty).metadata.update_repository.is_none());
    let mut valid = named(&empty, "Zoom");
    section(
        &mut valid,
        mod_api::UPDATE_REPOSITORY_SECTION,
        b"https://github.com/Example/Zoom.git",
    );
    let metadata = read(&valid).metadata;
    assert_eq!(metadata.update_repository.unwrap().slug(), "example/zoom");
    assert_eq!(metadata.name.as_deref(), Some("Zoom"));

    for data in [
        b"https://other.example/zoom".as_slice(),
        b"\xff",
        b"https://github.com/example/zoom/releases",
    ] {
        let mut invalid = named(&empty, "Zoom");
        section(&mut invalid, mod_api::UPDATE_REPOSITORY_SECTION, data);
        let metadata = read(&invalid).metadata;
        assert!(metadata.update_repository.is_none());
        assert_eq!(metadata.name.as_deref(), Some("Zoom"));
    }
    let mut duplicate = valid.clone();
    section(
        &mut duplicate,
        mod_api::UPDATE_REPOSITORY_SECTION,
        b"https://github.com/example/other",
    );
    assert!(read(&duplicate).metadata.update_repository.is_none());
    // Encode a nested component with its own repository; the outer component opts out.
    let mut outer = empty;
    outer.push(4);
    length(valid.len(), &mut outer);
    outer.extend(valid);
    assert!(read(&outer).metadata.update_repository.is_none());
}

#[test]
fn automatic_update_url_requires_one_valid_top_level_declaration() {
    let empty = wat::parse_str("(component)").unwrap();
    assert!(read(&empty).metadata.update_url.is_none());
    let mut valid = named(&empty, "Zoom");
    let url = b"https://github.com/Example/Zoom/releases/latest/download/zoom.wasm";
    section(&mut valid, mod_api::UPDATE_URL_SECTION, url);
    let metadata = read(&valid).metadata;
    let source = metadata.update_url.unwrap();
    assert_eq!(source.check_key(), "example/zoom");
    assert_eq!(source.asset_key(), "zoom.wasm");
    for data in [
        b"http://example.com/update.json".as_slice(),
        b"\xff",
        b"https://user:password@example.com/update.json",
    ] {
        let mut invalid = named(&empty, "Zoom");
        section(&mut invalid, mod_api::UPDATE_URL_SECTION, data);
        let metadata = read(&invalid).metadata;
        assert!(metadata.update_url.is_none());
        assert_eq!(metadata.name.as_deref(), Some("Zoom"));
    }
    let mut duplicate = valid.clone();
    section(&mut duplicate, mod_api::UPDATE_URL_SECTION, url);
    assert!(read(&duplicate).metadata.update_url.is_none());
    let mut nested = empty;
    nested.push(4);
    length(valid.len(), &mut nested);
    nested.extend(valid);
    assert!(read(&nested).metadata.update_url.is_none());
}

#[test]
fn https_manifest_declaration_retains_the_full_author_source() {
    let mut bytes = wat::parse_str("(component)").unwrap();
    let source = "https://mods.example/zoom/update.json";
    section(&mut bytes, mod_api::UPDATE_URL_SECTION, source.as_bytes());
    let metadata = read(&bytes).metadata;
    assert!(metadata.update_repository.is_none());
    let selected = metadata.update_url.unwrap();
    assert_eq!(selected.check_key(), source);
    assert_eq!(selected.asset_key(), source);
}
