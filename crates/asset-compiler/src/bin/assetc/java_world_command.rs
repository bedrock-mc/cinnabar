//! Per-install acquisition of the optional, pinned Java world resources.

use std::{error::Error, fs, io::Read, path::Path};

/// Authenticates the shared pin, downloads to temporary storage and writes only a visual carrier.
pub(super) fn prepare(manifest: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(manifest)?;
    if assets::canonical_source_manifest_sha256(&bytes)
        != assets::canonical_source_manifest_sha256(assets::java_world::SOURCE_MANIFEST.as_bytes())
    {
        return Err("Java world source manifest differs from the compiled pin".into());
    }
    let source =
        assets::java_world::JavaWorldSource::pinned().map_err(|error| error.to_string())?;
    let temporary = tempfile::NamedTempFile::new()?;
    super::vanilla_pack_command::download(&source.url, temporary.path())?;
    let file = fs::File::open(temporary.path())?;
    let mut client = Vec::new();
    file.take(source.bytes as u64 + 1)
        .read_to_end(&mut client)?;
    let carrier =
        assets::java_world::compile_java_world(&client).map_err(|error| error.to_string())?;
    super::write_blob_atomic(out, &carrier)?;
    Ok(())
}
