//! Isolated installation layouts for integration tests.

use crate::install_layout::{InstallEnvironment, InstallLayout, Platform};

/// Creates an isolated installation tree for integration tests that own local files.
pub fn scratch(label: &str) -> InstallLayout {
    // The clock separates runs of a reused pid; the counter separates calls within one process.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "cinnabar-layout-{label}-{}-{nonce}-{call}",
        std::process::id()
    ));
    scratch_at(host_platform(), &base)
}

/// The checkout's development layout. Resolved from the crate source, not the test binary, so a
/// relocated Cargo build dir still finds `.local`.
pub fn checkout() -> InstallLayout {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the launcher crate sits beneath the workspace");
    InstallLayout::resolve(
        host_platform(),
        &InstallEnvironment {
            executable: root.join("target/debug/cinnabar"),
            user_root: None,
            home: None,
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        },
    )
    .expect("a target/debug executable resolves to the development layout")
}

/// Returns the platform whose path rules match this test process.
fn host_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

/// Resolves a test installation under an explicit root for platform coverage.
pub fn scratch_at(platform: Platform, base: &std::path::Path) -> InstallLayout {
    let executable = match platform {
        Platform::Windows => "bin/cinnabar.exe",
        Platform::Linux => "bin/cinnabar",
        Platform::MacOs => "Cinnabar.app/Contents/MacOS/cinnabar",
    };
    InstallLayout::resolve(
        platform,
        &InstallEnvironment {
            executable: base.join(executable),
            user_root: None,
            home: Some(base.to_owned()),
            local_app_data: Some(base.join("data")),
            xdg_config_home: Some(base.join("config")),
            xdg_data_home: Some(base.join("data")),
            xdg_runtime_dir: Some(base.join("run")),
        },
    )
    .expect("absolute test roots form a supported installation")
}

/// A signed-in view with the account picker open over two saved accounts with pictures,
/// the first active.
pub fn saved_accounts_view() -> crate::menu::MenuView {
    let mut view = crate::menu::MenuView::new(true, "First".into());
    view.auth_state = crate::menu::auth::AuthState::Authenticated;
    view.dialog = Some(crate::menu::MenuDialog::Accounts);
    view.feeds.account_active_id = Some("first".into());
    view.feeds.accounts = vec![
        crate::accounts::AccountProfile {
            id: "first".into(),
            gamertag: "First".into(),
            picture_path: Some("first.png".into()),
        },
        crate::accounts::AccountProfile {
            id: "second".into(),
            gamertag: "Second".into(),
            picture_path: Some("second.png".into()),
        },
    ];
    view
}
