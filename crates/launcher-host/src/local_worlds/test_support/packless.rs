//! The original manager lifecycle over the real fixture protocol endpoint, without a BDS binary.

use crate::local_worlds::{
    Manager,
    manager::Parts,
    provision::{Platform, Provisioner},
    store::Store,
    test_support::FakeRunner,
};
use launcher::local_worlds::world::{Generator, NewWorld};
use std::sync::Arc;

pub(crate) struct World {
    pub(crate) manager: Manager,
    pub(crate) runner: Arc<FakeRunner>,
    _directory: tempfile::TempDir,
}

impl World {
    /// Captures the actual listener address through the same runner/opening/backend selection.
    /// Only local process supervision is in-memory; both protocol associations remain real.
    pub(crate) fn open(address: std::net::SocketAddr) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path().join("worlds")).unwrap());
        let mut provisioner =
            Provisioner::new(directory.path().join("unused-bds-install"), String::new());
        provisioner.platform = Platform {
            os: "linux",
            arch: "amd64",
        };
        provisioner.accept_eula().unwrap();
        let runner = Arc::new(FakeRunner {
            address: Some(address.to_string()),
            ..Default::default()
        });
        let mut parts = Parts::new(store);
        parts.setup = Some(Arc::new(provisioner));
        parts.runners.bds = Some(runner.clone());
        let manager = Manager::from_parts(parts);
        let saved = manager
            .create(&NewWorld {
                name: "Packless protocol world".into(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Bds),
                ..Default::default()
            })
            .unwrap();
        manager.open(&saved.id, Some(2)).unwrap();
        test_time::eventually("original packless manager opening", || {
            manager.status().state == bridge::WorldState::Running
        });
        Self {
            manager,
            runner,
            _directory: directory,
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.manager.shutdown();
    }
}
