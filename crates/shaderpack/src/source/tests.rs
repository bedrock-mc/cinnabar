use super::*;

/// A small temporary pack whose cleanup only touches this test's directory.
struct TempPack(PathBuf);

impl TempPack {
    /// Creates an isolated shader directory.
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
    /// Deletes this test's files.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Limits small enough to prove rejected growth without stress allocation.
fn small_limits() -> LoadLimits {
    LoadLimits {
        file_bytes: 128,
        total_bytes: 192,
        entries: 8,
        path_bytes: 256,
        archive_bytes: 8192,
    }
}

#[test]
fn ingestion_rejects_unused_text_before_oversized_reads() {
    let root = TempPack::new();
    std::fs::write(root.0.join("shaders/unused.unknown"), vec![b'A'; 129]).unwrap();
    assert!(PackSource::open_dir_limited(&root.0, small_limits()).is_err());
}

#[test]
fn ingestion_rejects_zip_inflation_before_allocation() {
    use std::io::Write;
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    writer
        .start_file(
            "shaders/unused.glsl",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&[b'A'; 129]).unwrap();
    writer.finish().unwrap();
    assert!(PackSource::open_zip_limited(&archive, small_limits()).is_err());
}

#[test]
fn ingestion_bounded_reader_stops_at_one_byte_past_limit() {
    let mut reader = io::Cursor::new([b'A'; 256]);
    assert!(read_bounded(&mut reader, 128).is_err());
    assert_eq!(reader.position(), 129);
}

/// Splits reads into small pieces without changing the underlying bytes.
struct PartialReader<R> {
    reader: R,
    chunk: usize,
}

impl<R: Read> Read for PartialReader<R> {
    /// Returns at most one configured chunk, including at the final partial read.
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let length = self.chunk.min(out.len());
        self.reader.read(&mut out[..length])
    }
}

#[test]
fn ingestion_reader_growth_is_amortized_and_capped() {
    let bytes = [b'X'; 90];
    let reader = PartialReader {
        reader: io::Cursor::new(bytes),
        chunk: 8,
    };
    let result = read_bounded(reader, 100).unwrap();
    assert_eq!(result, bytes);
    assert!(
        result.capacity() > result.len(),
        "partial reads retain geometric headroom"
    );
    assert!(result.capacity() <= 100);
}

#[test]
fn ingestion_reader_preserves_empty_exact_and_partial_boundaries() {
    assert!(read_bounded(io::empty(), 0).unwrap().is_empty());
    assert!(read_bounded(io::Cursor::new([1]), 0).is_err());
    assert_eq!(
        read_bounded(io::Cursor::new([1, 2, 3]), 3).unwrap(),
        [1, 2, 3]
    );
    let mut reader = PartialReader {
        reader: io::Cursor::new([7; 16]),
        chunk: 3,
    };
    assert!(read_bounded(&mut reader, 8).is_err());
    assert_eq!(reader.reader.position(), 9);
}

#[test]
fn ingestion_zip_index_limits_cover_paths_entries_and_fallback() {
    use std::io::Write;
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("shaders/ok.glsl", options).unwrap();
    writer.write_all(b"ok").unwrap();
    let mut fake = [0u8; 22];
    fake[..4].copy_from_slice(b"PK\x05\x06");
    fake[8..10].copy_from_slice(&9u16.to_le_bytes());
    fake[10..12].copy_from_slice(&9u16.to_le_bytes());
    let mut comment = fake.to_vec();
    comment.extend_from_slice(&[0; 32]);
    writer.set_raw_comment(comment.into_boxed_slice());
    writer.finish().unwrap();
    assert!(PackSource::open_zip_limited(&archive, small_limits()).is_err());
    let mut ordinary = std::fs::read(&archive).unwrap();
    let footer = ordinary.len() - fake.len() - 32 - 22;
    ordinary.truncate(footer + 22);
    ordinary[footer + 20..footer + 22].fill(0);
    std::fs::write(&archive, ordinary).unwrap();
    let pack = PackSource::open_zip_limited(&archive, small_limits()).unwrap();
    assert_eq!(pack.text("ok.glsl"), Some("ok"));
    let mut limits = small_limits();
    limits.entries = 0;
    assert!(PackSource::open_zip_limited(&archive, limits).is_err());
    limits = small_limits();
    limits.path_bytes = 1;
    assert!(PackSource::open_zip_limited(&archive, limits).is_err());
}

