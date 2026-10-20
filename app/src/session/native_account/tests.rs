use super::*;
use launcher::{accounts::AccountStore, install_layout::InstallLayout};
use std::{fs, io::Write};

/// Keeps every account file beneath an owned temporary installation.
fn installation() -> (tempfile::TempDir, InstallLayout) {
    let root = tempfile::tempdir().unwrap();
    let layout =
        launcher::test_support::scratch_at(launcher::install_layout::Platform::Linux, root.path());
    (root, layout)
}

/// Uses the ordinary hidden direct-start menu and its actual saved-account snapshot.
fn menu(layout: &InstallLayout) -> MenuRuntime {
    MenuRuntime::new_with_layout(
        false,
        Some(2),
        "Synthetic".into(),
        layout.clone(),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Synthetic"),
    )
    .with_native_sign_in(true)
}

/// Writes synthetic refresh-only input; these credentials must never reach a service.
fn seed(layout: &InstallLayout, generation: &str) {
    let path = layout.auth_cache();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "refresh_token":"synthetic-refresh",
                "cinnabar_sign_in_generation":generation
            }))
            .unwrap(),
        )
        .unwrap();
}

/// Builds the explicit raw service intent with the original generated signaling grammar.
fn raw_target() -> protocol::SessionEndpoint {
    protocol::SessionEndpoint::Native(protocol::NativeConnectionTarget::new(
        format!(
            "{}{}/42",
            bridge::contract::NETHERNET_TARGET_PREFIX,
            bridge::contract::NETHERNET_SIGNALING_WEBSOCKET
        ),
        protocol::NativeTransport::RakNet,
    ))
}

#[tokio::test]
async fn startup_account_shares_the_original_menu_owner_and_rejects_replacement_before_service_io()
{
    let (_root, layout) = installation();
    let store = AccountStore::new(layout.auth_cache());
    seed(&layout, "original");
    store.remember_current("123", "Original", None).unwrap();
    seed(&layout, "replacement");
    store.remember_current("456", "Replacement", None).unwrap();
    store.activate("123").unwrap();
    let mut menu = menu(&layout);
    assert_eq!(menu.selected_account_id(), Some("123"));
    let mut controller = SessionController::default();
    let endpoint = raw_target();
    let original = controller
        .native_startup_account(&mut menu, Some(&endpoint))
        .unwrap()
        .expect("startup lost the saved account input");
    assert!(Arc::ptr_eq(
        &original,
        &menu.native_account_for(layout.auth_cache())
    ));
    assert!(Arc::ptr_eq(
        &original,
        &controller.native_account_for(&mut menu, layout.auth_cache())
    ));
    store.activate("456").unwrap();
    let lease = store.try_native_lease().unwrap();
    assert_eq!(lease.selected_id(), Some("456"));
    assert_eq!(lease.generation(), "replacement");
    drop(lease);
    let protocol::SessionEndpoint::Native(target) = endpoint else {
        panic!("the raw service requires the native preparation owner");
    };
    let error = match original
        .prepare_connection(
            target,
            serde_json::json!({}),
            protocol::ClientRandomId::new(1).unwrap(),
        )
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("startup adopted another selected account"),
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn startup_account_preserves_offline_scope_and_rejects_a_lost_selected_cache() {
    let (_root, layout) = installation();
    let mut menu = menu(&layout);
    let mut controller = SessionController::default();
    let direct = protocol::SessionEndpoint::Native(protocol::NativeConnectionTarget::new(
        "127.0.0.1:19132".into(),
        protocol::NativeTransport::RakNet,
    ));
    assert!(
        controller
            .native_startup_account(&mut menu, Some(&direct))
            .unwrap()
            .is_none()
    );
    seed(&layout, "selected");
    AccountStore::new(layout.auth_cache())
        .remember_current("123", "Original", None)
        .unwrap();
    let mut menu = self::menu(&layout);
    fs::remove_file(layout.auth_cache()).unwrap();
    let error = controller
        .native_startup_account(&mut menu, Some(&direct))
        .err()
        .expect("missing selected credentials downgraded to offline play");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    fs::create_dir(layout.auth_cache()).unwrap();
    let error = controller
        .native_startup_account(&mut menu, Some(&direct))
        .err()
        .expect("an invalid selected cache downgraded to offline play");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    for endpoint in [
        protocol::SessionEndpoint::Core(layout.runtime_root.clone()),
        protocol::SessionEndpoint::Native(
            protocol::NativeConnectionTarget::local_lan(
                "127.0.0.1:19132".parse().unwrap(),
                "World".into(),
                protocol::NATIVE_JOIN_TIMEOUT,
            )
            .unwrap(),
        ),
    ] {
        assert!(
            controller
                .native_startup_account(&mut menu, Some(&endpoint))
                .unwrap()
                .is_none()
        );
    }
}
