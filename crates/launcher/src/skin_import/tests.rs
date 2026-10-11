use super::*;
use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};

/// Makes an original, drawable non-humanoid model for import tests.
fn geometry(name: &str) -> Vec<u8> {
    serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":name,"texture_width":64,"texture_height":64},
        "bones":[{"name":"body","cubes":[{"origin":[-12,0,-4],"size":[24,8,8],"uv":[0,0]}]}]
    }]})
    .to_string()
    .into_bytes()
}

/// Builds a small ZIP from exact paths without relying on local game assets.
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Supplies the metadata required by a skin-pack import.
fn manifest() -> Vec<u8> {
    serde_json::json!({"format_version":1,"header":{"name":"Fixture","min_engine_version":[1,21,0]},"modules":[{"type":"skin_pack"}]}).to_string().into_bytes()
}

/// Makes a declaration pointing to one explicit texture and geometry.
fn catalog(name: &str, texture: &str) -> Vec<u8> {
    serde_json::json!({"skins":[{"localization_name":"Fixture","texture":texture,"geometry":name,"type":"free"}]}).to_string().into_bytes()
}

#[test]
fn pack_preserves_named_geometry_and_engine_version_in_a_wrapped_root() {
    let name = "geometry.import.fixture";
    let bytes = archive(&[
        ("pack/manifest.json", &manifest()),
        ("pack/skins.json", &catalog(name, "textures/body.png")),
        ("pack/geometry.json", &geometry(name)),
        ("pack/textures/body.png", b"fixture PNG"),
    ]);
    let entries = parse_skin_pack(&bytes).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].model, SkinModel::Custom);
    assert_eq!(entries[0].engine_version.as_ref(), "1.21.0");
    assert_eq!(entries[0].png, b"fixture PNG");
    assert_eq!(
        assets::skin_geometry_name(&entries[0].geometry.as_ref().unwrap().resource_patch)
            .as_deref(),
        Some(name)
    );
}

#[test]
fn legacy_geometry_resolves_inheritance_without_changing_its_source() {
    let data = serde_json::json!({"format_version":"1.8.0","geometry.base":{"texturewidth":64,"textureheight":64,"bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}]},"geometry.fixture:geometry.base":{"bones":[{"name":"body","cubes":[{"origin":[8,0,0],"size":[8,8,8],"uv":[0,0]}]}]}}).to_string();
    let source = parse_skin_geometry(data.as_bytes(), Some("geometry.fixture")).unwrap();
    assert_eq!(source.geometry_data.as_ref(), data);
    let model = assets::parse_skin_geometry(&source.resource_patch, &source.geometry_data)
        .unwrap()
        .unwrap();
    assert_eq!(model.bones[0].cubes.len(), 2);
    assert!(
        parse_skin_geometry(data.as_bytes(), None).is_err(),
        "ambiguous loose models need a pack declaration"
    );
}

#[test]
fn malformed_missing_cyclic_and_oversized_geometry_are_rejected() {
    for data in [
        b"{broken".as_slice(),
        b"null",
        b"{}",
        br#"{"geometry.fixture":{"bones":[{"name":"a","parent":"b"},{"name":"b","parent":"a"}]}}"#,
    ] {
        assert!(parse_skin_geometry(data, None).is_err());
    }
    assert!(parse_skin_geometry(&geometry("geometry.fixture"), Some("geometry.missing")).is_err());
    assert!(matches!(
        parse_skin_geometry(
            &vec![b' '; render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES + 1],
            None
        ),
        Err(SkinImportError::TooLarge)
    ));
    let bones = (0..=assets::MAX_SKIN_GEOMETRY_BONES)
        .map(|i| serde_json::json!({"name":format!("bone{i}")}))
        .collect::<Vec<_>>();
    let data = serde_json::json!({"geometry.fixture":{"bones":bones}}).to_string();
    assert!(parse_skin_geometry(data.as_bytes(), None).is_err());
}

#[test]
fn malformed_archives_and_unsafe_or_duplicate_paths_are_rejected() {
    assert!(parse_skin_pack(b"not a ZIP").is_err());
    for path in [
        "../outside.png",
        "/outside.png",
        "drive:outside.png",
        "folder\\outside.png",
    ] {
        let bytes = archive(&[
            ("manifest.json", &manifest()),
            ("skins.json", &catalog(SkinModel::Classic.geometry(), path)),
            (path, b"fixture"),
        ]);
        assert!(parse_skin_pack(&bytes).is_err());
    }
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        (
            "skins.json",
            &catalog(SkinModel::Classic.geometry(), "skin.png"),
        ),
        ("skin.png", b"one"),
        ("SKIN.PNG", b"two"),
    ]);
    assert!(matches!(
        parse_skin_pack(&bytes),
        Err(SkinImportError::Archive)
    ));
}

#[test]
fn oversized_compressed_sources_and_declared_members_are_rejected() {
    assert!(matches!(
        parse_skin_pack(&vec![0; resource_pack::MAX_ARCHIVE_BYTES + 1]),
        Err(SkinImportError::TooLarge)
    ));
    let oversized = vec![0; resource_pack::MAX_PACK_TEXTURE_BYTES as usize + 1];
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        (
            "skins.json",
            &catalog(SkinModel::Classic.geometry(), "skin.png"),
        ),
        ("skin.png", &oversized),
    ]);
    assert!(matches!(
        parse_skin_pack(&bytes),
        Err(SkinImportError::TooLarge)
    ));
}

