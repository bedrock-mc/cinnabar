use super::*;
use bevy::{ecs::world::CommandQueue, prelude::World};

fn child_guard(exited: bool) -> CoreProcessGuard {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd.exe");
        command.args(["/Q", "/C", if exited { "exit 0" } else { "set /p hold=" }]);
        command
    };
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.args(["-c", if exited { "exit 0" } else { "read -r hold" }]);
        command
    };
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    if exited {
        assert!(child.wait().unwrap().success());
    }
    let mut guard = CoreProcessGuard::default();
    guard.replace(child);
    guard
}

fn fixture(exited: bool) -> (LauncherCoreSlot, MenuRuntime, World, PathBuf) {
    let layout = crate::install_layout::scratch("launcher-child-recovery");
    let socket_dir = layout.account_socket_dir(std::process::id(), 0);
    let directory = SessionDirectoryGuard::bind(socket_dir.clone()).unwrap();
    std::fs::write(
        crate::runtime::endpoint::bridge_endpoint_path(&socket_dir),
        [],
    )
    .unwrap();
    #[cfg(unix)]
    std::fs::write(launcher_control::control_endpoint_path(&socket_dir), []).unwrap();
    let slot = LauncherCoreSlot {
        core: Some(LauncherCore {
            _guard: child_guard(exited),
            _directory: directory,
            socket_dir: socket_dir.clone(),
            authenticated: false,
            auth_cache: None,
            attached: true,
        }),
        failed: None,
        retiring: None,
    };
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu.control_auth = Some(AuthState::SignedOut);
    let mut world = World::new();
    world.insert_resource(LauncherAccount::new(
        socket_dir.clone(),
        socket_dir.join("artwork"),
    ));
    (slot, menu, world, socket_dir)
}

fn drive(slot: &mut LauncherCoreSlot, menu: &mut MenuRuntime, world: &mut World, idle: bool) {
    let mut queue = CommandQueue::default();
    slot.drive(
        &mut Commands::new(&mut queue, world),
        menu,
        idle,
        false,
        None,
    );
    queue.apply(world);
}

#[test]
fn dead_child_detaches_clients_during_play_and_retries_unchanged_auth_when_idle() {
    let (mut slot, mut menu, mut world, socket_dir) = fixture(true);
    // A failed-mode latch must not suppress recovery of a child that subsequently existed.
    slot.failed = Some(false);
    drive(&mut slot, &mut menu, &mut world, false);
    assert!(
        slot.core.is_none(),
        "a dead child cannot serve another join"
    );
    assert!(slot.failed.is_none());
    assert!(!world.contains_resource::<LauncherAccount>());
    assert!(menu.control_auth.is_none());
    assert!(
        !socket_dir.exists(),
        "the dead core's stale endpoint is reclaimed"
    );
    assert!(!menu.layout.core_executable.exists());

    drive(&mut slot, &mut menu, &mut world, true);
    assert_eq!(
        slot.failed,
        Some(false),
        "idle must attempt the same auth mode again"
    );
}

#[test]
fn healthy_child_and_account_client_survive_idle_and_active_frames() {
    let (mut slot, mut menu, mut world, socket_dir) = fixture(false);
    let pid = slot.core.as_ref().unwrap()._guard.id();
    for idle in [false, true, false, true] {
        drive(&mut slot, &mut menu, &mut world, idle);
        assert_eq!(slot.core.as_ref().unwrap()._guard.id(), pid);
        assert!(world.contains_resource::<LauncherAccount>());
        assert_eq!(menu.control_auth, Some(AuthState::SignedOut));
        assert!(socket_dir.exists());
        assert!(slot.failed.is_none());
    }
}

