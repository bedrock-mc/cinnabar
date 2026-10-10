//! Host compatibility exports for the launcher installation model.
use launcher::install_layout::InstallLayout;

#[cfg(test)]
use launcher::install_layout::{InstallEnvironment, Platform};

/// Creates an isolated installation tree for app tests that own local files.
#[cfg(test)]
pub(crate) fn scratch(label: &str) -> InstallLayout {
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
#[cfg(test)]
pub(crate) fn checkout() -> InstallLayout {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the app crate sits inside the workspace");
    InstallLayout::resolve(
        host_platform(),
        &InstallEnvironment {
            executable: root.join("target/debug/bedrock-client"),
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

#[cfg(test)]
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
#[cfg(test)]
fn scratch_at(platform: Platform, base: &std::path::Path) -> InstallLayout {
    let executable = match platform {
        Platform::Windows => "bin/bedrock-client.exe",
        Platform::Linux => "bin/bedrock-client",
        Platform::MacOs => "Cinnabar.app/Contents/MacOS/bedrock-client",
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

#[cfg(test)]
mod tests {
    use crate::asset_startup::{AssetPathSource, select_asset_path_with_default};
    use std::{ffi::OsString, path::PathBuf};
    use {super::scratch_at, launcher::install_layout::Platform};

    #[test]
    fn scratch_layouts_keep_every_platform_under_the_supplied_root() {
        for (platform, root) in [
            (
                Platform::Windows,
                "C:/Users/test/AppData/Local/Temp/cinnabar-scratch",
            ),
            (Platform::Linux, "/tmp/cinnabar-scratch"),
            (Platform::MacOs, "/private/tmp/cinnabar-scratch"),
        ] {
            let base = PathBuf::from(root);
            let layout = scratch_at(platform, &base);
            for path in [
                &layout.resource_root,
                &layout.compiled_assets,
                &layout.physics_registry,
                &layout.core_executable,
                &layout.user_config_root,
                &layout.user_data_root,
                &layout.runtime_root,
                layout.transient_runtime_root(),
            ] {
                assert!(path.starts_with(&base), "{platform:?}: {}", path.display());
            }
            assert_eq!(
                layout.core_executable.extension().is_some(),
                platform == Platform::Windows
            );
        }
    }

    #[test]
    fn explicit_asset_sources_precede_the_layout_default() {
        let default = PathBuf::from("/bundle/resources/assets/vanilla-v2193.mcbea");
        let environment = select_asset_path_with_default(
            None,
            Some(OsString::from("/override/environment.mcbea")),
            &default,
        );
        assert_eq!(environment.source, AssetPathSource::Environment);
        let command_line = select_asset_path_with_default(
            Some(PathBuf::from("/override/cli.mcbea").as_path()),
            Some(OsString::from("/override/environment.mcbea")),
            &default,
        );
        assert_eq!(command_line.source, AssetPathSource::CommandLine);
        assert_eq!(
            select_asset_path_with_default(None, None, &default).path,
            default
        );
    }
}
