use std::io::Write;

use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use zip_footer::EOCD_MIN_BYTES;

const PACK_ID: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);
const MODULE_ID: Uuid = Uuid::from_u128(0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);

mod content;

fn manifest(extra: &str) -> String {
    format!(
        r#"{{
            // numeric v2 manifests are admitted
            "format_version": 2,
            "header": {{
                "name": "test // literal",
                "description": "bounded",
                "uuid": "{PACK_ID}",
                "version": [1, 2, 3]
            }},
            "modules": [{{
                "type": "resources",
                "uuid": "{MODULE_ID}",
                "version": [1, 2, 3]
            }}]
            {extra}
        }}"#
    )
}

fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .expect("start fixture file");
        writer.write_all(bytes).expect("write fixture file");
    }
    writer.finish().expect("finish fixture ZIP").into_inner()
}

fn deflated_zip_file(path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("start deflated fixture file");
    writer
        .write_all(bytes)
        .expect("write deflated fixture file");
    writer.finish().expect("finish fixture ZIP").into_inner()
}

fn validate_fixture(bytes: Vec<u8>, selected: &str) -> Result<ValidatedPack, AdmissionError> {
    validate_archive_parts(
        PACK_ID,
        "1.2.3",
        selected,
        bytes,
        None,
        None,
        Purpose::Rendering,
    )
    .map(|(pack, _, _)| pack)
}

