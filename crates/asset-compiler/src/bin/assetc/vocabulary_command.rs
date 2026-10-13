//! Deterministic vocabulary generation from the hash-checked sample archive.

use std::{error::Error, fs, path::Path};

use assets::{
    VanillaSource,
    vanilla_pack::{self, UnpackLimits},
};

use super::write_output_bundle;

/// Unpacks a verified source into isolated storage and writes its names-only derivatives.
pub(super) fn compile(
    archive: &Path,
    manifest: &Path,
    out: &Path,
    actors_out: &Path,
) -> Result<(), Box<dyn Error>> {
    let source = VanillaSource::read(manifest)?;
    let pinned = assets::vanilla_source();
    if source.commit != pinned.commit || source.sha256 != pinned.sha256 || source.tag != pinned.tag
    {
        return Err("vocabulary source does not match the pinned sample manifest".into());
    }
    let scratch = tempfile::tempdir()?;
    let paths = source.local_paths(scratch.path())?;
    fs::create_dir_all(paths.archive.parent().ok_or("archive has no parent")?)?;
    fs::copy(archive, &paths.archive)?;
    if vanilla_pack::sha256_file(&paths.archive)? != source.sha256.as_ref() {
        return Err("vocabulary archive does not match the pinned SHA-256".into());
    }
    vanilla_pack::unpack(&paths, &UnpackLimits::PINNED, &|| false)?;
    let (vocabulary, actors) = asset_compiler::vocabulary::generate(&paths.cache)?;
    write_output_bundle(&[(out, &vocabulary), (actors_out, &actors)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_archive_cannot_replace_existing_vocabularies() {
        let scratch = tempfile::tempdir().unwrap();
        let archive = scratch.path().join("pack.zip");
        let manifest = scratch.path().join("source.json");
        let out = scratch.path().join("vocabulary.tsv");
        let actors = scratch.path().join("actors.txt");
        fs::write(&archive, b"invalid archive").unwrap();
        fs::write(&manifest, assets::VANILLA_SOURCE_MANIFEST).unwrap();
        fs::write(
            &out,
            include_bytes!("../../../../assets/data/vanilla-pack-vocabulary-1.26.50.tsv"),
        )
        .unwrap();
        fs::write(
            &actors,
            include_bytes!("../../../../protocol/data/retail_actors_1_26_50.txt"),
        )
        .unwrap();
        let before = (fs::read(&out).unwrap(), fs::read(&actors).unwrap());
        let error = compile(&archive, &manifest, &out, &actors)
            .unwrap_err()
            .to_string();
        assert!(error.contains("pinned SHA-256"), "{error}");
        assert_eq!((fs::read(out).unwrap(), fs::read(actors).unwrap()), before);
    }
}
