use super::*;
use crate::LayeredPackView;
use std::{
    io::{Cursor, Write},
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{ZipWriter, write::SimpleFileOptions};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn legacy_manifest_subpack_memory_is_converted_before_automatic_selection() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    library.set_device_memory(24 << 30);
    let manifest = manifest(
        90,
        "resources",
        r#", "subpacks": [
            {"folder_name":"lite", "name":"Lite", "memory_tier":0},
            {"folder_name":"full", "name":"Full", "memory_tier":12}
        ]"#,
    );
    let archive = zip(&[
        ("manifest.json", manifest.as_bytes()),
        ("font/glyph_e1.png", b"root glyph"),
        ("subpacks/lite/font/glyph_e1.png", b"lite glyph"),
        ("subpacks/full/font/glyph_e1.png", b"full glyph"),
    ]);
    library
        .import(&fixture.write("tiers.mcpack", &archive))
        .unwrap();
    library.activate(Uuid::from_u128(90)).unwrap();
    assert_eq!(library.active()[0].subpack, "full");
    let view = LayeredPackView::new(library.apply().unwrap());
    assert_eq!(
        view.read("font/glyph_e1.png").unwrap().as_ref(),
        b"full glyph"
    );
}

struct Fixture(PathBuf);
impl Fixture {
    /// Gives each test isolated install storage.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-pack-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    /// Writes an import fixture outside the library storage directory.
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
    /// Opens the test library with a deliberately unrelated fixture engine version.
    fn library(&self) -> GlobalPackLibrary {
        GlobalPackLibrary::open(self.0.join("installed"), [9, 0, 0]).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Builds original fixture data without any shipped game content.
fn manifest(id: u128, kind: &str, extra: &str) -> String {
    let id = Uuid::from_u128(id);
    format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","name":"Fixture", "version":[1,0,0]}},"modules":[{{"type":"{kind}"}}]{extra}}}"#
    )
}

/// Compresses small synthetic files into a ZIP fixture.
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn imports_mcpack_and_zip_and_skips_unsafe_paths() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    for (id, extension) in [(1, "mcpack"), (2, "zip")] {
        let manifest = manifest(id, "resources", "");
        let bytes = zip(&[
            ("folder/manifest.json", manifest.as_bytes()),
            ("folder/textures/test.txt", b"texture"),
            ("../escape", b"unsafe"),
        ]);
        let report = library
            .import(&fixture.write(&format!("pack.{extension}"), &bytes))
            .unwrap();
        assert_eq!(report.imported.len(), 1);
        assert!(report.rejected.is_empty());
        library.activate(Uuid::from_u128(id)).unwrap();
    }
    let view = LayeredPackView::new(library.apply().unwrap());
    assert_eq!(view.read("textures/test.txt").unwrap().as_ref(), b"texture");
    assert!(!fixture.0.join("escape").exists());
    assert_eq!(fixture.library().available().len(), 2);
}

#[test]
fn imports_both_addon_encodings_and_reports_bad_resource_half() {
    for nested in [false, true] {
        let fixture = Fixture::new();
        let mut library = fixture.library();
        let resource = manifest(1, "resources", "");
        let behavior = manifest(2, "data", "");
        let bytes = if nested {
            zip(&[
                (
                    "resource.mcpack",
                    &zip(&[("manifest.json", resource.as_bytes())]),
                ),
                (
                    "behavior.mcpack",
                    &zip(&[("manifest.json", behavior.as_bytes())]),
                ),
                ("broken.mcpack", b"broken"),
            ])
        } else {
            zip(&[
                ("resource/manifest.json", resource.as_bytes()),
                ("behavior/manifest.json", behavior.as_bytes()),
                ("broken/manifest.json", b"broken"),
            ])
        };
        let report = library
            .import(&fixture.write("bundle.mcaddon", &bytes))
            .unwrap();
        assert_eq!(report.imported.len(), 1);
        assert_eq!(report.skipped_behavior, 1);
        assert_eq!(report.rejected.len(), 1);
    }
}

