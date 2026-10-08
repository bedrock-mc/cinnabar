use std::io::Write;

use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

const PACK_ID: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);
const MODULE_ID: Uuid = Uuid::from_u128(0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);

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
    validate_archive_parts(PACK_ID, "1.2.3", selected, bytes, None, None).map(|(pack, _)| pack)
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
    let (pack, _) =
        validate_archive_parts(PACK_ID, "1.2.3", "", archive.clone(), key, None).unwrap();
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
        validate_archive_parts(PACK_ID, "1.2.3", "", archive, wrong, None).unwrap_err(),
        AdmissionError::MalformedContentsIndex
    );
    let unindexed = zip_files(&[("manifest.json", manifest.as_bytes())]);
    assert_eq!(
        validate_archive_parts(
            PACK_ID,
            "1.2.3",
            "",
            unindexed,
            ContentKey::new(PACK_KEY),
            None
        )
        .unwrap_err(),
        AdmissionError::MissingContentsIndex
    );
}

#[test]
fn rejects_malformed_jsonc_and_multiple_json_values() {
    for body in [
        br#"{"format_version":2 /* unterminated"#.as_slice(),
        br#"{} {}"#.as_slice(),
    ] {
        let archive = zip_files(&[("manifest.json", body)]);
        assert_eq!(
            validate_fixture(archive, "").unwrap_err(),
            AdmissionError::MalformedManifest
        );
    }

    let invalid_utf8_comment = b"// \xff\n{}";
    let archive = zip_files(&[("manifest.json", invalid_utf8_comment)]);
    assert_eq!(
        validate_fixture(archive, "").unwrap_err(),
        AdmissionError::MalformedManifest
    );
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
