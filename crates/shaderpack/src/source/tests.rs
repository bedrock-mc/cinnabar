use super::*;
use std::io::Write;

/// Invalid Unix path bytes fail admission even on filesystems that cannot store such names.
#[cfg(unix)]
#[test]
fn relative_keys_reject_non_unicode_components() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    for name in [b"a\xff.glsl".as_slice(), b"nested/a\xfe.glsl"] {
        let path = PathBuf::from(OsString::from_vec(name.to_vec()));
        let error = relative_key(&path).expect_err("non-Unicode component rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("UTF-8"));
    }
}

/// A small temporary pack whose cleanup only touches this test's directory.
struct TempPack(PathBuf);

impl TempPack {
    /// Creates an isolated pack directory with an empty `shaders/` folder.
    fn new() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        loop {
            let root = std::env::temp_dir().join(format!(
                "shaderpack-load-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&root) {
                Ok(()) => {
                    let pack = Self(root);
                    std::fs::create_dir(pack.0.join("shaders")).unwrap();
                    return pack;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test pack: {error}"),
            }
        }
    }
}

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Limits small enough to prove rejection without large inputs.
fn small_limits() -> LoadLimits {
    LoadLimits {
        file_bytes: 128,
        total_bytes: 192,
        entries: 8,
        archive_bytes: 8192,
    }
}

/// Writes a deflated archive with the given entries.
fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn ingestion_rejects_oversized_files_including_unused_ones() {
    let root = TempPack::new();
    std::fs::write(root.0.join("shaders/unused.unknown"), [b'A'; 129]).unwrap();
    assert!(matches!(
        PackSource::open_dir(&root.0, small_limits()),
        Err(PackError::Limit { .. })
    ));
    let archive = root.0.join("pack.zip");
    write_zip(&archive, &[("shaders/unused.glsl", &[b'A'; 129])]);
    assert!(matches!(
        PackSource::open_zip(&archive, small_limits()),
        Err(PackError::Limit { .. })
    ));
}

#[test]
fn ingestion_bounds_total_text_entries_and_archive_size() {
    let root = TempPack::new();
    for name in ["a.glsl", "b.glsl"] {
        std::fs::write(root.0.join("shaders").join(name), [b'A'; 100]).unwrap();
    }
    assert!(PackSource::open_dir(&root.0, small_limits()).is_err());
    std::fs::remove_file(root.0.join("shaders/b.glsl")).unwrap();
    assert!(PackSource::open_dir(&root.0, small_limits()).is_ok());
    for i in 0..8 {
        std::fs::write(root.0.join(format!("shaders/{i}.png")), []).unwrap();
    }
    assert!(PackSource::open_dir(&root.0, small_limits()).is_err());

    let archive = root.0.join("pack.zip");
    let names: Vec<String> = (0..9).map(|i| format!("shaders/{i}.png")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &[][..])).collect();
    write_zip(&archive, &entries);
    assert!(PackSource::open_zip(&archive, small_limits()).is_err());
    let mut limits = small_limits();
    limits.entries = 16;
    assert!(PackSource::open_zip(&archive, limits).is_ok());
    limits.archive_bytes = 64;
    assert!(PackSource::open_zip(&archive, limits).is_err());
}

#[test]
fn bounded_reader_accepts_exact_limit_and_rejects_one_more_byte() {
    assert!(read_bounded(io::empty(), 0).unwrap().is_empty());
    assert_eq!(read_bounded(&[1, 2, 3][..], 3).unwrap(), [1, 2, 3]);
    let error = read_bounded(&[1, 2, 3, 4][..], 3).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
}

#[cfg(unix)]
#[test]
fn directory_assets_reject_external_symlinks() {
    let root = TempPack::new();
    let shaders = root.0.join("shaders");
    let outside = root.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("asset.bin"), b"external witness").unwrap();
    std::fs::write(shaders.join("asset.bin"), b"inside").unwrap();
    std::os::unix::fs::symlink(&outside, shaders.join("tex")).unwrap();
    std::os::unix::fs::symlink(outside.join("asset.bin"), shaders.join("link.bin")).unwrap();
    std::os::unix::fs::symlink(shaders.join("asset.bin"), shaders.join("local.bin")).unwrap();
    let source = PackSource::open(&root.0).unwrap();
    assert_eq!(source.read_bytes("asset.bin").unwrap(), b"inside");
    assert_eq!(source.read_bytes("local.bin").unwrap(), b"inside");
    for path in ["tex/asset.bin", "link.bin"] {
        let error = source
            .read_bytes(path)
            .expect_err("external asset rejected");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}

#[cfg(unix)]
#[test]
fn ingestion_skips_directory_symlinks() {
    let root = TempPack::new();
    let levels = root.0.join("levels");
    std::fs::create_dir_all(levels.join("0")).unwrap();
    std::fs::write(levels.join("0/leaf.glsl"), "leaf").unwrap();
    for name in ["left", "right"] {
        std::os::unix::fs::symlink(levels.join("0"), root.0.join("shaders").join(name)).unwrap();
    }
    let source = PackSource::open_dir(&root.0, small_limits()).unwrap();
    assert_eq!(source.files().count(), 0);
}

#[test]
fn zip_prefix_accepts_root_or_one_top_folder() {
    assert_eq!(
        shaders_prefix(["shaders/a.fsh", "x.txt"].into_iter()).as_deref(),
        Some("shaders/")
    );
    assert_eq!(
        shaders_prefix(["Pack v1/shaders/world0/a.fsh"].into_iter()).as_deref(),
        Some("Pack v1/shaders/")
    );
    assert_eq!(shaders_prefix(["a/b/shaders/x.fsh"].into_iter()), None);
}
