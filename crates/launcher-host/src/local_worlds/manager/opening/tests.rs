//! Hosting denial is independent of the still-running original local player route.

use super::*;
use crate::{local_worlds::test_support::NativeHostFixture, native_account::NativeAccount};
use launcher::{accounts::AccountStore, local_worlds::options::WorldOption};

/// A single-player choice seals both intent and direct publication before account acquisition.
#[test]
fn single_player_world_denies_native_hosting_without_retiring_the_local_player() {
    let fixture = NativeHostFixture::new();
    let manager = &fixture.manager;
    let selected = manager.native_host_intent().unwrap();
    let local = HostOpening::capture(manager).unwrap();
    let original_target = local.native_connection_target().unwrap();
    let namespace = tempfile::tempdir().unwrap();
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(namespace.path().join("account.json")),
        namespace.path().join("device.json"),
        None,
    ));
    {
        let mut life = lock(&manager.inner.life);
        life.world
            .as_mut()
            .unwrap()
            .options
            .set(WorldOption::Multiplayer, 0);
    }
    assert_eq!(
        manager.native_host_intent().err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        manager
            .native_hosted_world(
                account,
                Instant::now() + crate::native_account::ACQUISITION_TIMEOUT
            )
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    let current_target = local.native_connection_target().unwrap();
    assert_eq!(
        (current_target.address, current_target.transport),
        (original_target.address, original_target.transport)
    );
    assert!(!fixture.runner.last().stopped());
    assert!(!namespace.path().join("account.json").exists());
    lock(&manager.inner.life)
        .world
        .as_mut()
        .unwrap()
        .options
        .set(WorldOption::Multiplayer, 1);
    let enabled = manager.native_host_intent().unwrap();
    assert!(selected.same(&enabled));
}
