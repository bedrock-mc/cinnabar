use std::path::PathBuf;

use crate::{Options, Platform};

pub(crate) const ASSET_FILES: &[&str] = &[
    "ui-monocraft-v1.mcbefont",
    "vanilla-v1.mcbeatm",
    "vanilla-v1.mcbeent",
    "vanilla-v1.mcbehud",
    "vanilla-v1.mcbeico",
    "vanilla-v1.mcbelang",
    "vanilla-v1001.mcbea",
];

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
            format!("{resource_root}/block-physics-v1001.bin"),
        ),
        (
            options.notices.clone(),
            format!("{resource_root}/THIRD_PARTY_NOTICES.md"),
        ),
    ];
    files.extend(
        ASSET_FILES
            .iter()
            .map(|name| (options.assets.join(name), format!("{resource_root}/{name}"))),
    );
    files
}
