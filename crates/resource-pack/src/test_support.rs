//! Pack-stack fixtures for tests of the crates that read admitted packs.

use std::io::{Cursor, Write};

/// Admits an archive containing the supplied artwork as a server pack.
pub fn pack_view(files: &[(String, Vec<u8>)]) -> crate::LayeredPackView {
    layered_pack_view(&[files])
}

/// Admits fixture layers in exact lowest-to-highest precedence order.
pub fn layered_pack_view(layers: &[&[(String, Vec<u8>)]]) -> crate::LayeredPackView {
    let archives = layers
        .iter()
        .enumerate()
        .map(|(index, files)| {
            let id = format!("00000000-0000-0000-0000-{:012x}", index + 17);
            let manifest = format!(
                r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
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
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                zip.finish().unwrap().into_inner(),
            )
        })
        .collect();
    crate::LayeredPackView::new(crate::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(archives),
    ))
}