#[test]
fn rejects_encrypted_malformed_and_unsupported_imports() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(1, "resources", "");
    for extension in ["mcpack", "mcaddon", "zip"] {
        assert!(
            library
                .import(&fixture.write(&format!("bad.{extension}"), b"not ZIP"))
                .is_err()
        );
        for contents in [
            b"encrypted index".as_slice(),
            br#"{"content":[{"path":"texture","key":"secret"}]}"#,
        ] {
            let bytes = zip(&[
                ("manifest.json", manifest.as_bytes()),
                ("contents.json", contents),
            ]);
            let report = library
                .import(&fixture.write(&format!("locked.{extension}"), &bytes))
                .unwrap();
            assert!(report.imported.is_empty());
            assert!(report.rejected[0].contains("encrypted"));
        }
    }
    assert!(matches!(
        library.import(&fixture.write("pack.txt", b"")),
        Err(LibraryError::UnsupportedExtension)
    ));
    assert!(library.available().is_empty());
}

#[test]
fn staged_priority_persists_only_after_apply_and_removal_reveals_lower_layer() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    for (id, contents) in [(1, "lower"), (2, "higher")] {
        let manifest = manifest(id, "resources", "");
        let bytes = zip(&[
            ("manifest.json", manifest.as_bytes()),
            ("textures/test.txt", contents.as_bytes()),
            ("ui/test.json", contents.as_bytes()),
        ]);
        library
            .import(&fixture.write(&format!("{id}.zip"), &bytes))
            .unwrap();
        library.activate(Uuid::from_u128(id)).unwrap();
    }
    assert!(fixture.library().active().is_empty());
    let first = LayeredPackView::new(library.apply().unwrap());
    assert_eq!(first.read("textures/test.txt").unwrap().as_ref(), b"higher");
    assert_eq!(fixture.library().active(), library.active());
    library.move_pack(Uuid::from_u128(1), 0).unwrap();
    let reordered = LayeredPackView::new(library.apply().unwrap());
    assert_eq!(reordered.read("ui/test.json").unwrap().as_ref(), b"lower");
    library.deactivate(Uuid::from_u128(1));
    let removed = LayeredPackView::new(library.apply().unwrap());
    assert_eq!(
        removed.read("textures/test.txt").unwrap().as_ref(),
        b"higher"
    );
    assert_eq!(first.read("textures/test.txt").unwrap().as_ref(), b"higher");
}

#[test]
fn selects_subpack_and_rejects_newer_engine_on_activation() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(
        1,
        "resources",
        r#", "subpacks":[{"folder_name":"high","name":"High","memory_performance_tier":4}]"#,
    );
    let bytes = zip(&[
        ("manifest.json", manifest.as_bytes()),
        ("textures/test.txt", b"root"),
        ("subpacks/high/textures/test.txt", b"high"),
    ]);
    library.import(&fixture.write("pack.zip", &bytes)).unwrap();
    assert_eq!(library.available()[0].subpacks[0].memory_tier, 4);
    library.activate(Uuid::from_u128(1)).unwrap();
    library.select_subpack(Uuid::from_u128(1), "high").unwrap();
    assert_eq!(
        LayeredPackView::new(library.apply().unwrap())
            .read("textures/test.txt")
            .unwrap()
            .as_ref(),
        b"high"
    );
    assert_eq!(fixture.library().active()[0].subpack, "high");
    let future = manifest.replace(
        "\"version\":[1,0,0]",
        "\"version\":[1,0,0],\"min_engine_version\":[99,0,0]",
    );
    library.deactivate(Uuid::from_u128(1));
    library.apply().unwrap();
    library
        .import(&fixture.write("future.zip", &zip(&[("manifest.json", future.as_bytes())])))
        .unwrap();
    assert!(matches!(
        library.activate(Uuid::from_u128(1)),
        Err(LibraryError::NewerEngine)
    ));
}

