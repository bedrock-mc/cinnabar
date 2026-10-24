//! Complete admission and reads for server archive content.

use super::*;
use crate::{LayeredPackView, ValidatedPackStack};

const PADDED_MANIFEST_BYTES: usize = 262_145;
const LARGE_FILE_BYTES: usize = 67_108_865;

/// Builds a safe 513-byte member name with short individual path components.
fn long_path() -> String {
    let directories = std::iter::repeat_n("a".repeat(49), 9)
        .collect::<Vec<_>>()
        .join("/");
    let path = format!("textures/{directories}/{}.png", "b".repeat(50));
    assert_eq!(path.len(), 513);
    path
}

/// Stores a valid manifest and the complete original payload without compression.
fn stored_archive(path: &str, payload: &[u8]) -> Vec<u8> {
    let manifest = manifest("");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in [("manifest.json", manifest.as_bytes()), (path, payload)] {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn server_content_keeps_long_safe_paths_and_rejects_traversal() {
    let path = long_path();
    let raw = format!("./{}", path.replace('/', "\\"));
    let unsafe_path = format!("{path}/../escape.txt");
    let manifest = manifest("");
    let archive = zip_files(&[
        ("manifest.json", manifest.as_bytes()),
        (&raw, b"complete"),
        (&unsafe_path, b"unsafe"),
    ]);
    let pack = validate_fixture(archive, "").expect("valid server pack");
    assert!(pack.contains(&path), "safe member must stay indexed");
    assert_eq!(pack.skipped_entries(), 1, "only traversal is skipped");
    assert_eq!(pack.file_size(&path), Some(8));
    assert_eq!(
        pack.read_file(&path).unwrap().unwrap().as_ref(),
        b"complete"
    );
}

#[test]
fn server_content_keeps_keys_for_long_safe_paths() {
    const PACK_KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    const FILE_KEY: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz012345";
    let path = long_path();
    let contents = serde_json::to_vec(&serde_json::json!({
        "content": [{
            "path": format!("./{}", path.replace('/', "\\")),
            "key": std::str::from_utf8(FILE_KEY).unwrap()
        }]
    }))
    .unwrap();
    let mut encrypted = b"complete plaintext".to_vec();
    crate::crypto::tests::encrypt(FILE_KEY, &mut encrypted);
    let manifest = manifest("");
    let archive = zip_files(&[
        ("manifest.json", manifest.as_bytes()),
        ("contents.json", &contents),
        (&path, &encrypted),
    ]);
    let (pack, _) = validate_archive_parts(
        PACK_ID,
        crate::manifest::Version([1, 2, 3]),
        "",
        archive,
        ContentKey::new(PACK_KEY),
        None,
    )
    .unwrap();
    assert_eq!(
        pack.read_file(&path).unwrap().unwrap().as_ref(),
        b"complete plaintext"
    );
}

#[test]
fn server_content_accepts_complete_padded_manifests() {
    let mut manifest = manifest("").into_bytes();
    manifest.resize(PADDED_MANIFEST_BYTES, b' ');
    let archive = zip_files(&[("manifest.json", &manifest), ("marker.txt", b"present")]);
    let pack = validate_fixture(archive, "").expect("valid padded manifest");
    assert_eq!(
        pack.read_file("manifest.json").unwrap().unwrap().as_ref(),
        manifest
    );
    assert_eq!(
        pack.read_file("marker.txt").unwrap().unwrap().as_ref(),
        b"present"
    );
}

#[test]
fn server_content_still_rejects_malformed_padded_manifests() {
    let mut manifest = manifest("").into_bytes();
    let closing = manifest.iter().rposition(|byte| *byte == b'}').unwrap();
    manifest[closing] = b' ';
    manifest.resize(PADDED_MANIFEST_BYTES, b' ');
    let archive = zip_files(&[("manifest.json", &manifest)]);
    assert_eq!(
        validate_fixture(archive, "").unwrap_err(),
        AdmissionError::MalformedManifest
    );
}

#[test]
fn server_content_reads_the_complete_large_stored_member() {
    let path = "complete.bin";
    let mut payload = vec![0x5a; LARGE_FILE_BYTES];
    payload[LARGE_FILE_BYTES - 8..].copy_from_slice(b"end-data");
    let pack = validate_fixture(stored_archive(path, &payload), "").unwrap();
    assert_eq!(pack.file_size(path), Some(payload.len() as u64));
    assert!(pack.contains(path));
    let retained = pack.reads.retained_bytes();
    assert_eq!(
        pack.read_file_with_limit(path, payload.len() as u64)
            .unwrap()
            .unwrap()
            .as_ref(),
        payload
    );
    assert_eq!(pack.read_file(path).unwrap().unwrap().as_ref(), payload);
    assert_eq!(pack.reads.retained_bytes(), retained);
    assert_eq!(
        pack.read_file_with_limit(path, payload.len() as u64 - 1),
        Err(AdmissionError::FileTooLarge)
    );
    let view = LayeredPackView::tracked(Arc::new(ValidatedPackStack {
        packs: vec![pack].into_boxed_slice(),
        rejections: Box::default(),
        fingerprint: Default::default(),
    }));
    assert_eq!(view.read(path).unwrap().as_ref(), payload);
    {
        let layers = view.read_layers(path);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].as_ref(), payload);
    }
    assert_eq!(
        view.layers()
            .next()
            .unwrap()
            .read_file(path)
            .unwrap()
            .unwrap()
            .as_ref(),
        payload
    );
    assert!(
        view.dependencies()
            .unwrap()
            .snapshot()
            .iter()
            .any(|dependency| {
                matches!(dependency, crate::PackDependency::File { path: recorded, limit }
            if recorded == path && *limit >= payload.len() as u64)
            })
    );
}

#[test]
fn server_content_still_rejects_corrupt_stored_data() {
    let path = "complete.bin";
    let mut archive = stored_archive(path, b"original bytes");
    let offset = ZipArchive::new(Cursor::new(&archive))
        .unwrap()
        .by_name(path)
        .unwrap()
        .data_start() as usize;
    archive[offset] ^= 1;
    let pack = validate_fixture(archive, "").unwrap();
    assert_eq!(pack.read_file(path), Err(AdmissionError::InvalidFileData));
}

#[test]
fn server_content_shared_paths_survive_local_import() {
    let path = long_path();
    let (report, mut archives) = crate::import::read_import(
        stored_archive(&path, b"complete"),
        std::path::Path::new("long-path.mcpack"),
    )
    .unwrap();
    assert!(report.rejected.is_empty());
    assert_eq!(archives.len(), 1);
    let (_, archive) = archives.remove(0);
    let pack = validate_fixture(archive, "").unwrap();
    assert_eq!(
        pack.read_file(&path).unwrap().unwrap().as_ref(),
        b"complete"
    );
}
