use std::path::{Path, PathBuf};
use thiserror::Error;

mod android;
pub use android::configure_android;

const APP_DIR: &str = crate::PRODUCT_NAME;

/// The pinned vanilla resource pack below a resource root, which mirrors `.local/`.
pub fn vanilla_pack_relative() -> String {
    assets::vanilla_source().installed_pack_dir("resource_pack")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallEnvironment {
    pub executable: PathBuf,
    pub user_root: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LayoutError {
    #[error("the current executable path is unavailable")]
    MissingExecutable,
    #[error("{platform} user home is unavailable")]
    MissingUserHome { platform: &'static str },
    #[error("LOCALAPPDATA is unavailable for the Windows installed layout")]
    MissingLocalAppData,
    #[error("{variable} must be an absolute path for the {platform} installed layout")]
    InvalidUserRoot {
        variable: &'static str,
        platform: &'static str,
    },
    #[error("Linux executable is not inside a supported <prefix>/bin layout: `{0}`")]
    InvalidLinuxLayout(PathBuf),
    #[error("macOS executable is not inside <name>.app/Contents/MacOS: `{0}`")]
    InvalidMacOsBundle(PathBuf),
    #[error("the Android activity has not configured the install layout")]
    AndroidUnconfigured,
    #[error("the Android install layout cannot change within a process")]
    AndroidReconfigured,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallLayout {
    pub resource_root: PathBuf,
    pub compiled_assets: PathBuf,
    pub physics_registry: PathBuf,
    pub core_executable: PathBuf,
    pub user_config_root: PathBuf,
    pub user_data_root: PathBuf,
    pub runtime_root: PathBuf,
    transient_runtime_root: PathBuf,
}

impl InstallLayout {
    pub fn resolve(
        platform: Platform,
        environment: &InstallEnvironment,
    ) -> Result<Self, LayoutError> {
        if environment.executable.as_os_str().is_empty() {
            return Err(LayoutError::MissingExecutable);
        }
        if let Some((root, binary_dir)) = development_root(&environment.executable) {
            let local = root.join(".local");
            let isolated = isolated_user_roots(platform, environment)?;
            let custom = isolated.is_some();
            let (config, data, runtime) = isolated
                .unwrap_or_else(|| (local.join("cinnabar"), local.clone(), local.join("run")));
            return Ok(Self {
                resource_root: local.clone(),
                compiled_assets: local.join("assets/compiled"),
                physics_registry: local
                    .join("assets/compiled")
                    .join(assets::carriers::physics_registry_basename()),
                core_executable: binary_dir.join(core_filename(platform)),
                user_config_root: config,
                user_data_root: data,
                runtime_root: runtime.clone(),
                transient_runtime_root: if custom {
                    runtime
                } else {
                    local.join("cinnabar")
                },
            });
        }

        let executable_dir = environment
            .executable
            .parent()
            .ok_or(LayoutError::MissingExecutable)?;
        let (resource_root, core_executable) = match platform {
            Platform::Windows => (
                executable_dir.join("resources"),
                executable_dir.join(core_filename(platform)),
            ),
            Platform::Linux => {
                let prefix = executable_dir
                    .parent()
                    .filter(|_| executable_dir.file_name().is_some_and(|name| name == "bin"))
                    .ok_or_else(|| {
                        LayoutError::InvalidLinuxLayout(environment.executable.clone())
                    })?;
                (
                    prefix.join("share/cinnabar"),
                    executable_dir.join(core_filename(platform)),
                )
            }
            Platform::MacOs => {
                let contents = executable_dir
                    .parent()
                    .filter(|contents| contents.file_name().is_some_and(|name| name == "Contents"))
                    .filter(|_| {
                        executable_dir
                            .file_name()
                            .is_some_and(|name| name == "MacOS")
                    })
                    .filter(|contents| {
                        contents.parent().and_then(Path::extension)
                            == Some(std::ffi::OsStr::new("app"))
                    })
                    .ok_or_else(|| {
                        LayoutError::InvalidMacOsBundle(environment.executable.clone())
                    })?;
                (
                    contents.join("Resources"),
                    executable_dir.join(core_filename(platform)),
                )
            }
        };
        let (user_config_root, user_data_root, runtime_root) = user_roots(platform, environment)?;
        Ok(Self {
            compiled_assets: resource_root.join("assets"),
            physics_registry: resource_root
                .join("assets")
                .join(assets::carriers::physics_registry_basename()),
            resource_root,
            core_executable,
            user_config_root,
            user_data_root,
            transient_runtime_root: runtime_root.clone(),
            runtime_root,
        })
    }

    pub fn discover() -> Result<Self, LayoutError> {
        if cfg!(target_os = "android") {
            return android::discover();
        }
        let platform = current_platform();
        let home = std::env::var_os(home_variable(platform)).map(PathBuf::from);
        let layout = Self::resolve(
            platform,
            &InstallEnvironment {
                executable: cargo_artifact_path(
                    std::env::current_exe().map_err(|_| LayoutError::MissingExecutable)?,
                    std::env::var_os("CARGO_MANIFEST_DIR")
                        .map(PathBuf::from)
                        .as_deref(),
                ),
                user_root: std::env::var_os("CINNABAR_USER_ROOT").map(PathBuf::from),
                home,
                local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
                xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
                xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
                xdg_runtime_dir: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            },
        )?;
        Ok(layout.with_prepared_assets())
    }

    /// True for a packaged install (not a `target/` checkout), where Mojang-derived carriers are
    /// prepared per user instead of shipped.
    #[must_use]
    pub fn is_installed(&self) -> bool {
        self.compiled_assets != self.resource_root.join("assets/compiled")
    }

    /// Redirects carrier loading to the per-user prepared directory when the bundle ships none.
    #[must_use]
    pub fn with_prepared_assets(mut self) -> Self {
        let bundled = self.world_assets();
        if self.is_installed() && !bundled.is_file() {
            self.compiled_assets = self.prepared_assets_dir();
        }
        self
    }

    #[must_use]
    pub fn prepared_assets_dir(&self) -> PathBuf {
        self.user_data_root.join("assets/compiled")
    }

    /// Scratch tree where first-run preparation fetches and compiles the vanilla pack.
    #[must_use]
    pub fn prepare_workspace(&self) -> PathBuf {
        self.user_data_root.join("prepare")
    }

    /// Bundled manifests, registries and `assetc` used by first-run preparation.
    #[must_use]
    pub fn prep_kit(&self) -> PathBuf {
        self.resource_root.join("prep-kit")
    }

    /// Download staging, installer logs, and temporary startup signals for this install.
    #[must_use]
    pub fn update_dir(&self) -> PathBuf {
        self.user_data_root.join("update")
    }

    #[must_use]
    pub fn log_dir(&self) -> PathBuf {
        self.user_data_root.join("logs")
    }

    #[must_use]
    pub fn screenshots_dir(&self) -> PathBuf {
        self.user_data_root.join("screenshots")
    }

    #[must_use]
    pub fn crash_dir(&self) -> PathBuf {
        self.user_data_root.join("crashes")
    }

    #[must_use]
    pub fn world_assets(&self) -> PathBuf {
        self.compiled_assets.join(assets::carriers::WORLD.output)
    }

    /// Optional Steve/Alex carrier compiled from the pinned vanilla pack.
    #[must_use]
    pub fn starter_skins_asset(&self) -> PathBuf {
        self.compiled_assets
            .join(assets::carriers::STARTER_SKINS.output)
    }

    /// The local player's own skin PNG, shipped beside the other assets under `resources/assets`
    /// (installed) or `.local/assets` (development). Cosmetic: absence falls back to the default.
    #[must_use]
    pub fn player_skin_asset(&self) -> PathBuf {
        self.resource_root.join("assets/skin/player.png")
    }

    pub fn dressing_room_dir(&self) -> PathBuf {
        self.user_data_root.join("skins")
    }

    pub fn dressing_room_capes_dir(&self) -> PathBuf {
        self.dressing_room_dir().join("capes")
    }

    pub fn skin_selection_file(&self) -> PathBuf {
        self.user_config_root.join("skin-selection.json")
    }

    #[must_use]
    pub fn auth_cache(&self) -> PathBuf {
        self.user_data_root.join("auth/microsoft-token.json")
    }

    /// The install's one login device profile, whichever account's token the core holds.
    #[must_use]
    pub fn device_profile_file(&self) -> PathBuf {
        self.auth_cache().with_file_name("device.json")
    }

    /// Downloaded launcher artwork, beside the auth cache with the core's persona art.
    #[must_use]
    pub fn launcher_artwork_dir(&self) -> PathBuf {
        self.auth_cache()
            .with_file_name("catalog-cache")
            .join("artwork")
    }

    /// The public keys of NetherNet servers the player trusted.
    #[must_use]
    pub fn server_trust_file(&self) -> PathBuf {
        self.user_data_root.join("trusted_server_public_keys.json")
    }

    /// The local vanilla resource pack (development checkouts carry it under
    /// `.local`); runtime fallbacks read images from it when present.
    #[must_use]
    pub fn vanilla_pack_dir(&self) -> PathBuf {
        if self.is_installed() {
            // An install ships no vanilla pack; first-run preparation keeps its textures per user.
            self.user_data_root.join(vanilla_pack_relative())
        } else {
            self.resource_root.join(vanilla_pack_relative())
        }
    }

    /// Imported optional packs and the applied global selection.
    #[must_use]
    pub fn global_resource_packs_dir(&self) -> PathBuf {
        self.user_data_root.join("resource-packs/global")
    }

    /// Mod components imported through Settings.
    #[must_use]
    pub fn mods_dir(&self) -> PathBuf {
        self.user_data_root.join("mods")
    }

    /// Java shader packs (zips or folders) the player drops in to pick from the Shaders tab.
    #[must_use]
    pub fn shader_packs_dir(&self) -> PathBuf {
        self.user_data_root.join("shaderpacks")
    }

    /// Translated shader-pack programs reused across launches; safe to delete.
    #[must_use]
    pub fn shader_pack_cache_dir(&self) -> PathBuf {
        self.user_data_root.join("cache/shaderpacks")
    }

    #[must_use]
    pub fn resource_pack_cache_dir(&self) -> PathBuf {
        self.user_data_root.join("resource-packs/v1/objects")
    }

    /// Verified chunk blobs reused across launches; safe to delete while the client is closed.
    #[must_use]
    pub fn client_blob_cache_file(&self) -> PathBuf {
        self.user_data_root.join("cache/client-blobs-v1.redb")
    }

    /// Far-terrain summaries of terrain servers sent, per hashed server; safe to delete.
    #[must_use]
    pub fn far_terrain_cache_dir(&self) -> PathBuf {
        self.user_data_root.join("far-terrain/v1")
    }

    /// Join-time server-pack archives and compilations reused across launches; safe to delete.
    #[must_use]
    pub fn compiled_pack_cache_dir(&self) -> PathBuf {
        self.user_data_root.join("resource-packs/compiled")
    }

    #[must_use]
    pub fn local_worlds_dir(&self) -> PathBuf {
        self.user_data_root.join("worlds")
    }

    /// Downloaded Bedrock Dedicated Server builds, beside the worlds they host.
    #[must_use]
    pub fn bds_dir(&self) -> PathBuf {
        self.user_data_root.join("bds")
    }

    #[must_use]
    pub fn server_file(&self) -> PathBuf {
        self.user_config_root.join("servers.json")
    }

    #[must_use]
    /// Saved opt-in graphics settings.
    pub fn graphics_file(&self) -> PathBuf {
        self.user_config_root.join("graphics.json")
    }

    #[must_use]
    pub fn catalog_file(&self, process_id: u32) -> PathBuf {
        self.transient_runtime_root
            .join(format!("catalog-{process_id}.json"))
    }

    /// Root holding per-session runtime directories and endpoint artifacts.
    #[must_use]
    pub fn transient_runtime_root(&self) -> &Path {
        &self.transient_runtime_root
    }

    #[must_use]
    pub fn direct_socket_dir(&self, process_id: u32) -> PathBuf {
        self.transient_runtime_root
            .join(format!("direct-{process_id}"))
    }

    /// Account-core incarnations have separate endpoints from each other and game sessions.
    #[must_use]
    pub fn account_socket_dir(&self, pid: u32, incarnation: u64) -> PathBuf {
        self.transient_runtime_root
            .join(format!("account-{pid}-{incarnation}"))
    }

    #[must_use]
    pub fn connect_socket_dir(&self, process_id: u32, generation: u64) -> PathBuf {
        self.transient_runtime_root
            .join(format!("connect-{process_id}-{generation}"))
    }
}

/// Cargo runs tests from its build directory, which `build-dir` puts outside the
/// checkout; such a binary resolves as if it ran from its workspace's `target/debug`.
fn cargo_artifact_path(executable: PathBuf, manifest_dir: Option<&Path>) -> PathBuf {
    if development_root(&executable).is_some() {
        return executable;
    }
    let workspace = manifest_dir.and_then(|manifest| {
        manifest
            .ancestors()
            .find(|directory| directory.join("Cargo.lock").is_file())
    });
    match (workspace, executable.file_name()) {
        (Some(root), Some(name)) => root.join("target/debug/deps").join(name),
        _ => executable,
    }
}

fn development_root(executable: &Path) -> Option<(PathBuf, PathBuf)> {
    for ancestor in executable.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == "target") {
            let profile = executable
                .strip_prefix(ancestor)
                .ok()?
                .components()
                .next()?
                .as_os_str();
            // Cargo profile directories this repository builds the client into.
            if !["debug", "release", "play"]
                .iter()
                .any(|known| profile == *known)
            {
                continue;
            }
            let root = ancestor.parent()?.to_owned();
            let mut binary_dir = executable.parent()?.to_owned();
            if binary_dir.file_name().is_some_and(|name| name == "deps") {
                binary_dir.pop();
            }
            return Some((root, binary_dir));
        }
    }
    None
}

fn isolated_user_roots(
    platform: Platform,
    environment: &InstallEnvironment,
) -> Result<Option<(PathBuf, PathBuf, PathBuf)>, LayoutError> {
    let Some(root) = environment.user_root.as_deref() else {
        return Ok(None);
    };
    let name = match platform {
        Platform::Windows => "Windows",
        Platform::Linux => "Linux",
        Platform::MacOs => "macOS",
    };
    require_absolute(root, platform, "CINNABAR_USER_ROOT", name)?;
    Ok(Some((
        root.join("config"),
        root.join("data"),
        root.join("run"),
    )))
}

fn user_roots(
    platform: Platform,
    environment: &InstallEnvironment,
) -> Result<(PathBuf, PathBuf, PathBuf), LayoutError> {
    if let Some(roots) = isolated_user_roots(platform, environment)? {
        return Ok(roots);
    }
    match platform {
        Platform::Windows => {
            let base = environment
                .local_app_data
                .as_deref()
                .filter(|path| !path.as_os_str().is_empty())
                .ok_or(LayoutError::MissingLocalAppData)?;
            require_absolute(base, Platform::Windows, "LOCALAPPDATA", "Windows")?;
            let root = base.join(APP_DIR);
            Ok((root.clone(), root.clone(), root.join("run")))
        }
        Platform::MacOs => {
            let home = required_home(environment, "macOS")?;
            require_absolute(home, Platform::MacOs, "HOME", "macOS")?;
            let root = home.join("Library/Application Support").join(APP_DIR);
            Ok((root.clone(), root.clone(), root.join("run")))
        }
        Platform::Linux => {
            let config_base = environment
                .xdg_config_home
                .as_deref()
                .filter(|path| absolute_for(Platform::Linux, path))
                .map(Path::to_owned);
            let data_base = environment
                .xdg_data_home
                .as_deref()
                .filter(|path| absolute_for(Platform::Linux, path))
                .map(Path::to_owned);
            let home = if config_base.is_none() || data_base.is_none() {
                let home = required_home(environment, "Linux")?;
                require_absolute(home, Platform::Linux, "HOME", "Linux")?;
                Some(home)
            } else {
                None
            };
            let config = config_base
                .unwrap_or_else(|| {
                    home.expect("home required for config fallback")
                        .join(".config")
                })
                .join("cinnabar");
            let data = data_base
                .unwrap_or_else(|| {
                    home.expect("home required for data fallback")
                        .join(".local/share")
                })
                .join("cinnabar");
            let runtime = environment
                .xdg_runtime_dir
                .as_ref()
                .filter(|path| absolute_for(Platform::Linux, path))
                .map_or_else(|| data.join("run"), |root| root.join("cinnabar"));
            Ok((config, data, runtime))
        }
    }
}

fn required_home<'a>(
    environment: &'a InstallEnvironment,
    platform: &'static str,
) -> Result<&'a Path, LayoutError> {
    environment
        .home
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(LayoutError::MissingUserHome { platform })
}