#[test]
fn dead_child_is_replaced_without_a_sign_in_change_and_waits_for_new_readiness() {
    let (mut slot, mut menu, mut world, socket_dir) = fixture(true);
    let old_pid = slot.core.as_ref().unwrap()._guard.id();
    let executable = &menu.layout.core_executable;
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    // A harmless process fixture proves production spawn/adoption; it intentionally offers no IPC.
    #[cfg(windows)]
    std::fs::copy(
        PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"),
        executable,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    drive(&mut slot, &mut menu, &mut world, true);
    let core = slot
        .core
        .as_ref()
        .expect("same-mode dead child must restart");
    assert_ne!(core._guard.id(), old_pid);
    assert!(!core.authenticated);
    assert!(
        !core.attached,
        "old readiness must not attach clients to the replacement"
    );
    assert!(slot.failed.is_none());
    assert!(!world.contains_resource::<LauncherAccount>());
    assert!(!crate::runtime::endpoint::bridge_endpoint_exists(
        &socket_dir
    ));
    #[cfg(unix)]
    assert!(!launcher_control::control_endpoint_path(&socket_dir).exists());
}

#[test]
fn a_new_account_retires_the_authenticated_core_even_when_sign_in_mode_is_unchanged() {
    let (mut slot, mut menu, mut world, _) = fixture(false);
    let core = slot.core.as_mut().unwrap();
    core.authenticated = true;
    core.auth_cache = Some(menu.layout.auth_cache());
    menu.feeds.account_adding = true;
    menu.accounts.pending_ready = true;
    drive(&mut slot, &mut menu, &mut world, false);
    assert!(
        slot.core.is_some(),
        "active play keeps its owning account core"
    );
    drive(&mut slot, &mut menu, &mut world, true);
    assert!(slot.core.is_none());
    assert!(!world.contains_resource::<LauncherAccount>());
    assert!(
        menu.accounts.skip_control,
        "old profile data must not enter the new account"
    );
    // The retire signal belongs to `drive`; taking it here races the sender's drop.
    test_time::eventually_within(Duration::from_secs(5), "the retired core to stop", || {
        if slot.retiring.is_none() {
            return true;
        }
        drive(&mut slot, &mut menu, &mut world, true);
        false
    });
    assert_eq!(slot.failed, Some(true));
}

#[test]
fn account_switch_waits_for_the_previous_profiles_credential_save() {
    let (mut slot, mut menu, mut world, _) = fixture(false);
    let old_pid = slot.core.as_ref().unwrap()._guard.id();
    let (saved, pending) = crossbeam_channel::bounded(1);
    menu.accounts.remember = Some(pending);
    menu.accounts.operation = Some(super::super::accounts::Operation::Switch("2".into()));
    drive(&mut slot, &mut menu, &mut world, true);
    assert_eq!(slot.core.as_ref().unwrap()._guard.id(), old_pid);
    assert!(menu.accounts.operation.is_some());
    assert!(menu.accounts.work.is_none());
    drop(saved);
    menu.poll_accounts();
    assert!(
        menu.accounts.remember.is_none(),
        "an interrupted saver cannot stall account changes"
    );
    drive(&mut slot, &mut menu, &mut world, true);
    assert!(slot.core.is_none());
    assert!(menu.accounts.work.is_some());
}

#[test]
fn retired_account_control_paths_cannot_reach_a_new_core_or_game_session() {
    let layout = crate::install_layout::scratch("account-endpoint-isolation");
    let first = next_account_socket_dir(&layout);
    let second = next_account_socket_dir(&layout);
    assert_ne!(first, second);
    for generation in 0..3 {
        let game = layout.connect_socket_dir(std::process::id(), generation);
        assert_ne!(first, game);
        assert_ne!(second, game);
    }
}

#[test]
fn realm_membership_core_exit_retires_the_pending_dialog() {
    use launcher::menu::realm_membership::{Stage, State};
    for stage in [Stage::Verifying, Stage::Joining] {
        let (mut slot, mut menu, mut world, _) = fixture(true);
        menu.control_auth = Some(AuthState::Authenticated);
        menu.realm_membership.state = Some(State {
            stage,
            ..Default::default()
        });
        drive(&mut slot, &mut menu, &mut world, false);
        assert!(!world.contains_resource::<LauncherAccount>());
        assert!(
            menu.realm_membership.state.is_none(),
            "a detached account control cannot complete the dialog"
        );
    }
}
