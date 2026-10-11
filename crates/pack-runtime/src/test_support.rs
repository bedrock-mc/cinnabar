use resource_pack::ValidatedPackStack;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

/// Builds original test data into the same archive admission path used at runtime.
pub(crate) fn stack(files: &[(&str, &[u8])]) -> Arc<ValidatedPackStack> {
    let id = "00000000-0000-0000-0000-00000000000a";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
        protocol::ResourcePackArchive::unencrypted(
            id.parse().unwrap(),
            "1.0.0".into(),
            String::new(),
            zip.finish().unwrap().into_inner(),
        ),
    ]))
}