fn require_absolute(
    path: &Path,
    platform: Platform,
    variable: &'static str,
    platform_name: &'static str,
) -> Result<(), LayoutError> {
    if absolute_for(platform, path) {
        Ok(())
    } else {
        Err(LayoutError::InvalidUserRoot {
            variable,
            platform: platform_name,
        })
    }
}

fn absolute_for(platform: Platform, path: &Path) -> bool {
    let value = path.as_os_str().to_string_lossy();
    if value.is_empty() {
        return false;
    }
    match platform {
        Platform::Windows => {
            let bytes = value.as_bytes();
            (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
                || value.starts_with("\\\\")
                || value.starts_with("//")
        }
        Platform::Linux | Platform::MacOs => value.starts_with('/'),
    }
}

const fn core_filename(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "bedrock-core.exe",
        Platform::Linux | Platform::MacOs => "bedrock-core",
    }
}

const fn home_variable(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "USERPROFILE",
        Platform::Linux | Platform::MacOs => "HOME",
    }
}

const fn current_platform() -> Platform {
    #[cfg(target_os = "windows")]
    return Platform::Windows;
    #[cfg(target_os = "linux")]
    return Platform::Linux;
    #[cfg(target_os = "macos")]
    return Platform::MacOs;
    #[allow(unreachable_code)]
    Platform::Linux
}

#[cfg(test)]
mod tests;
