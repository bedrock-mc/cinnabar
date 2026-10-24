//! Actual production localserver process and an original pack in its configured resource directory.

use crate::local_worlds::{Manager, manager::Parts, process::ProcessRunner, store::Store};
use launcher::local_worlds::world::{Generator, NewWorld};
use std::{io::Write, path::PathBuf, sync::Arc};
use uuid::Uuid;

const PACK_ID: Uuid = Uuid::from_u128(0x61_0000_0000_4000_8000_0000_0000_0001);
const MODULE_ID: Uuid = Uuid::from_u128(0x61_0000_0000_4000_8000_0000_0000_0002);
const SERVER_ENV: &str = "CINNABAR_NATIVE_HOST_WORLD_SERVER";

pub(crate) struct World {
    pub(crate) manager: Manager,
    pub(crate) archive: Vec<u8>,
    pub(crate) pack: Uuid,
    _directory: tempfile::TempDir,
}

impl World {
    /// Uses only a root-built exact localserver fixture; never builds or substitutes a fake runner.
    pub(crate) fn open() -> Option<Self> {
        let Some(binary) = std::env::var_os(SERVER_ENV) else {
            eprintln!("missing fixture: {SERVER_ENV} (build tools/localserver)");
            return None;
        };
        let binary = PathBuf::from(binary);
        assert!(
            binary.is_absolute() && binary.is_file(),
            "invalid localserver fixture path"
        );
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path().join("worlds")).unwrap());
        let mut parts = Parts::new(Arc::clone(&store));
        parts.runners.dragonfly = Some(Arc::new(ProcessRunner::new(binary)));
        let manager = Manager::from_parts(parts);
        let saved = manager
            .create(&NewWorld {
                name: "Paired native host fixture".into(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Dragonfly),
                ..Default::default()
            })
            .unwrap();
        let archive = pack();
        let resources = store.dir(&saved.id).unwrap().join("resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("paired-fixture.mcpack"), &archive).unwrap();
        let world = Self {
            manager,
            archive,
            pack: PACK_ID,
            _directory: directory,
        };
        world.manager.open(&saved.id, Some(2)).unwrap();
        test_time::eventually("actual Dragonfly process ready", || {
            world.manager.status().state == bridge::WorldState::Running
        });
        Some(world)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.manager.shutdown();
    }
}

/// Builds only original metadata; no image, game asset or carrier is copied into the fixture.
fn pack() -> Vec<u8> {
    let engine = protocol::GAME_VERSION
        .split('.')
        .take(3)
        .map(|part| part.parse::<u32>().unwrap())
        .collect::<Vec<_>>();
    let manifest = serde_json::json!({
        "format_version":2,
        "header":{"name":"Paired host fixture", "description":"Original metadata fixture",
            "uuid":PACK_ID,"version":[1,0,0],"min_engine_version":engine},
        "modules":[{"type":"resources","uuid":MODULE_ID,"version":[1,0,0]}]
    });
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
