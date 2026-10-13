//! Known user-data roots; startup checks directory metadata without enumerating profiles.

use launcher::install_layout::Platform;
use std::path::{Path, PathBuf};

const MAX_PROFILES: usize = 32;

/// A known root; profile containers are enumerated only after explicit consent.
#[derive(Clone, Debug)]
pub(super) struct Location {
    pub path: PathBuf,
    pub label: String,
    pub profiles: bool,
}

/// One player profile and its optional shared, user-installed packs.
#[derive(Clone, Debug)]
pub(super) struct Source {
    pub root: PathBuf,
    pub shared: Option<PathBuf>,
    pub label: String,
}

/// Uses only documented environment paths, never inspecting installed game files.
pub(super) fn current() -> Vec<Location> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let platform = if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    known(
        platform,
        home.as_deref(),
        roaming.as_deref(),
        local.as_deref(),
        xdg.as_deref(),
    )
}

/// Builds a fixed directory inventory from the platform's user environment.
fn known(
    platform: Platform,
    home: Option<&Path>,
    roaming: Option<&Path>,
    local: Option<&Path>,
    xdg: Option<&Path>,
) -> Vec<Location> {
    let mut locations = Vec::new();
    let mut add = |base: Option<&Path>, suffix: &str, label: &str, profiles| {
        if let Some(base) = base.filter(|base| base.is_absolute()) {
            locations.push(Location {
                path: base.join(suffix),
                label: label.to_owned(),
                profiles,
            });
        }
    };
    match platform {
        Platform::Windows => {
            add(roaming, "Minecraft Bedrock/Users", "Windows", true);
            add(
                roaming,
                "Minecraft Bedrock Preview/Users",
                "Windows Preview",
                true,
            );
            add(
                local,
                "Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang",
                "Windows legacy",
                false,
            );
        }
        Platform::MacOs => {
            add(
                home,
                "Library/Containers/com.mojang.minecraftpe/Data/Documents/games/com.mojang",
                "PlayCover",
                false,
            );
            add(
                home,
                "Library/Containers/com.mojang.minecraftpe/Documents/games/com.mojang",
                "PlayCover",
                false,
            );
            add(
                home,
                "Library/Application Support/mcpelauncher/games/com.mojang",
                "Bedrock Launcher",
                false,
            );
        }
        Platform::Linux => {
            if let Some(xdg) = xdg.filter(|base| base.is_absolute()) {
                add(
                    Some(xdg),
                    "mcpelauncher/games/com.mojang",
                    "Bedrock Launcher",
                    false,
                );
            } else {
                add(
                    home,
                    ".local/share/mcpelauncher/games/com.mojang",
                    "Bedrock Launcher",
                    false,
                );
            }
            add(
                home,
                ".var/app/io.mrarm.mcpelauncher/data/mcpelauncher/games/com.mojang",
                "Bedrock Launcher (Flatpak)",
                false,
            );
        }
    }
    locations
}

/// Enumerates only known profile children after consent, without reading account files.
pub(super) fn discover(locations: &[Location]) -> anyhow::Result<Vec<Source>> {
    let mut sources = Vec::new();
    for location in locations {
        if !location.path.is_dir() {
            continue;
        }
        if std::fs::symlink_metadata(&location.path)?
            .file_type()
            .is_symlink()
        {
            continue;
        }
        if location.profiles {
            let mut profiles = Vec::new();
            for entry in std::fs::read_dir(&location.path)?.take(MAX_PROFILES + 1) {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    profiles.push(entry.path());
                }
            }
            if profiles.len() > MAX_PROFILES {
                anyhow::bail!(
                    "Too many Minecraft profiles; import at most {MAX_PROFILES} profiles from one install."
                );
            }
            profiles.sort();
            let shared = location.path.join("Shared/games/com.mojang");
            for (index, profile) in profiles.into_iter().enumerate() {
                let root = profile.join("games/com.mojang");
                if root.is_dir() && !sources.iter().any(|source: &Source| source.root == root) {
                    sources.push(Source {
                        shared: (shared.is_dir() && root != shared).then(|| shared.clone()),
                        root,
                        label: format!("{} — profile {}", location.label, index + 1),
                    });
                }
            }
        } else if !sources.iter().any(|source| source.root == location.path) {
            sources.push(Source {
                root: location.path.clone(),
                shared: None,
                label: location.label.clone(),
            });
        }
        if sources.len() >= MAX_PROFILES {
            sources.truncate(MAX_PROFILES);
            break;
        }
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_derived_without_reading_their_contents() {
        let root = tempfile::tempdir().unwrap();
        let windows = known(
            Platform::Windows,
            Some(root.path()),
            Some(root.path()),
            Some(root.path()),
            None,
        );
        assert!(windows[0].profiles);
        assert_eq!(windows[0].path, root.path().join("Minecraft Bedrock/Users"));
        assert!(
            known(Platform::Linux, Some(root.path()), None, None, None)
                .iter()
                .any(|path| path.label.contains("Flatpak"))
        );
        assert!(
            known(Platform::MacOs, Some(root.path()), None, None, None)
                .iter()
                .any(|path| path.label == "PlayCover")
        );
        assert!(known(Platform::Windows, None, None, None, None).is_empty());
    }

    #[test]
    fn profiles_are_separate_and_share_only_the_pack_root() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["100", "200", "Shared"] {
            std::fs::create_dir_all(temp.path().join(name).join("games/com.mojang")).unwrap();
        }
        let sources = discover(&[Location {
            path: temp.path().to_owned(),
            label: "Windows".into(),
            profiles: true,
        }])
        .unwrap();
        assert_eq!(sources.len(), 3);
        assert_eq!(
            sources[0].shared,
            Some(temp.path().join("Shared/games/com.mojang"))
        );
        assert_ne!(sources[0].root, sources[1].root);
    }
}
