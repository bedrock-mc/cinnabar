//! Loading contracts exercised through the public API with small original inputs.

use shaderpack::{PackError, PackOrigin, PackSource};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// An isolated temporary pack directory removed only by its owning test.
struct TempPack(PathBuf);

impl TempPack {
    /// Creates a fresh pack without deleting paths left by another process.
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "shaderpack-api-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test pack: {error}"),
            }
        }
    }
}

impl Drop for TempPack {
    /// Removes this test's directory and all of its generated files.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Text and binary witnesses shared by the folder and archive loaders.
const FILES: &[(&str, &[u8])] = &[
    ("z.fsh", b"\xef\xbb\xbftext\xff"),
    ("lib/a.glsl", b"alpha"),
    ("texture.PNG", b"image bytes"),
    ("hidden.unknown", b"\0binary"),
];

/// Checks deterministic text admission and the separate raw asset view.
fn assert_loaded(source: &PackSource) {
    assert_eq!(
        source.files().collect::<Vec<_>>(),
        [("lib/a.glsl", "alpha"), ("z.fsh", "text\u{fffd}")]
    );
    assert_eq!(source.text("z.fsh"), Some("text\u{fffd}"));
    assert_eq!(source.text("texture.PNG"), None);
    assert_eq!(source.text("missing"), None);
    for &(path, bytes) in FILES {
        assert_eq!(source.read_bytes(path).unwrap(), bytes);
    }
    for path in ["../outside", "/outside", "lib/../../outside"] {
        assert_eq!(
            source.read_bytes(path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "{path}"
        );
    }
    assert!(source.read_bytes("missing").is_err());
}

#[test]
fn folders_accept_pack_or_shaders_root_and_preserve_raw_assets() {
    let root = TempPack::new();
    let shaders = root.0.join("shaders");
    std::fs::create_dir_all(shaders.join("lib")).unwrap();
    for &(path, bytes) in FILES {
        std::fs::write(shaders.join(path), bytes).unwrap();
    }
    std::fs::write(root.0.join("outside"), "outside text").unwrap();
    for path in [&root.0, &shaders] {
        let source = PackSource::open(path).unwrap();
        assert_eq!(
            source.origin(),
            &PackOrigin::Directory(shaders.canonicalize().unwrap())
        );
        assert_loaded(&source);
    }
}

#[test]
fn archives_accept_root_or_one_top_folder_and_preserve_raw_assets() {
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    for prefix in ["shaders/", "Example Pack/shaders/"] {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for &(path, bytes) in FILES {
            writer
                .start_file(format!("{prefix}{path}"), options)
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.start_file("outside", options).unwrap();
        writer.write_all(b"outside text").unwrap();
        writer
            .start_file(format!("{prefix}../escape.glsl"), options)
            .unwrap();
        writer.write_all(b"escaped text").unwrap();
        writer.finish().unwrap();
        let source = PackSource::open(&archive).unwrap();
        assert_eq!(
            source.origin(),
            &PackOrigin::Zip {
                archive: archive.clone(),
                prefix: prefix.to_owned(),
            }
        );
        assert_loaded(&source);
    }
}

#[test]
fn memory_sources_own_paths_and_text_and_replace_duplicate_paths() {
    let source = {
        let files = [
            (String::from("z.fsh"), String::from("z")),
            (String::from("a.vsh"), String::from("first")),
            (String::from("a.vsh"), String::from("last")),
        ];
        PackSource::from_files(files)
    };
    let cloned = source.clone();
    drop(source);
    assert_eq!(cloned.origin(), &PackOrigin::Memory);
    assert_eq!(
        cloned.files().collect::<Vec<_>>(),
        [("a.vsh", "last"), ("z.fsh", "z")]
    );
    assert_eq!(cloned.text("a.vsh"), Some("last"));
    assert_eq!(cloned.read_bytes("a.vsh").unwrap(), b"last");
    assert_eq!(
        cloned.read_bytes("missing").unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        cloned.read_bytes("../z.fsh").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn missing_shaders_and_truncated_archives_return_errors() {
    let root = TempPack::new();
    assert!(matches!(
        PackSource::open(&root.0),
        Err(PackError::NotAShaderPack(path)) if path == root.0
    ));
    let archive = root.0.join("pack.zip");
    for bytes in [b"".as_slice(), b"PK\x05\x06", &[0; 22]] {
        std::fs::write(&archive, bytes).unwrap();
        assert!(PackSource::open(&archive).is_err());
    }
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    writer
        .start_file(
            "a/b/shaders/a.fsh",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"text").unwrap();
    writer.finish().unwrap();
    assert!(matches!(
        PackSource::open(&archive),
        Err(PackError::NotAShaderPack(path)) if path == archive
    ));
}
