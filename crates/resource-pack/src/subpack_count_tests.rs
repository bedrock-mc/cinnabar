use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{AdmissionError, GlobalPackLibrary, LayeredPackView, manifest::read_manifest};

const PACK_ID: Uuid = Uuid::from_u128(1);
const VERSION: &str = "1.2.3";
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

/// Builds a complete format-2 manifest with distinct, equally supported subpacks.
fn manifest(id: Uuid, count: usize) -> Value {
    let version = crate::manifest::Version::parse(VERSION).unwrap().0;
    json!({
        "format_version": 2,
        "header": {
            "uuid": id,
            "name": "Subpack fixture",
            "description": "Original test content",
            "version": version,
        },
        "modules": [{"uuid": Uuid::from_u128(3), "type": "resources", "version": version}],
        "subpacks": (0..count).map(|index| json!({
            "folder_name": format!("p{index}"),
            "name": format!("Option {index}"),
            "memory_tier": 11,
        })).collect::<Vec<_>>(),
    })
}

/// Writes the supplied manifest and small original members with complete ZIP metadata.
fn archive(manifest: &Value, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(&serde_json::to_vec(manifest).unwrap())
        .unwrap();
    for (path, bytes) in files {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Adds root fallbacks and distinct content in the first and last declared folders.
fn tiered_archive(manifest: &Value) -> Vec<u8> {
    let subpacks = manifest["subpacks"].as_array().unwrap();
    let first = subpacks.first().unwrap()["folder_name"].as_str().unwrap();
    let last = subpacks.last().unwrap()["folder_name"].as_str().unwrap();
    archive(
        manifest,
        &[
            ("marker.txt", b"root"),
            ("root-only.txt", b"root fallback"),
            (&format!("subpacks/{first}/marker.txt"), b"first"),
            (&format!("subpacks/{last}/marker.txt"), b"last"),
            (&format!("subpacks/{last}/sentinel.txt"), b"last sentinel"),
        ],
    )
}

#[test]
fn manifest_preserves_all_declared_subpacks_and_checks_identity() {
    for count in [64, 65] {
        let bytes = serde_json::to_vec(&manifest(PACK_ID, count)).unwrap();
        let parsed = read_manifest(&bytes, PACK_ID, VERSION).unwrap();
        assert_eq!(parsed.subpacks.len(), count);
        for (index, subpack) in parsed.subpacks.iter().enumerate() {
            assert_eq!(subpack.folder, format!("p{index}"));
            assert_eq!(subpack.name, format!("Option {index}"));
            assert_eq!(subpack.memory_tier, 1);
        }
        assert_eq!(
            read_manifest(&bytes, Uuid::from_u128(2), VERSION).err(),
            Some(AdmissionError::ManifestIdentityMismatch)
        );
    }
}

#[test]
fn malformed_folder_rows_do_not_hide_valid_subpacks() {
    let mut manifest = manifest(PACK_ID, 65);
    for folder in [
        "",
        ".",
        "..",
        "../outside",
        "nested/folder",
        "nested\\folder",
    ] {
        manifest["subpacks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"folder_name": folder, "name": "Invalid", "memory_tier": 11}));
    }
    let parsed = read_manifest(&serde_json::to_vec(&manifest).unwrap(), PACK_ID, VERSION).unwrap();
    assert_eq!(parsed.subpacks.len(), 65);
    assert_eq!(parsed.subpacks.last().unwrap().folder, "p64");
    assert!(
        parsed
            .subpacks
            .iter()
            .all(|pack| pack.folder.starts_with('p'))
    );
}

#[cfg(feature = "handoff")]
#[test]
fn server_handoff_keeps_last_subpack_and_layer_precedence() {
    use protocol::{ResourcePackArchive, ResourcePackHandoff};

    for count in [64, 65] {
        let manifest = manifest(PACK_ID, count);
        let selected = manifest["subpacks"].as_array().unwrap().last().unwrap()["folder_name"]
            .as_str()
            .unwrap();
        let bytes = tiered_archive(&manifest);
        let lower_id = Uuid::from_u128(2);
        let lower = archive(
            &self::manifest(lower_id, 0),
            &[
                ("marker.txt", b"lower"),
                ("lower-only.txt", b"lower fallback"),
            ],
        );
        for (requested, memory, expected) in [
            ("", 4 << 30, selected),
            (selected, 4 << 30, selected),
            (selected, 0, ""),
        ] {
            let stack = crate::validate_handoff_for_device(
                ResourcePackHandoff::from_archives(vec![
                    ResourcePackArchive::unencrypted(
                        PACK_ID,
                        VERSION.into(),
                        requested.into(),
                        bytes.clone(),
                    ),
                    ResourcePackArchive::unencrypted(
                        lower_id,
                        VERSION.into(),
                        String::new(),
                        lower.clone(),
                    ),
                ]),
                memory,
            );
            assert!(stack.rejections().is_empty(), "{stack:?}");
            assert_eq!(stack.packs().len(), 2);
            assert_eq!(stack.packs()[1].sub_pack_name(), expected);
            let view = LayeredPackView::new(stack);
            assert_eq!(
                view.read("marker.txt").unwrap().as_ref(),
                if expected.is_empty() {
                    b"root"
                } else {
                    b"last"
                }
            );
            assert_eq!(view.read("sentinel.txt").is_some(), !expected.is_empty());
            if !expected.is_empty() {
                assert_eq!(
                    view.read("sentinel.txt").unwrap().as_ref(),
                    b"last sentinel"
                );
            }
            assert_eq!(
                view.read("root-only.txt").unwrap().as_ref(),
                b"root fallback"
            );
            assert_eq!(
                view.read("lower-only.txt").unwrap().as_ref(),
                b"lower fallback"
            );
            assert!(view.list("subpacks/").is_empty());
        }
    }
}

struct TemporaryLibrary(PathBuf);

impl TemporaryLibrary {
    /// Creates isolated storage for one import and installed-pack reload test.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-subpack-count-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TemporaryLibrary {
    /// Removes only the temporary storage owned by this fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn local_import_and_installed_reload_keep_the_last_subpack() {
    for count in [64, 65] {
        let directory = TemporaryLibrary::new();
        let manifest = manifest(PACK_ID, count);
        let selected = manifest["subpacks"].as_array().unwrap().last().unwrap()["folder_name"]
            .as_str()
            .unwrap();
        let source = directory.0.join("fixture.mcpack");
        fs::write(&source, tiered_archive(&manifest)).unwrap();
        let installed = directory.0.join("installed");
        let mut library = GlobalPackLibrary::open(&installed, [9, 0, 0]).unwrap();
        library.set_device_memory(4 << 30);
        let report = library.import(&source).unwrap();
        assert!(report.rejected.is_empty(), "{report:?}");
        assert_eq!(report.imported.len(), 1);
        assert_eq!(report.imported[0].subpacks.len(), count);
        library.activate(PACK_ID).unwrap();
        assert_eq!(library.active()[0].subpack, selected);
        library.select_subpack(PACK_ID, selected).unwrap();
        assert!(matches!(
            library.select_subpack(PACK_ID, "../outside"),
            Err(crate::LibraryError::Admission(
                AdmissionError::InvalidSubpack
            ))
        ));
        library.apply().unwrap();
        drop(library);
        let mut reopened = GlobalPackLibrary::open(installed, [9, 0, 0]).unwrap();
        assert_eq!(reopened.available()[0].subpacks.len(), count);
        assert_eq!(reopened.active()[0].subpack, selected);
        let view = LayeredPackView::new(reopened.preview().unwrap());
        assert_eq!(view.read("marker.txt").unwrap().as_ref(), b"last");
        assert_eq!(
            view.read("sentinel.txt").unwrap().as_ref(),
            b"last sentinel"
        );
        assert_eq!(
            view.read("root-only.txt").unwrap().as_ref(),
            b"root fallback"
        );
    }
}