/// Acquisition may retain opaque archives that the original rendering owner still refuses.
fn verify_opaque_identity(module: &str) {
    let manifest =
        manifest("").replace(r#""type": "resources""#, &format!(r#""type": "{module}""#));
    let archive = zip_files(&[("manifest.json", manifest.as_bytes())]);
    assert_eq!(
        validate_fixture(archive.clone(), "").err(),
        Some(AdmissionError::InvalidModules),
        "opaque {module} content must remain unavailable as a rendering layer"
    );
    assert!(
        verify_archive_identity(&archive, PACK_ID, "1.2.3").is_ok(),
        "valid opaque {module} archive must complete acquisition without rendering admission"
    );
}

#[test]
fn opaque_identity_keeps_behavior_archives_separate_from_rendering() {
    verify_opaque_identity("data");
}

#[test]
fn opaque_identity_keeps_client_data_archives_separate_from_rendering() {
    verify_opaque_identity("client_data");
}

#[test]
fn opaque_identity_keeps_skin_archives_separate_from_rendering() {
    verify_opaque_identity("skin_pack");
}

/// Reuses the original matching manifest and ZIP writer with one explicit modules shape.
fn archive_with_modules(modules: Option<serde_json::Value>) -> Vec<u8> {
    let mut root: serde_json::Value =
        bedrock_packs::jsonc::parse_jsonc_lenient(manifest("").as_bytes()).unwrap();
    let object = root.as_object_mut().expect("fixture manifest object");
    if let Some(modules) = modules {
        let _ = object.insert("modules".into(), modules);
    } else {
        let _ = object.remove("modules");
    }
    let bytes = serde_json::to_vec(&root).unwrap();
    zip_files(&[("manifest.json", &bytes)])
}

#[test]
fn opaque_module_shapes_preserve_null_defaults_and_unknown_strings() {
    use serde_json::json;
    for (modules, expected) in [
        (None, ArchiveKind::Skins),
        (Some(json!(null)), ArchiveKind::Skins),
        (Some(json!([])), ArchiveKind::Skins),
        (Some(json!([null])), ArchiveKind::Skins),
        (Some(json!([{}])), ArchiveKind::Skins),
        (Some(json!([{"type": null}])), ArchiveKind::Skins),
        (
            Some(json!([{"type": "future_module", "unused": {"value": true}}])),
            ArchiveKind::Skins,
        ),
        (Some(json!([null, {"type": "data"}])), ArchiveKind::Behavior),
    ] {
        let archive = archive_with_modules(modules.clone());
        assert_eq!(
            validate_fixture(archive.clone(), "").err(),
            Some(AdmissionError::InvalidModules)
        );
        assert_eq!(
            verify_archive_identity(&archive, PACK_ID, "1.2.3"),
            Ok(expected),
            "valid module defaults were rejected: {modules:?}"
        );
    }
}

#[test]
fn opaque_module_shapes_reject_wrong_containers_elements_and_types() {
    use serde_json::json;
    for modules in [
        json!("bad"),
        json!({}),
        json!(false),
        json!(1),
        json!(["bad"]),
        json!([[]]),
        json!([false]),
        json!([1]),
        json!([{"type": false}]),
        json!([{"type": 1}]),
        json!([{"type": []}]),
        json!([{"type": {}}]),
        json!([{"type": "data"}, {"type": false}]),
    ] {
        let archive = archive_with_modules(Some(modules.clone()));
        assert_eq!(
            validate_fixture(archive.clone(), "").err(),
            Some(AdmissionError::InvalidModules)
        );
        assert!(
            verify_archive_identity(&archive, PACK_ID, "1.2.3").is_err(),
            "malformed modules reached opaque delivery classification: {modules}"
        );
    }
}

#[test]
fn archive_delivery_kind_uses_the_same_admitted_manifest_and_member_priority() {
    for (module, member, expected) in [
        (None, None, ArchiveKind::Resources),
        (Some("data"), None, ArchiveKind::Addon),
        (Some("client_data"), None, ArchiveKind::Addon),
        (Some("script"), None, ArchiveKind::Resources),
        (
            Some("data"),
            Some("world/level.dat"),
            ArchiveKind::WorldTemplate,
        ),
        (Some("data"), Some("../level.dat"), ArchiveKind::Addon),
    ] {
        let mut manifest = manifest("");
        if let Some(module) = module {
            manifest = manifest.replace(
                r#""type": "resources","#,
                &format!(r#""type": "resources"}}, {{"type": "{module}","#),
            );
        }
        let mut files = vec![("manifest.json", manifest.as_bytes())];
        if let Some(path) = member {
            files.push((path, b"original world metadata"));
        }
        assert_eq!(
            verify_archive_identity(&zip_files(&files), PACK_ID, "1.2.3"),
            Ok(expected)
        );
    }
}

#[test]
fn opaque_archive_kinds_keep_the_original_delivery_priority() {
    for (module, member, expected) in [
        ("data", None, ArchiveKind::Behavior),
        ("client_data", None, ArchiveKind::Behavior),
        ("skin_pack", None, ArchiveKind::Skins),
        ("script", None, ArchiveKind::Skins),
        ("data", Some("world/level.dat"), ArchiveKind::WorldTemplate),
        ("data", Some("../level.dat"), ArchiveKind::Behavior),
    ] {
        let manifest =
            manifest("").replace(r#""type": "resources""#, &format!(r#""type": "{module}""#));
        let mut files = vec![("manifest.json", manifest.as_bytes())];
        if let Some(member) = member {
            files.push((member, b"original world metadata"));
        }
        let bytes = zip_files(&files);
        assert_eq!(
            verify_archive_identity(&bytes, PACK_ID, "1.2.3"),
            Ok(expected)
        );
        assert_eq!(
            validate_fixture(bytes, "").err(),
            Some(AdmissionError::InvalidModules)
        );
    }
}

#[test]
fn opaque_identity_still_checks_manifest_identity_and_format() {
    let manifest = manifest("").replace(r#""type": "resources""#, r#""type": "data""#);
    let archive = zip_files(&[("manifest.json", manifest.as_bytes())]);
    for (id, version) in [(Uuid::nil(), "1.2.3"), (PACK_ID, "1.2.4")] {
        assert_eq!(
            verify_archive_identity(&archive, id, version),
            Err(AdmissionError::ManifestIdentityMismatch)
        );
    }
    let manifest = manifest.replace(r#""format_version": 2"#, r#""format_version": 2.5"#);
    assert_eq!(
        verify_archive_identity(
            &zip_files(&[("manifest.json", manifest.as_bytes())]),
            PACK_ID,
            "1.2.3"
        ),
        Err(AdmissionError::UnsupportedManifestFormat)
    );
}

#[test]
fn admits_jsonc_manifest_and_exposes_only_selected_logical_namespace() {
    let manifest = manifest(
        r#", "subpacks": [
            {"folder_name":"low", "name":"Low", "memory_tier":1},
            {"folder_name":"high", "name":"High", "memory_tier":2}
        ] /* a block comment */"#,
    );
    let archive = zip_files(&[
        ("manifest.json", manifest.as_bytes()),
        ("textures/root.txt", b"root"),
        ("subpacks/low/textures/root.txt", b"low"),
        ("subpacks/high/textures/root.txt", b"high"),
        ("subpacks/high/textures/only.txt", b"only"),
    ]);
    let pack = validate_fixture(archive, "high").expect("valid selected subpack");

    assert_eq!(
        pack.read_file("textures/root.txt")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"high"
    );
    assert_eq!(
        pack.read_file("textures/only.txt")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"only"
    );
    assert!(
        pack.read_file("subpacks/low/textures/root.txt")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        pack.files_under("textures/").as_ref(),
        ["textures/only.txt", "textures/root.txt"]
    );
}
#[test]
fn root_selection_excludes_all_physical_subpacks() {
    let manifest =
        manifest(r#", "subpacks": [{"folder_name":"high", "name":"High", "memory_tier":2}]"#);
    let archive = zip_files(&[
        ("manifest.json", manifest.as_bytes()),
        ("base.txt", b"root"),
        ("subpacks/high/base.txt", b"high"),
    ]);
    let pack = validate_fixture(archive, "").expect("root selection");
    assert_eq!(
        pack.read_file("base.txt").unwrap().unwrap().as_ref(),
        b"root"
    );
    assert_eq!(pack.files_under("").as_ref(), ["base.txt", "manifest.json"]);
}

#[test]
fn undeclared_server_subpack_names_fall_back_to_root_resources() {
    for extra in [
        "",
        r#", "subpacks": [{"folder_name":"high", "name":"High", "memory_tier":2}]"#,
    ] {
        let manifest = manifest(extra);
        let archive = zip_files(&[
            ("manifest.json", manifest.as_bytes()),
            ("base.txt", b"root"),
            ("subpacks/high/base.txt", b"high"),
            ("subpacks/server label/base.txt", b"undeclared"),
        ]);
        let pack = validate_fixture(archive, "server label").expect("root fallback");
        assert_eq!(pack.sub_pack_name(), "server label");
        assert_eq!(
            pack.read_file("base.txt").unwrap().unwrap().as_ref(),
            b"root"
        );
        assert_eq!(pack.files_under("").as_ref(), ["base.txt", "manifest.json"]);
    }
}

#[test]
fn skips_unsafe_duplicate_and_nonfile_entries_without_dropping_the_pack() {
    let manifest = manifest("");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_directory("directory/", SimpleFileOptions::default())
        .unwrap();
    writer
        .add_symlink("link", "target", SimpleFileOptions::default())
        .unwrap();
    for (path, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("../x", b"x".as_slice()),
        ("Textures/x", b"a"),
        ("textures/X", b"b"),
        ("textures\\win.txt", b"w"),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let pack = validate_fixture(writer.finish().unwrap().into_inner(), "").expect("lenient pack");
    assert_eq!(
        pack.skipped_entries(),
        3,
        "traversal, symlink, case duplicate"
    );
    assert_eq!(
        pack.read_file("textures/x").unwrap().unwrap().as_ref(),
        b"a"
    );
    assert_eq!(
        pack.read_file("textures/win.txt")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"w"
    );
}

#[test]
fn unwraps_a_single_top_level_folder_holding_the_manifest() {
    let manifest = manifest("");
    let archive = zip_files(&[
        ("pack/manifest.json", manifest.as_bytes()),
        ("pack/texts/en_US.lang", b"a=b"),
        ("stray.txt", b"outside"),
    ]);
    let pack = validate_fixture(archive, "").expect("wrapped pack");
    assert_eq!(
        pack.files_under("").as_ref(),
        ["manifest.json", "texts/en_US.lang"]
    );
}

#[test]
fn rejects_zip64_sentinel_before_zip_parser_allocation() {
    let manifest = manifest("");
    let mut archive = zip_files(&[("manifest.json", manifest.as_bytes())]);
    let eocd = archive
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .unwrap();
    archive[eocd + 10..eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(
        preflight_eocd(&archive),
        Err(AdmissionError::UnsupportedZip64)
    );
}
#[test]
fn skips_zip_encrypted_and_unsupported_codec_entries() {
    let manifest = manifest("");
    let original = zip_files(&[("manifest.json", manifest.as_bytes()), ("a.txt", b"a")]);
    let second = |needle: &[u8]| {
        original
            .windows(4)
            .enumerate()
            .filter(|(_, window)| *window == needle)
            .nth(1)
            .unwrap()
            .0
    };
    let (local, central) = (second(b"PK\x03\x04"), second(b"PK\x01\x02"));

    let mut encrypted = original.clone();
    encrypted[local + 6..local + 8].copy_from_slice(&1u16.to_le_bytes());
    encrypted[central + 8..central + 10].copy_from_slice(&1u16.to_le_bytes());
    let mut unsupported = original.clone();
    unsupported[local + 8..local + 10].copy_from_slice(&12u16.to_le_bytes());
    unsupported[central + 10..central + 12].copy_from_slice(&12u16.to_le_bytes());
    for archive in [encrypted, unsupported] {
        let pack = validate_fixture(archive, "").expect("pack survives one bad entry");
        assert_eq!(pack.skipped_entries(), 1);
        assert!(pack.read_file("a.txt").unwrap().is_none());
    }
}

#[test]
fn plaintext_archives_with_advertised_keys_keep_root_and_wrapped_resources() {
    const PACK_KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    for format in [1, 2] {
        let manifest = manifest("").replace(
            "\"format_version\": 2",
            &format!("\"format_version\": {format}"),
        );
        for prefix in ["", "wrapped/", "./wrapped\\"] {
            let manifest_path = format!("{prefix}manifest.json");
            let asset_path = format!("{prefix}texts/en_US.lang");
            let archive = zip_files(&[
                (&manifest_path, manifest.as_bytes()),
                (&asset_path, b"fixture.name=Plain resource"),
                ("../outside.txt", b"outside"),
            ]);
            let (pack, _, _) = validate_archive_parts(
                PACK_ID,
                "1.2.3",
                "",
                archive,
                ContentKey::new(PACK_KEY),
                None,
                Purpose::Rendering,
            )
            .expect("plaintext resource pack with an advertised key");
            assert_eq!(
                pack.read_file("texts/en_US.lang").unwrap().as_deref(),
                Some(b"fixture.name=Plain resource".as_slice()),
            );
            assert!(pack.read_file("outside.txt").unwrap().is_none());
            assert_eq!(pack.skipped_entries(), 1);
        }
    }
}

#[test]
fn encrypted_pack_decrypts_listed_files_and_keeps_plaintext_despite_key() {
    const PACK_KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    const FILE_KEY: &str = "abcdefghijklmnopqrstuvwxyz012345";
    let index = format!(
        r#"{{"content":[{{"path":"texts/en_US.lang","key":"{FILE_KEY}"}},{{"path":"plain.json","key":"{FILE_KEY}"}}]}}"#
    );
    let mut contents = vec![0u8; 256];
    let mut body = index.into_bytes();
    crate::crypto::tests::encrypt(PACK_KEY, &mut body);
    contents.extend_from_slice(&body);
    let mut lang = b"a=secret".to_vec();
    crate::crypto::tests::encrypt(FILE_KEY.as_bytes(), &mut lang);
    let manifest = manifest("");
    let archive = zip_files(&[
        ("manifest.json", manifest.as_bytes()),
        ("contents.json", &contents),
        ("texts/en_US.lang", &lang),
        ("plain.json", b"{\"a\": 1}"),
    ]);
    let key = ContentKey::new(PACK_KEY);
    let (pack, _, _) = validate_archive_parts(
        PACK_ID,
        "1.2.3",
        "",
        archive.clone(),
        key,
        None,
        Purpose::Rendering,
    )
    .unwrap();
    assert_eq!(
        pack.read_file("texts/en_US.lang")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"a=secret"
    );
    assert_eq!(
        pack.read_file("plain.json").unwrap().unwrap().as_ref(),
        b"{\"a\": 1}"
    );
    assert!(!format!("{pack:?}").contains(FILE_KEY));

    let wrong = ContentKey::new(b"vutsrqponmlkjihgfedcba9876543210");
    assert_eq!(
        validate_archive_parts(
            PACK_ID,
            "1.2.3",
            "",
            archive,
            wrong,
            None,
            Purpose::Rendering
        )
        .unwrap_err(),
        AdmissionError::MalformedContentsIndex
    );
}

#[test]
fn local_plaintext_archive_with_advertised_key_reads_stored_resources() {
    let Some(path) = std::env::var_os("CINNABAR_PLAINTEXT_PACK_FIXTURE") else {
        eprintln!(
            "skipping local_plaintext_archive_with_advertised_key_reads_stored_resources: missing CINNABAR_PLAINTEXT_PACK_FIXTURE archive"
        );
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping local_plaintext_archive_with_advertised_key_reads_stored_resources: missing CINNABAR_PLAINTEXT_PACK_FIXTURE archive"
            );
            return;
        }
        Err(error) => panic!("read plaintext archive fixture: {error}"),
    };
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).expect("fixture ZIP");
    let manifest_index = (0..zip.len())
        .find(|index| {
            let file = zip.by_index(*index).unwrap();
            canonical_path(file.name_raw())
                .is_some_and(|path| MANIFEST_NAMES.contains(&path.rsplit('/').next().unwrap()))
        })
        .expect("fixture manifest");
    let manifest_path = canonical_path(zip.by_index(manifest_index).unwrap().name_raw()).unwrap();
    let root = manifest_path
        .rsplit_once('/')
        .map_or("", |(prefix, _)| &manifest_path[..prefix.len() + 1]);
    let manifest: serde_json::Value =
        serde_json::from_reader(zip.by_index(manifest_index).unwrap()).unwrap();
    let id = manifest["header"]["uuid"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let version = crate::manifest::Version::from_value(&manifest["header"]["version"])
        .unwrap()
        .0
        .map(|part| part.to_string())
        .join(".");
    let (pack, _, _) = validate_archive_parts(
        id,
        &version,
        "",
        bytes.clone(),
        ContentKey::new(b"0123456789abcdefghijklmnopqrstuv"),
        None,
        Purpose::Rendering,
    )
    .expect("keyed plaintext fixture admission");
    let mut compared = 0;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        if file.is_dir() {
            continue;
        }
        let Some(path) = canonical_path(file.name_raw()) else {
            continue;
        };
        let Some(logical) = path.strip_prefix(root) else {
            continue;
        };
        assert_ne!(
            logical, "contents.json",
            "fixture must have no encryption index"
        );
        let Some(actual) = pack.read_file(logical).unwrap() else {
            continue;
        };
        let mut stored = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut stored).unwrap();
        assert!(
            actual.as_ref() == stored.as_slice(),
            "fixture resource differs from stored bytes"
        );
        compared += 1;
    }
    assert!(
        compared > 1,
        "fixture must contain resources beyond its manifest"
    );
}

#[test]
fn advertised_key_does_not_bypass_bad_indexes_or_manifests() {
    const PACK_KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let manifest = manifest("");
    for prefix in ["", "wrapped/"] {
        let manifest_path = format!("{prefix}manifest.json");
        let contents_path = format!("{prefix}contents.json");
        let archive = zip_files(&[
            (&manifest_path, manifest.as_bytes()),
            (&contents_path, b"not an index"),
        ]);
        assert_eq!(
            validate_archive_parts(
                PACK_ID,
                "1.2.3",
                "",
                archive,
                ContentKey::new(PACK_KEY),
                None,
                Purpose::Rendering
            )
            .unwrap_err(),
            AdmissionError::MalformedContentsIndex,
        );
        let archive = zip_files(&[(&manifest_path, b"not a manifest")]);
        assert_eq!(
            validate_archive_parts(
                PACK_ID,
                "1.2.3",
                "",
                archive,
                ContentKey::new(PACK_KEY),
                None,
                Purpose::Rendering
            )
            .unwrap_err(),
            AdmissionError::MalformedManifest,
        );
    }
}

#[test]
fn unusable_contents_index_is_not_treated_as_plaintext() {
    let manifest = manifest("");
    let mut archive = zip_files(&[
        ("wrapped/manifest.json", manifest.as_bytes()),
        ("./wrapped\\contents.json", b"{}"),
    ]);
    let central = archive
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"PK\x01\x02")
        .nth(1)
        .unwrap()
        .0;
    archive[central + 8..central + 10].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        validate_archive_parts(
            PACK_ID,
            "1.2.3",
            "",
            archive,
            ContentKey::new(b"0123456789abcdefghijklmnopqrstuv"),
            None,
            Purpose::Rendering
        )
        .unwrap_err(),
        AdmissionError::MissingContentsIndex,
    );
}

#[test]
fn rejects_incomplete_manifest_jsonc() {
    let archive = zip_files(&[("manifest.json", br#"{"format_version":2 /* unterminated"#)]);
    assert_eq!(
        validate_fixture(archive, "").unwrap_err(),
        AdmissionError::MalformedManifest
    );
}

#[test]
fn manifest_uses_first_jsonc_value() {
    for tail in [" {}", " /* unfinished trailing comment"] {
        let body = format!("{}{tail}", manifest(""));
        let archive = zip_files(&[("manifest.json", body.as_bytes())]);
        validate_fixture(archive, "").expect("complete first manifest value");
    }
}

#[test]
fn admits_legacy_encoded_manifest_comments() {
    for comment in [b"// \xff\n".as_slice(), b"/* \xff */".as_slice()] {
        let mut body = comment.to_vec();
        body.extend_from_slice(manifest("").as_bytes());
        let archive = zip_files(&[("manifest.json", &body)]);
        validate_fixture(archive, "").expect("valid manifest with a Latin-1 comment");
    }
}

#[test]
fn manifest_read_is_bounded_by_its_forged_declared_size() {
    let body = manifest("");
    let mut archive = deflated_zip_file("manifest.json", body.as_bytes());
    let local = archive
        .windows(4)
        .position(|window| window == b"PK\x03\x04")
        .unwrap();
    let central = archive
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .unwrap();
    archive[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
    archive[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());

    assert_eq!(
        validate_fixture(archive, "").unwrap_err(),
        AdmissionError::InvalidFileData
    );
}

#[test]
fn admits_public_reference_scale_entry_count() {
    const REFERENCE_ENTRY_COUNT: usize = 17_805;
    let manifest = manifest("");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(manifest.as_bytes()).unwrap();
    for index in 1..REFERENCE_ENTRY_COUNT {
        writer
            .start_file(
                format!("textures/generated/{index:05}.txt"),
                SimpleFileOptions::default(),
            )
            .unwrap();
    }
    let archive = writer.finish().unwrap().into_inner();
    let pack = validate_fixture(archive, "").expect("reference-scale central directory");
    assert_eq!(pack.entry_count(), REFERENCE_ENTRY_COUNT);
}

#[test]
fn maximum_entry_subpack_overlay_uses_bounded_key_index() {
    let manifest =
        manifest(r#", "subpacks": [{"folder_name":"high", "name":"High", "memory_tier":2}]"#);
    let common = format!("assets/{}/", "a".repeat(420));
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(manifest.as_bytes()).unwrap();
    for index in 0..16_383 {
        writer
            .start_file(
                format!("{common}{index:05}.txt"),
                SimpleFileOptions::default(),
            )
            .unwrap();
    }
    for index in 0..16_384 {
        writer
            .start_file(
                format!("subpacks/high/{common}{index:05}.txt"),
                SimpleFileOptions::default(),
            )
            .unwrap();
    }
    let archive = writer.finish().unwrap().into_inner();
    let pack = validate_fixture(archive, "high").expect("maximum-entry selected subpack");
    assert_eq!(pack.entry_count(), MAX_ENTRIES_PER_PACK);
    assert_eq!(pack.files_under("assets/").len(), 16_384);
}

#[test]
fn errors_never_include_manifest_or_path_data() {
    let secret = "attacker-secret-marker";
    let archive = zip_files(&[("manifest.json", secret.as_bytes())]);
    let error = validate_fixture(archive, "").unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}

#[test]
fn review_zip_comment_can_contain_an_eocd_signature() {
    let mut archive = zip_files(&[("manifest.json", manifest("").as_bytes())]);
    let footer = archive.len() - EOCD_MIN_BYTES;
    archive[footer + 20..footer + 22].copy_from_slice(&24_u16.to_le_bytes());
    archive.extend_from_slice(b"PK\x05\x06");
    archive.extend_from_slice(&[0; 20]);
    assert!(preflight_eocd(&archive).is_ok());
    assert!(validate_fixture(archive.clone(), "").is_ok());
    let fake = footer + EOCD_MIN_BYTES;
    archive[fake + 20..fake + 22].copy_from_slice(&2_u16.to_le_bytes());
    assert!(preflight_eocd(&archive).is_ok());
}

#[test]
fn review_wrapped_legacy_manifests_and_subpack_protection() {
    let text = manifest(r#", "subpacks":[{"folder_name":"high","name":"High","memory_tier":1}]"#);
    let wrapped = zip_files(&[
        ("wrapper/pack_manifest.json", text.as_bytes()),
        ("wrapper/test.txt", b"fixture"),
    ]);
    assert!(validate_fixture(wrapped, "").is_ok());
    let shadowed = zip_files(&[
        ("pack_manifest.json", text.as_bytes()),
        ("subpacks/high/pack_manifest.json", text.as_bytes()),
    ]);
    assert!(matches!(
        validate_fixture(shadowed, "high"),
        Err(AdmissionError::InvalidSubpack)
    ));
}