#[test]
fn failed_apply_leaves_persisted_selection_and_composition_gives_higher_layer_precedence() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(1, "resources", "");
    library
        .import(&fixture.write("pack.zip", &zip(&[("manifest.json", manifest.as_bytes())])))
        .unwrap();
    library.activate(Uuid::from_u128(1)).unwrap();
    let initial = library.apply().unwrap();
    library
        .select_subpack(Uuid::from_u128(1), "missing")
        .unwrap_err();
    fs::remove_file(library.root.join(library.available()[0].filename())).unwrap();
    assert!(library.apply().is_err());
    assert_eq!(fixture.library().active(), library.active());
    let empty = ValidatedPackStack {
        packs: Box::default(),
        rejections: Box::default(),
    };
    let composed = ValidatedPackStack::compose(&empty, &initial).unwrap();
    assert_eq!(composed.packs().len(), 1);
}

#[test]
fn localizes_known_manifest_keys_and_retains_literal_descriptions() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(1, "resources", "").replace("Fixture", "pack.name");
    let report = library
        .import(&fixture.write(
            "localized.mcpack",
            &zip(&[
                ("manifest.json", manifest.as_bytes()),
                (
                    "texts/en_US.lang",
                    b"pack.name=Readable pack name\npack.description=Description",
                ),
            ]),
        ))
        .unwrap();
    assert_eq!(report.imported[0].name, "Readable pack name");
    assert_eq!(report.imported[0].description, "");
}

#[test]
fn preview_does_not_persist_and_acknowledgement_preserves_newer_edits() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(1, "resources", "");
    library
        .import(&fixture.write("pack.zip", &zip(&[("manifest.json", manifest.as_bytes())])))
        .unwrap();
    library.activate(Uuid::from_u128(1)).unwrap();
    let selection = library.active().to_vec();
    assert_eq!(library.preview().unwrap().packs().len(), 1);
    assert!(fixture.library().active().is_empty());
    library.deactivate(Uuid::from_u128(1));
    library.commit_selection(&selection).unwrap();
    assert!(
        library.active().is_empty(),
        "newer staged edit is preserved"
    );
    assert_eq!(fixture.library().active(), selection);
}

#[test]
fn active_replacement_preserves_old_snapshots_and_updates_on_apply() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let id = Uuid::from_u128(1);
    let original = manifest(1, "resources", "");
    let source = fixture.write(
        "original.zip",
        &zip(&[
            ("manifest.json", original.as_bytes()),
            ("textures/test.txt", b"original"),
        ]),
    );
    library.import(&source).unwrap();
    library.activate(id).unwrap();
    let old = LayeredPackView::new(library.apply().unwrap());
    library.preview().unwrap();
    for manifest in [original.clone(), original.replace("[1,0,0]", "[2,0,0]")] {
        let pending = library.active().to_vec();
        let replacement = fixture.write(
            "replacement.zip",
            &zip(&[
                ("manifest.json", manifest.as_bytes()),
                ("textures/test.txt", b"changed"),
            ]),
        );
        assert_eq!(library.import(&replacement).unwrap().imported.len(), 1);
        let restarted = LayeredPackView::new(fixture.library().preview().unwrap());
        assert_eq!(fixture.library().active(), pending);
        assert!(restarted.read("textures/test.txt").is_some());
        library.commit_selection(&pending).unwrap();
        let new = LayeredPackView::new(library.apply().unwrap());
        assert_eq!(new.read("textures/test.txt").unwrap().as_ref(), b"changed");
        assert_eq!(old.read("textures/test.txt").unwrap().as_ref(), b"original");
        assert_eq!(fixture.library().active(), library.active());
    }
}

#[test]
fn pack_icons_are_read_from_their_own_archive_without_activation() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    for id in [1, 2] {
        let manifest = manifest(id, "resources", "");
        let icon = [id as u8];
        let source = fixture.write(
            "pack.zip",
            &zip(&[
                ("manifest.json", manifest.as_bytes()),
                ("pack_icon.png", &icon),
            ]),
        );
        library.import(&source).unwrap();
    }
    assert!(library.active().is_empty());
    assert_eq!(
        library
            .pack_icon(&library.available()[0])
            .unwrap()
            .unwrap()
            .as_ref(),
        &[1]
    );
    assert_eq!(
        library
            .pack_icon(&library.available()[1])
            .unwrap()
            .unwrap()
            .as_ref(),
        &[2]
    );
}