/// Writes a tiny valid ZIP whose text entries share an empty DEFLATE payload.
fn empty_alias_zip(aliases: u16, blocks: usize) -> Vec<u8> {
    let mut compressed = Vec::new();
    for _ in 0..blocks {
        compressed.extend_from_slice(&[0, 0, 0, 255, 255]);
    }
    compressed.extend_from_slice(&[1, 0, 0, 255, 255]);
    let local_name = b"shaders/0.glsl";
    let mut local = [0u8; 30];
    local[..4].copy_from_slice(b"PK\x03\x04");
    local[4..6].copy_from_slice(&20u16.to_le_bytes());
    local[8..10].copy_from_slice(&8u16.to_le_bytes());
    local[18..22].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    local[26..28].copy_from_slice(&(local_name.len() as u16).to_le_bytes());
    let mut out = local.to_vec();
    out.extend_from_slice(local_name);
    out.extend_from_slice(&compressed);
    let start = out.len();
    for index in 0..aliases {
        let name = format!("shaders/{index}.glsl");
        let mut central = [0u8; 46];
        central[..4].copy_from_slice(b"PK\x01\x02");
        central[4..6].copy_from_slice(&20u16.to_le_bytes());
        central[6..8].copy_from_slice(&20u16.to_le_bytes());
        central[10..12].copy_from_slice(&8u16.to_le_bytes());
        central[20..24].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
        central[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&central);
        out.extend_from_slice(name.as_bytes());
    }
    let mut end = [0u8; 22];
    end[..4].copy_from_slice(b"PK\x05\x06");
    end[8..10].copy_from_slice(&aliases.to_le_bytes());
    end[10..12].copy_from_slice(&aliases.to_le_bytes());
    end[12..16].copy_from_slice(&((out.len() - start) as u32).to_le_bytes());
    end[16..20].copy_from_slice(&(start as u32).to_le_bytes());
    out.extend_from_slice(&end);
    out
}

#[test]
fn ingestion_zip_aliases_share_actual_compressed_read_budget() {
    let root = TempPack::new();
    let archive = root.0.join("aliases.zip");
    let bytes = empty_alias_zip(4, 40);
    assert!(bytes.len() < 1800);
    std::fs::write(&archive, bytes).unwrap();
    let mut limits = small_limits();
    limits.archive_bytes = 1800;
    assert!(PackSource::open_zip_limited(&archive, limits).is_err());
    assert_eq!(
        PackSource::open_zip_limited(&archive, small_limits())
            .unwrap()
            .files()
            .count(),
        4
    );
}

#[test]
fn ingestion_counts_total_decoded_text_and_all_entries() {
    let root = TempPack::new();
    for name in ["a.glsl", "b.glsl"] {
        std::fs::write(root.0.join("shaders").join(name), vec![b'A'; 100]).unwrap();
    }
    assert!(PackSource::open_dir_limited(&root.0, small_limits()).is_err());
    std::fs::remove_file(root.0.join("shaders/b.glsl")).unwrap();
    std::fs::write(root.0.join("shaders/a.glsl"), [0xff; 65]).unwrap();
    assert!(PackSource::open_dir_limited(&root.0, small_limits()).is_err());
    for i in 0..8 {
        std::fs::write(root.0.join(format!("shaders/{i}.png")), []).unwrap();
    }
    assert!(PackSource::open_dir_limited(&root.0, small_limits()).is_err());
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
fn ingestion_skips_directory_symlink_graphs() {
    let root = TempPack::new();
    let levels = root.0.join("levels");
    std::fs::create_dir_all(levels.join("0")).unwrap();
    std::fs::write(levels.join("0/leaf.glsl"), "leaf").unwrap();
    for name in ["left", "right"] {
        std::os::unix::fs::symlink(levels.join("0"), root.0.join("shaders").join(name)).unwrap();
    }
    let source = PackSource::open_dir_limited(&root.0, small_limits()).unwrap();
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
