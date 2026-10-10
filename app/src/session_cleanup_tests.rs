use launcher_host::session_cleanup::SessionDirectoryGuard;
use std::process;

#[test]
fn session_controller_binding_replaces_releases_and_drops_cleanly() {
    let layout = launcher::test_support::scratch("session-controller-binding");
    let mut controller = crate::session::SessionController::default();

    let first = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 1))
        .expect("bind first");
    let first_directory = layout.connect_socket_dir(process::id(), 1);
    controller.bind_directory(first);
    assert!(first_directory.is_dir());

    // Binding a replacement releases the superseded session directory.
    let second = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 2))
        .expect("bind second");
    let second_directory = layout.connect_socket_dir(process::id(), 2);
    controller.bind_directory(second);
    assert!(
        !first_directory.exists(),
        "replaced binding removes the old session directory"
    );
    assert!(second_directory.exists());

    controller.release_directory();
    assert!(!second_directory.exists());

    let third = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 3))
        .expect("bind third");
    let third_directory = layout.connect_socket_dir(process::id(), 3);
    controller.bind_directory(third);
    drop(controller);
    assert!(
        !third_directory.exists(),
        "dropping the session controller removes its session directory"
    );
    let _ = fs::remove_dir_all(root.path().join(".local"));
}
