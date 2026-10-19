//! Loading contracts exercised through the public API with small original inputs.

use shaderpack::{PackError, PackOrigin, PackSource};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// An isolated temporary pack directory removed only by its owning test.
struct TempPack(PathBuf);

impl TempPack {
    /// Creates a fresh pack without deleting paths left by another process.
    fn new() -> Self {
        Self::in_dir(&std::env::temp_dir())
    }

    /// Creates an isolated pack inside the supplied directory without changing the working directory.
    fn in_dir(parent: &Path) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        loop {
            let path = parent.join(format!(
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
    for path in ["../outside", "/outside", "lib/../../outside", "./z.fsh"] {
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
                archive: archive.canonicalize().unwrap(),
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

#[cfg(unix)]
#[test]
fn source_paths_directory_rejects_non_unicode_names() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let root = TempPack::new();
    let shaders = root.0.join("shaders");
    std::fs::create_dir(&shaders).unwrap();
    for name in [b"a\xff.glsl".as_slice(), b"a\xfe.glsl"] {
        let unsupported = shaders.join(OsString::from_vec(name.to_vec()));
        std::fs::write(&unsupported, "invalid name").unwrap();
        match PackSource::open(&root.0).expect_err("non-Unicode component rejected") {
            PackError::Io { path, error } => {
                assert_eq!(path, unsupported.canonicalize().unwrap());
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert!(error.to_string().contains("UTF-8"));
            }
            error => panic!("unexpected path error: {error}"),
        }
        std::fs::remove_file(unsupported).unwrap();
    }
}

#[test]
fn source_paths_preserve_unicode_and_nested_components() {
    let root = TempPack::new();
    let shaders = root.0.join("shaders");
    std::fs::create_dir_all(shaders.join("層")).unwrap();
    let path = "層/é.glsl";
    std::fs::write(shaders.join(path), "unicode").unwrap();
    let source = PackSource::open(&root.0).unwrap();
    assert_eq!(source.files().collect::<Vec<_>>(), [(path, "unicode")]);
    assert_eq!(source.read_bytes(path).unwrap(), b"unicode");
}

#[test]
fn source_paths_relative_zip_origin_is_canonical_for_deferred_reads() {
    let cwd = std::env::current_dir().unwrap();
    let root = TempPack::in_dir(&cwd);
    let archive = root.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    writer
        .start_file(
            "shaders/asset.bin",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"retained archive").unwrap();
    writer.finish().unwrap();
    let relative = archive.strip_prefix(&cwd).unwrap();
    assert!(relative.is_relative());
    let source = PackSource::open(relative).unwrap();
    assert_eq!(
        source.origin(),
        &PackOrigin::Zip {
            archive: archive.canonicalize().unwrap(),
            prefix: "shaders/".to_owned(),
        }
    );
    assert_eq!(source.read_bytes("asset.bin").unwrap(), b"retained archive");
}

#[test]
fn source_paths_zip_name_collisions_keep_text_and_bytes_consistent() {
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for suffix in ["X", "Y"] {
        writer
            .start_file(
                format!("shaders/é{suffix}.glsl"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(suffix.as_bytes()).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    for (suffix, replacement) in [("X", 0xff), ("Y", 0xfe)] {
        let name = format!("shaders/é{suffix}.glsl");
        let starts: Vec<_> = bytes
            .windows(name.len())
            .enumerate()
            .filter_map(|(offset, value)| (value == name.as_bytes()).then_some(offset))
            .collect();
        assert_eq!(starts.len(), 2);
        for start in starts {
            bytes[start + name.len() - ".glsl".len() - 1] = replacement;
        }
    }
    std::fs::write(&archive, bytes).unwrap();
    let source = PackSource::open(&archive).unwrap();
    let files: Vec<_> = source.files().collect();
    assert_eq!(files.len(), 1);
    let (path, text) = files[0];
    assert_eq!(source.read_bytes(path).unwrap(), text.as_bytes());
}

#[cfg(unix)]
#[test]
fn source_paths_memory_and_zip_keep_exact_distinct_names() {
    let files = [
        ("a\\b.glsl", "literal"),
        ("a/b.glsl", "nested"),
        ("層/é.glsl", "unicode"),
    ];
    let memory = PackSource::from_files(files);
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for (path, text) in files {
        writer
            .start_file(
                format!("shaders/{path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    let zipped = PackSource::open(&archive).unwrap();
    assert_eq!(
        memory.files().collect::<Vec<_>>(),
        zipped.files().collect::<Vec<_>>()
    );
    for source in [&memory, &zipped] {
        for (path, text) in files {
            assert_eq!(source.text(path), Some(text));
            assert_eq!(source.read_bytes(path).unwrap(), text.as_bytes());
        }
    }
}

#[test]
fn source_paths_zip_keeps_legacy_cp437_names() {
    let root = TempPack::new();
    let archive = root.0.join("pack.zip");
    let name = b"shaders/X.glsl";
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    writer
        .start_file(
            std::str::from_utf8(name).unwrap(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"legacy name").unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    let starts: Vec<_> = bytes
        .windows(name.len())
        .enumerate()
        .filter_map(|(offset, value)| (value == name).then_some(offset))
        .collect();
    assert_eq!(starts.len(), 2);
    for start in starts {
        bytes[start + b"shaders/".len()] = 0x82;
    }
    std::fs::write(&archive, bytes).unwrap();
    let source = PackSource::open(&archive).unwrap();
    assert_eq!(source.text("é.glsl"), Some("legacy name"));
    assert_eq!(source.read_bytes("é.glsl").unwrap(), b"legacy name");
}

#[test]
fn directory_layout_opens_current_shaders_root() {
    const CHILD_FLAG: &str = "SHADERPACK_TEST_CURRENT_SHADERS_ROOT";
    const CHILD_SUCCESS: i32 = 73;
    if std::env::var_os(CHILD_FLAG).is_some() {
        let canonical = std::env::current_dir().unwrap().canonicalize().unwrap();
        for path in [".", "./nested/..", "../shaders", ".."] {
            let source = PackSource::open(Path::new(path)).unwrap();
            assert_eq!(source.origin(), &PackOrigin::Directory(canonical.clone()));
            assert_eq!(
                source.files().collect::<Vec<_>>(),
                [("witness.glsl", "root")]
            );
            assert_eq!(source.read_bytes("asset.bin").unwrap(), b"raw root");
        }
        std::process::exit(CHILD_SUCCESS);
    }

    let root = TempPack::new();
    let shaders = root.0.join("shaders");
    std::fs::create_dir_all(shaders.join("nested")).unwrap();
    std::fs::write(shaders.join("witness.glsl"), "root").unwrap();
    std::fs::write(shaders.join("asset.bin"), b"raw root").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "directory_layout_opens_current_shaders_root"])
        .env(CHILD_FLAG, "1")
        .current_dir(&shaders)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(CHILD_SUCCESS),
        "isolated shaders-root test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn directory_layout_preserves_explicit_shaders_symlinks() {
    let root = TempPack::new();
    let target = root.0.join("payload");
    let wrapper = root.0.join("wrapper");
    let alias = root.0.join("wrapper-alias");
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&wrapper).unwrap();
    std::fs::write(target.join("witness.glsl"), "linked root").unwrap();
    std::fs::write(target.join("asset.bin"), b"linked asset").unwrap();
    std::os::unix::fs::symlink(&target, wrapper.join("shaders")).unwrap();
    std::os::unix::fs::symlink(&wrapper, &alias).unwrap();
    for path in [
        wrapper.clone(),
        wrapper.join("shaders"),
        wrapper.join("./shaders"),
        alias.clone(),
        alias.join("shaders"),
    ] {
        let source = PackSource::open(&path).unwrap();
        assert_eq!(
            source.origin(),
            &PackOrigin::Directory(target.canonicalize().unwrap())
        );
        assert_eq!(source.text("witness.glsl"), Some("linked root"));
        assert_eq!(source.read_bytes("asset.bin").unwrap(), b"linked asset");
    }
    assert!(matches!(
        PackSource::open(&target),
        Err(PackError::NotAShaderPack(path)) if path == target
    ));
}
