#[cfg(test)]
mod tests {
    use crate::asset_startup::{AssetPathSource, select_asset_path_with_default};
    use launcher::{install_layout::Platform, test_support::scratch_at};
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
