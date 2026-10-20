use resource_pack::ValidatedPackStack;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

/// Builds original test data into the same archive admission path used at runtime.
pub(crate) fn stack(files: &[(&str, &[u8])]) -> Arc<ValidatedPackStack> {
    stack_with_options(files, zip::write::SimpleFileOptions::default())
}

/// Admits complete Stored members without external fixtures or compression artifacts.
pub(crate) fn stored_stack(files: &[(&str, &[u8])]) -> Arc<ValidatedPackStack> {
    stack_with_options(
        files,
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
}

/// Admits original fixture data using the requested ZIP storage method.
fn stack_with_options(
    files: &[(&str, &[u8])],
    options: zip::write::SimpleFileOptions,
) -> Arc<ValidatedPackStack> {
    let id = "00000000-0000-0000-0000-00000000000a";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        zip.start_file(name, options).unwrap();
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
