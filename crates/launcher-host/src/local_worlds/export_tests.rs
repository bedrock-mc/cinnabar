use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use bridge::Backend;
use launcher::local_worlds::world::NewWorld;
use zip::ZipArchive;

use super::{
    export::export,
    store::{LEVEL_DIR, Store, THUMBNAIL_FILE},
    test_support::scratch,
    world::WorldError,
};

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    exports: PathBuf,
}

fn fixture() -> Fixture {
    let dir = scratch();
    let worlds = dir.path().join("worlds");
    let exports = dir.path().join("exports");
    fs::create_dir_all(&exports).expect("exports");
    Fixture {
        store: Store::open(worlds).expect("store"),
        exports,
        _dir: dir,
    }
}

impl Fixture {
    fn populate_world(&self, id: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = self.store.dir(id).expect("world dir");
        let level = dir.join(LEVEL_DIR);
        fs::create_dir_all(&level).expect("level dir");
        for (rel_path, content) in files {
            let full_path = level.join(rel_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).expect("parent dir");
            }
            fs::write(full_path, content).expect("write file");
        }
        dir
    }

    fn read_zip_entries(&self, zip_path: &Path) -> Vec<(String, Vec<u8>)> {
        let file = File::open(zip_path).expect("open zip");
        let mut archive = ZipArchive::new(file).expect("read zip");
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).expect("zip entry");
            let name = entry.name().to_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).expect("read data");
            entries.push((name, data));
        }
        entries
    }
}

#[test]
fn test_export_bds_world() {
    let fix = fixture();
    let world = fix
        .store
        .create(&NewWorld {
            name: "BDS Survival".to_owned(),
            backend: Some(Backend::Bds),
            ..NewWorld::default()
        })
        .expect("create");

    fix.populate_world(
        &world.id,
        &[
            ("level.dat", b"level-dat-bytes"),
            ("levelname.txt", b"BDS Survival"),
            ("db/000001.ldb", b"chunk-data-1"),
            ("db/CURRENT", b"MANIFEST-000001\n"),
            ("LOCK", b""),
        ],
    );

    let dest = fix.exports.join("bds_world.mcworld");
    export(&fix.store, &world.id, &dest).expect("export");

    assert!(dest.is_file(), "mcworld file must exist");
    let entries = fix.read_zip_entries(&dest);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

    assert!(names.contains(&"level.dat"));
    assert!(names.contains(&"levelname.txt"));
    assert!(names.contains(&"db/000001.ldb"));
    assert!(names.contains(&"db/CURRENT"));
    assert!(!names.contains(&"LOCK"), "LOCK file must be skipped");
    assert!(!names.contains(&"world.json"), "world.json must be skipped");

    let level_name = entries.iter().find(|(n, _)| n == "levelname.txt").unwrap();
    assert_eq!(level_name.1, b"BDS Survival");
}

#[test]
fn test_export_dragonfly_world() {
    let fix = fixture();
    let world = fix
        .store
        .create(&NewWorld {
            name: "Dragonfly Flat".to_owned(),
            backend: Some(Backend::Dragonfly),
            ..NewWorld::default()
        })
        .expect("create");

    let world_dir = fix.populate_world(
        &world.id,
        &[
            ("level.dat", b"level-dat-bytes"),
            ("db/000002.ldb", b"chunk-data-2"),
        ],
    );

    fs::write(world_dir.join(THUMBNAIL_FILE), b"icon-jpeg-bytes").expect("thumbnail");

    let dest = fix.exports.join("dragonfly.mcworld");
    export(&fix.store, &world.id, &dest).expect("export");

    let entries = fix.read_zip_entries(&dest);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

    assert!(names.contains(&"level.dat"));
    assert!(
        names.contains(&"levelname.txt"),
        "levelname.txt should be synthesized"
    );
    assert!(
        names.contains(&"world_icon.jpeg"),
        "thumbnail should be included at root"
    );
    assert!(names.contains(&"db/000002.ldb"));

    let level_name = entries.iter().find(|(n, _)| n == "levelname.txt").unwrap();
    assert_eq!(level_name.1, b"Dragonfly Flat");

    let icon = entries
        .iter()
        .find(|(n, _)| n == "world_icon.jpeg")
        .unwrap();
    assert_eq!(icon.1, b"icon-jpeg-bytes");
}

#[test]
fn test_export_ungenerated_world_fails() {
    let fix = fixture();
    let world = fix
        .store
        .create(&NewWorld {
            name: "Empty World".to_owned(),
            ..NewWorld::default()
        })
        .expect("create");

    let dest = fix.exports.join("empty.mcworld");
    let err = export(&fix.store, &world.id, &dest).unwrap_err();
    assert!(matches!(err, WorldError::NoWorldData));
    assert!(!dest.exists());
}
