//! Pack-stack fixtures for tests of the crates that read admitted packs.

use std::io::{Cursor, Write};

/// Counts uncached inflate/decrypt attempts for a file, shared across aliases and pack clones.
pub fn file_read_count(pack: &crate::ValidatedPack, path: &str) -> Option<usize> {
    pack.entry(path).map(|entry| {
        entry
            .cached
            .reads
            .load(std::sync::atomic::Ordering::Relaxed)
    })
}

/// Admits an archive containing the supplied artwork as a server pack.
pub fn pack_view(files: &[(String, Vec<u8>)]) -> crate::LayeredPackView {
    layered_pack_view(&[files])
}

/// Admits fixture layers in exact lowest-to-highest precedence order.
pub fn layered_pack_view(layers: &[&[(String, Vec<u8>)]]) -> crate::LayeredPackView {
    let archives = layers
        .iter()
        .enumerate()
        .rev()
        .map(|(index, files)| {
            let id = format!("00000000-0000-0000-0000-{:012x}", index + 17);
            bedrock_packs::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                archive_bytes(id.parse().unwrap(), files),
            )
        })
        .collect();
    crate::LayeredPackView::new(crate::validate_handoff(
        bedrock_packs::ResourcePackHandoff::from_archives(archives),
    ))
}

/// Builds a small original archive with a resource manifest and the supplied files.
pub fn archive_bytes(id: uuid::Uuid, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    archive_bytes_with_module(id, files, "resources")
}

/// Builds a fixture with one explicit module while retaining the same manifest and ZIP writer.
pub fn archive_bytes_with_module(
    id: uuid::Uuid,
    files: &[(String, Vec<u8>)],
    module: &str,
) -> Vec<u8> {
    let module = serde_json::to_string(module).expect("fixture module string");
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":{module}}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in std::iter::once(("manifest.json", manifest.as_bytes())).chain(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    ) {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
