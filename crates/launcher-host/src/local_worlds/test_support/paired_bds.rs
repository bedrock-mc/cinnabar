//! Real original BdsRunner over a dedicated, already provisioned fixture installation.
use crate::local_worlds::{
    Manager,
    bds::BdsRunner,
    manager::Parts,
    provision::{Platform, Provisioner},
    store::Store,
};
use launcher::local_worlds::world::{Generator, NewWorld};
use std::{path::PathBuf, sync::Arc};

/// This root is a disposable fixture install: the original runner writes its normal properties/world link.
const ROOT_ENV: &str = "CINNABAR_NATIVE_HOST_BDS_ROOT";

pub(crate) struct World {
    pub(crate) manager: Manager,
    _directory: tempfile::TempDir,
}

impl World {
    /// Uses only a previously provisioned exact build and EULA; it neither downloads nor substitutes a runner.
    pub(crate) fn open(lan: bool) -> Option<Self> {
        if !Platform::host().runs_bds() {
            eprintln!("missing fixture: native BDS platform");
            return None;
        }
        let Some(root) = std::env::var_os(ROOT_ENV) else {
            eprintln!("missing fixture: {ROOT_ENV} (dedicated provisioned BDS install root)");
            return None;
        };
        let root = PathBuf::from(root);
        assert!(
            root.is_absolute() && root.is_dir(),
            "invalid BDS fixture root"
        );
        let pin = super::super::server_pin();
        let mut provisioner = Provisioner::new(root, pin.server_version.clone());
        // Missing fixture files cannot turn an acceptance control into an implicit download.
        provisioner.allow_host = Arc::new(|_| false);
        if !provisioner.eula_accepted() || provisioner.installed().is_none() {
            eprintln!("missing fixture: {ROOT_ENV} exact installed build/EULA");
            return None;
        }
        let provisioner = Arc::new(provisioner);
        let mut runner = BdsRunner::new(Arc::clone(&provisioner), pin.bds_container_image.clone());
        runner.lan_visible = lan;
        runner.start_timeout = Some(crate::native_account::ACQUISITION_TIMEOUT);
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path().join("worlds")).unwrap());
        let mut parts = Parts::new(store);
        parts.runners.bds = Some(Arc::new(runner));
        parts.setup = Some(provisioner);
        let manager = Manager::from_parts(parts);
        let saved = manager
            .create(&NewWorld {
                name: "Original BDS native host fixture".into(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Bds),
                ..Default::default()
            })
            .unwrap();
        let world = Self {
            manager,
            _directory: directory,
        };
        world.manager.open(&saved.id, Some(2)).unwrap();
        test_time::eventually_within(
            crate::native_account::ACQUISITION_TIMEOUT,
            "actual BDS process ready",
            || world.manager.status().state == bridge::WorldState::Running,
        );
        Some(world)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.manager.shutdown();
    }
}
