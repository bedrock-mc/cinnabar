use std::path::PathBuf;

use crate::{Options, Platform};

/// The carriers a bundle ships: every one packaged startup requires.
pub(crate) fn asset_files() -> impl Iterator<Item = &'static str> {
    assets::carriers::required().map(|carrier| carrier.output)
}

pub(crate) fn input_files(options: &Options) -> Vec<(PathBuf, String)> {
    let (binary_root, resource_root, client_name, core_name) = match options.platform {
        Platform::Windows => (
            "",
            "resources/assets",
            "bedrock-client.exe",
            "bedrock-core.exe",
        ),
        Platform::Linux => (
            "bin/",
            "share/cinnabar/assets",
            "bedrock-client",
            "bedrock-core",
        ),
        Platform::Macos => (
            "Cinnabar.app/Contents/MacOS/",
            "Cinnabar.app/Contents/Resources/assets",
            "bedrock-client",
            "bedrock-core",
        ),
    };
    let mut files = vec![
        (
            options.client.clone(),
            format!("{binary_root}{client_name}"),
        ),
        (options.core.clone(), format!("{binary_root}{core_name}")),
        (
            options.physics.clone(),
            format!(
                "{resource_root}/{}",
                assets::carriers::physics_registry_basename()
            ),
        ),
        (
            options.notices.clone(),
            format!("{resource_root}/THIRD_PARTY_NOTICES.md"),
        ),
    ];
    files.extend(
        asset_files().map(|name| (options.assets.join(name), format!("{resource_root}/{name}"))),
    );
    files
}