#[test]
fn catalog_larger_than_one_manifest_reopens_after_multiple_valid_imports() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    for id in [1, 2] {
        let mut manifest: serde_json::Value =
            serde_json::from_str(&manifest(id, "resources", "")).unwrap();
        manifest["header"]["description"] =
            serde_json::Value::String("x".repeat(crate::MAX_MANIFEST_BYTES / 2));
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let report = library
            .import(&fixture.write("pack.zip", &zip(&[("manifest.json", &bytes)])))
            .unwrap();
        assert_eq!(report.imported.len(), 1);
    }
    assert!(
        fs::metadata(library.root.join(CATALOG_FILE)).unwrap().len()
            > crate::MAX_MANIFEST_BYTES as u64
    );
    assert_eq!(fixture.library().available().len(), 2);
}

#[test]
fn catalog_limit_rejection_does_not_publish_metadata_or_archive() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let manifest = manifest(1, "resources", "");
    let source = fixture.write("pack.zip", &zip(&[("manifest.json", manifest.as_bytes())]));
    library.catalog.available.push(InstalledPack {
        id: Uuid::from_u128(2),
        version: [1, 0, 0],
        name: "existing".into(),
        revision: 0,
        description: "x".repeat(MAX_CATALOG_BYTES),
        min_engine_version: None,
        subpacks: Vec::new(),
    });
    assert!(matches!(
        library.import(&source),
        Err(LibraryError::CatalogTooLarge)
    ));
    assert_eq!(library.available().len(), 1);
    assert_eq!(library.available()[0].id, Uuid::from_u128(2));
    assert!(!library.root.exists());
}

#[test]
fn review_subpack_selection_uses_the_active_revision() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let id = Uuid::from_u128(1);
    for folder in ["old", "new"] {
        let text = manifest(
            1,
            "resources",
            &format!(
                r#", "subpacks":[{{"folder_name":"{folder}","name":"Fixture","memory_tier":1}}]"#
            ),
        );
        library
            .import(&fixture.write("revision.zip", &zip(&[("manifest.json", text.as_bytes())])))
            .unwrap();
        if folder == "old" {
            library.activate(id).unwrap();
            library.apply().unwrap();
        }
    }
    let mut reopened = fixture.library();
    reopened.select_subpack(id, "old").unwrap();
    assert!(reopened.select_subpack(id, "new").is_err());
    reopened.preview().unwrap();
}

#[test]
fn review_failed_archive_deletion_remains_retryable() {
    let fixture = Fixture::new();
    let mut library = fixture.library();
    let text = manifest(1, "resources", "");
    library
        .import(&fixture.write("first.zip", &zip(&[("manifest.json", text.as_bytes())])))
        .unwrap();
    library
        .import(&fixture.write("second.zip", &zip(&[("manifest.json", text.as_bytes())])))
        .unwrap();
    let obsolete = library.root.join(library.catalog.retained[0].filename());
    let bytes = fs::read(&obsolete).unwrap();
    fs::remove_file(&obsolete).unwrap();
    fs::create_dir(&obsolete).unwrap();
    assert!(storage::prune(&library.root, &mut library.catalog).is_err());
    assert_eq!(library.catalog.retained.len(), 1);
    fs::remove_dir(&obsolete).unwrap();
    fs::write(&obsolete, bytes).unwrap();
    storage::prune(&library.root, &mut library.catalog).unwrap();
    assert!(!obsolete.exists());
    assert!(library.catalog.retained.is_empty());
}

#[test]
fn review_corrupt_nested_archive_keeps_valid_siblings() {
    let fixture = Fixture::new();
    let text = manifest(1, "resources", "");
    let good = zip(&[("manifest.json", text.as_bytes())]);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("bad.mcpack", &b"corrupt-me"[..]),
        ("good.mcpack", good.as_slice()),
    ] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let mut bundle = writer.finish().unwrap().into_inner();
    let payload = bundle
        .windows(10)
        .position(|bytes| bytes == b"corrupt-me")
        .unwrap();
    bundle[payload] ^= 1;
    let report = fixture
        .library()
        .import(&fixture.write("bundle.mcaddon", &bundle))
        .unwrap();
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.rejected.len(), 1);
}