#[test]
fn missing_models_and_non_skin_pack_modules_are_rejected() {
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        ("skins.json", &catalog("geometry.missing", "skin.png")),
        ("skin.png", b"fixture"),
    ]);
    assert!(matches!(
        parse_skin_pack(&bytes),
        Err(SkinImportError::MissingFile)
    ));
    let manifest = serde_json::json!({"modules":[{"type":"resources"}]}).to_string();
    let bytes = archive(&[
        ("manifest.json", manifest.as_bytes()),
        (
            "skins.json",
            &catalog(SkinModel::Classic.geometry(), "skin.png"),
        ),
        ("skin.png", b"fixture"),
    ]);
    assert!(matches!(
        parse_skin_pack(&bytes),
        Err(SkinImportError::Unsupported)
    ));
}

#[test]
fn mixed_pack_builtin_entries_use_the_catalog_and_authored_overrides_stay_custom() {
    let name = "geometry.import.fixture";
    let mut entries: serde_json::Value =
        serde_json::from_slice(&catalog(name, "custom.png")).unwrap();
    entries["skins"].as_array_mut().unwrap().push(serde_json::json!({
        "localization_name":"Classic","texture":"classic.png","geometry":SkinModel::Classic.geometry(),"type":"free"
    }));
    let declarations = entries.to_string();
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        ("skins.json", declarations.as_bytes()),
        ("geometry.json", &geometry(name)),
        ("custom.png", b"fixture"),
        ("classic.png", b"fixture"),
    ]);
    let skins = parse_skin_pack(&bytes).unwrap();
    assert_eq!(skins[0].model, SkinModel::Custom);
    assert_eq!(skins[1].model, SkinModel::Classic);
    assert!(skins[1].geometry.is_none());
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        (
            "skins.json",
            &catalog(SkinModel::Classic.geometry(), "skin.png"),
        ),
        ("geometry.json", &geometry(SkinModel::Classic.geometry())),
        ("skin.png", b"fixture"),
    ]);
    assert_eq!(parse_skin_pack(&bytes).unwrap()[0].model, SkinModel::Custom);
}

#[test]
fn animated_skin_declarations_are_rejected_instead_of_losing_their_animation() {
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&catalog(SkinModel::Classic.geometry(), "skin.png")).unwrap();
    catalog["skins"][0]["animations"] = serde_json::json!({"move":"animation.fixture"});
    let catalog = catalog.to_string();
    let bytes = archive(&[
        ("manifest.json", &manifest()),
        ("skins.json", catalog.as_bytes()),
        ("skin.png", b"fixture"),
    ]);
    assert!(matches!(
        parse_skin_pack(&bytes),
        Err(SkinImportError::Unsupported)
    ));
}
