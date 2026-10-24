use std::path::PathBuf;

use crate::{Options, Platform};

/// Compiled asset inputs; the physics carrier comes from its explicit input path.
pub(crate) fn asset_files() -> impl Iterator<Item = &'static str> {
    assets::carriers::required()
        .filter(|carrier| carrier.name != assets::carriers::BLOCK_PHYSICS.name)
        .map(|carrier| carrier.output)
}

/// Runtime executable names and path overrides from the release packaging table.
pub(crate) fn runtime_binaries() -> impl Iterator<Item = (&'static str, &'static str)> {
    include_str!("../../../packaging/common/runtime-binaries.txt")
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next().expect("runtime binary name");
            fields.next().expect("runtime Cargo package");
            (name, fields.next().expect("runtime path override"))
        })
}

/// Directory containing sibling executables in each local distribution layout.
pub(crate) fn binary_root(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "",
        Platform::Linux => "bin/",
        Platform::Macos => "Cinnabar.app/Contents/MacOS/",
    }
}

/// Maps runtime executables and required resources to their distribution destinations.
pub(crate) fn input_files(options: &Options) -> Vec<(PathBuf, String)> {
    let binary_root = binary_root(options.platform);
    let suffix = if options.platform == Platform::Windows {
        ".exe"
    } else {
        ""
    };
    let resource_root = match options.platform {
        Platform::Windows => "resources/assets",
        Platform::Linux => "share/cinnabar/assets",
        Platform::Macos => "Cinnabar.app/Contents/Resources/assets",
    };
    let mut files: Vec<_> = runtime_binaries()
        .map(|(name, override_name)| {
            let filename = format!("{name}{suffix}");
            let source = match override_name {
                "CLIENT" => options.client.clone(),
                _ => options.binaries.join(&filename),
            };
            (source, format!("{binary_root}{filename}"))
        })
        .collect();
    files.extend([
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
    ]);
    files.extend(
        asset_files().map(|name| (options.assets.join(name), format!("{resource_root}/{name}"))),
    );
    files
}
