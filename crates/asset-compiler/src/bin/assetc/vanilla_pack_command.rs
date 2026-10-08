//! `assetc vanilla-pack`: the development path to the bounded unpack first-run setup uses.

use std::{error::Error, fs, io::Write, path::Path};

use assets::{
    VanillaSource,
    vanilla_pack::{self, UnpackLimits, sha256_file},
};

/// `workspace` is the directory the manifest's `.local/assets` paths resolve against.
pub(super) fn acquire(
    manifest: &Path,
    workspace: &Path,
    accept_eula: bool,
) -> Result<(), Box<dyn Error>> {
    if !accept_eula {
        return Err(
            "refusing to fetch Mojang assets without the explicit --accept-eula flag".into(),
        );
    }
    let source = VanillaSource::read(manifest)?;
    let paths = source.local_paths(workspace)?;
    if paths.is_unpacked() {
        paths.reclaim_stale_staging();
        println!(
            "Vanilla source is already available: {}",
            paths.cache.display()
        );
        return Ok(());
    }
    let expected = source.sha256.to_ascii_lowercase();
    if paths.archive.is_file() && sha256_file(&paths.archive)? == expected {
        println!("Using verified archive: {}", paths.archive.display());
    } else {
        if !source.url.starts_with("https://") {
            return Err(format!("sample pack URL is not HTTPS: {}", source.url).into());
        }
        if let Some(parent) = paths.archive.parent() {
            fs::create_dir_all(parent)?;
        }
        println!("Downloading {}", source.url);
        download(&source.url, &paths.partial)?;
        let actual = sha256_file(&paths.partial)?;
        if actual != expected {
            let _ = fs::remove_file(&paths.partial);
            return Err(format!("SHA-256 mismatch: expected {expected}, got {actual}").into());
        }
        fs::rename(&paths.partial, &paths.archive)?;
    }
    vanilla_pack::unpack(&paths, &UnpackLimits::PINNED, &|| false)?;
    println!("Vanilla source ready: {}", paths.cache.display());
    Ok(())
}

fn download(url: &str, partial: &Path) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut response = reqwest::get(url).await?.error_for_status()?;
        let mut file = fs::File::create(partial)?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk)?;
        }
        file.sync_all()?;
        Ok::<(), Box<dyn Error>>(())
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    /// Backdates a directory's modification time; Windows opens directories only with backup
    /// semantics.
    fn backdate(dir: &Path, age: Duration) {
        let mut options = fs::File::options();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
        }
        options
            .open(dir)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
    }

    #[cfg(windows)]
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    #[test]
    fn a_cached_pack_still_reclaims_abandoned_staging() {
        let workspace = tempfile::tempdir().unwrap();
        let manifest = workspace.path().join("vanilla-source.json");
        fs::write(&manifest, assets::VANILLA_SOURCE_MANIFEST).unwrap();
        let paths = VanillaSource::read(&manifest)
            .unwrap()
            .local_paths(workspace.path())
            .unwrap();
        fs::create_dir_all(paths.cache.join("resource_pack")).unwrap();
        fs::write(paths.cache.join("resource_pack/blocks.json"), b"{}").unwrap();
        let name = paths.cache.file_name().unwrap().to_str().unwrap();
        let stale = paths.cache.with_file_name(format!("{name}.extracting-1-1"));
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join("leftover.bin"), b"x").unwrap();
        backdate(&stale, Duration::from_secs(3 * 24 * 60 * 60));

        acquire(&manifest, workspace.path(), true).unwrap();
        assert!(!stale.exists());
        assert!(paths.is_unpacked());
    }
}
