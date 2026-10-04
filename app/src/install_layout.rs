//! Host compatibility exports for the launcher installation model.
pub use launcher::install_layout::InstallLayout;
#[cfg(test)]
pub use launcher::install_layout::vanilla_pack_relative;
#[cfg(test)]
pub(crate) use launcher::install_layout::{InstallEnvironment, Platform};

/// Creates an isolated installation tree for app tests that own local files.
#[cfg(test)]
pub(crate) fn scratch(label: &str) -> InstallLayout {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let base = std::env::temp_dir().join(format!(
        "cinnabar-layout-{label}-{}-{nonce}",
        std::process::id()
    ));
    let platform = if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    scratch_at(platform, &base)
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
    use super::{Platform, scratch_at};
    use crate::asset_startup::{AssetPathSource, select_asset_path_with_default};
    use std::{ffi::OsString, path::PathBuf};

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
