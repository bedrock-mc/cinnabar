use std::path::{Path, PathBuf};
use thiserror::Error;

const APP_DIR: &str = "Cinnabar";

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
            return Ok(Self {
                resource_root: local.clone(),
                compiled_assets: local.join("assets/compiled"),
                physics_registry: local.join("assets/block-physics-v1001.bin"),
                core_executable: binary_dir.join(core_filename(platform)),
                user_config_root: local.join("cinnabar"),
                user_data_root: local.clone(),
                runtime_root: local.join("run"),
                transient_runtime_root: local.join("cinnabar"),
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
            physics_registry: resource_root.join("assets/block-physics-v1001.bin"),
            resource_root,
            core_executable,
            user_config_root,
            user_data_root,
            transient_runtime_root: runtime_root.clone(),
            runtime_root,
        })
    }

    pub fn discover() -> Result<Self, LayoutError> {
        let platform = current_platform();
        let home = std::env::var_os(home_variable(platform)).map(PathBuf::from);
        Self::resolve(
            platform,
            &InstallEnvironment {
                executable: std::env::current_exe().map_err(|_| LayoutError::MissingExecutable)?,
                home,
                local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
                xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
                xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
                xdg_runtime_dir: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            },
        )
    }

    #[must_use]
    pub fn world_assets(&self) -> PathBuf {
        self.compiled_assets.join("vanilla-v1001.mcbea")
    }

    #[must_use]
    pub fn auth_cache(&self) -> PathBuf {
        self.user_data_root.join("auth/microsoft-token.json")
    }

    #[must_use]
    pub fn server_file(&self) -> PathBuf {
        self.user_config_root.join("servers.json")
    }

    #[must_use]
    pub fn catalog_file(&self, process_id: u32) -> PathBuf {
        self.transient_runtime_root
            .join(format!("catalog-{process_id}.json"))
    }

    #[must_use]
    pub fn direct_socket_dir(&self, process_id: u32) -> PathBuf {
        self.transient_runtime_root
            .join(format!("direct-{process_id}"))
    }

    #[must_use]
    pub fn connect_socket_dir(&self, process_id: u32, generation: u64) -> PathBuf {
        self.transient_runtime_root
            .join(format!("connect-{process_id}-{generation}"))
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
            if profile != "debug" && profile != "release" {
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

fn user_roots(
    platform: Platform,
    environment: &InstallEnvironment,
) -> Result<(PathBuf, PathBuf, PathBuf), LayoutError> {
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
mod tests {
    use super::{InstallEnvironment, InstallLayout, LayoutError, Platform};
    use crate::asset_startup::{AssetPathSource, select_asset_path_with_default};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn environment(executable: &str, home: &str) -> InstallEnvironment {
        InstallEnvironment {
            executable: PathBuf::from(executable),
            home: Some(PathBuf::from(home)),
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        }
    }

    #[test]
    fn resolves_development_layout_from_target_ancestor() {
        let layout = InstallLayout::resolve(
            Platform::Linux,
            &environment("/work/cinnabar/target/debug/deps/client-test", "/home/dev"),
        )
        .unwrap();
        assert_eq!(
            layout.world_assets(),
            PathBuf::from("/work/cinnabar/.local/assets/compiled/vanilla-v1001.mcbea")
        );
        assert_eq!(
            layout.physics_registry,
            PathBuf::from("/work/cinnabar/.local/assets/block-physics-v1001.bin")
        );
        assert_eq!(
            layout.runtime_root,
            PathBuf::from("/work/cinnabar/.local/run")
        );
        assert_eq!(
            layout.direct_socket_dir(41),
            PathBuf::from("/work/cinnabar/.local/cinnabar/direct-41")
        );
        assert_eq!(
            layout.connect_socket_dir(41, 3),
            PathBuf::from("/work/cinnabar/.local/cinnabar/connect-41-3")
        );
        assert_eq!(
            layout.catalog_file(41),
            PathBuf::from("/work/cinnabar/.local/cinnabar/catalog-41.json")
        );
    }

    #[test]
    fn resolves_windows_app_layout_and_user_roots() {
        let mut env = environment(
            "C:/Program Files/Cinnabar/bedrock-client.exe",
            "C:/Users/dev",
        );
        env.local_app_data = Some(PathBuf::from("C:/Users/dev/AppData/Local"));
        let layout = InstallLayout::resolve(Platform::Windows, &env).unwrap();
        assert_eq!(
            layout.compiled_assets,
            PathBuf::from("C:/Program Files/Cinnabar/resources/assets")
        );
        assert_eq!(
            layout.auth_cache(),
            PathBuf::from("C:/Users/dev/AppData/Local/Cinnabar/auth/microsoft-token.json")
        );
    }

    #[test]
    fn resolves_linux_xdg_layout() {
        let mut env = environment("/opt/cinnabar/bin/bedrock-client", "/home/dev");
        env.xdg_config_home = Some(PathBuf::from("/cfg"));
        env.xdg_data_home = Some(PathBuf::from("/data"));
        env.xdg_runtime_dir = Some(PathBuf::from("/run/user/1000"));
        let layout = InstallLayout::resolve(Platform::Linux, &env).unwrap();
        assert_eq!(
            layout.compiled_assets,
            PathBuf::from("/opt/cinnabar/share/cinnabar/assets")
        );
        assert_eq!(
            layout.server_file(),
            PathBuf::from("/cfg/cinnabar/servers.json")
        );
        assert_eq!(
            layout.runtime_root,
            PathBuf::from("/run/user/1000/cinnabar")
        );
    }

    #[test]
    fn resolves_macos_app_bundle() {
        let layout = InstallLayout::resolve(
            Platform::MacOs,
            &environment(
                "/Applications/Cinnabar.app/Contents/MacOS/bedrock-client",
                "/Users/dev",
            ),
        )
        .unwrap();
        assert_eq!(
            layout.compiled_assets,
            PathBuf::from("/Applications/Cinnabar.app/Contents/Resources/assets")
        );
        assert_eq!(
            layout.core_executable,
            PathBuf::from("/Applications/Cinnabar.app/Contents/MacOS/bedrock-core")
        );
    }

    #[test]
    fn explicit_asset_sources_precede_the_layout_default() {
        let default = PathBuf::from("/bundle/resources/assets/vanilla-v1001.mcbea");
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

    #[test]
    fn installed_layouts_fail_without_required_identity_or_user_roots() {
        let mut linux = environment("/opt/cinnabar/bin/bedrock-client", "/home/dev");
        linux.home = None;
        assert_eq!(
            InstallLayout::resolve(Platform::Linux, &linux),
            Err(LayoutError::MissingUserHome { platform: "Linux" })
        );

        let windows = environment("C:/Cinnabar/bedrock-client.exe", "C:/Users/dev");
        assert_eq!(
            InstallLayout::resolve(Platform::Windows, &windows),
            Err(LayoutError::MissingLocalAppData)
        );

        let mut missing_executable = linux;
        missing_executable.executable = PathBuf::new();
        assert_eq!(
            InstallLayout::resolve(Platform::Linux, &missing_executable),
            Err(LayoutError::MissingExecutable)
        );
    }

    #[test]
    fn installed_layouts_never_accept_relative_user_roots() {
        let mut windows = environment("C:/Cinnabar/bedrock-client.exe", "C:/Users/dev");
        windows.local_app_data = Some(PathBuf::from("relative/local"));
        assert!(matches!(
            InstallLayout::resolve(Platform::Windows, &windows),
            Err(LayoutError::InvalidUserRoot {
                variable: "LOCALAPPDATA",
                ..
            })
        ));

        let mac = environment(
            "/Applications/Cinnabar.app/Contents/MacOS/bedrock-client",
            "relative/home",
        );
        assert!(matches!(
            InstallLayout::resolve(Platform::MacOs, &mac),
            Err(LayoutError::InvalidUserRoot {
                variable: "HOME",
                ..
            })
        ));

        let mut linux = environment("/opt/cinnabar/bin/bedrock-client", "/home/dev");
        linux.xdg_config_home = Some(PathBuf::from("relative/config"));
        linux.xdg_data_home = Some(PathBuf::new());
        linux.xdg_runtime_dir = Some(PathBuf::from("relative/run"));
        let layout = InstallLayout::resolve(Platform::Linux, &linux).unwrap();
        assert_eq!(
            layout.user_config_root,
            PathBuf::from("/home/dev/.config/cinnabar")
        );
        assert_eq!(
            layout.user_data_root,
            PathBuf::from("/home/dev/.local/share/cinnabar")
        );
        assert_eq!(
            layout.runtime_root,
            PathBuf::from("/home/dev/.local/share/cinnabar/run")
        );
    }

    #[test]
    fn absolute_linux_xdg_roots_do_not_require_home() {
        let mut linux = environment("/opt/cinnabar/bin/bedrock-client", "/unused");
        linux.home = None;
        linux.xdg_config_home = Some(PathBuf::from("/cfg"));
        linux.xdg_data_home = Some(PathBuf::from("/data"));
        linux.xdg_runtime_dir = Some(PathBuf::from("/run/user/1000"));
        let layout = InstallLayout::resolve(Platform::Linux, &linux).unwrap();
        assert_eq!(layout.user_config_root, PathBuf::from("/cfg/cinnabar"));
        assert_eq!(layout.user_data_root, PathBuf::from("/data/cinnabar"));
        assert_eq!(
            layout.runtime_root,
            PathBuf::from("/run/user/1000/cinnabar")
        );
    }

    #[test]
    fn malformed_macos_path_never_guesses_an_app_bundle() {
        let mac_environment = environment("/opt/cinnabar/bin/bedrock-client", "/Users/dev");
        assert!(matches!(
            InstallLayout::resolve(Platform::MacOs, &mac_environment),
            Err(LayoutError::InvalidMacOsBundle(_))
        ));
        let linux_environment = environment("/opt/cinnabar/bedrock-client", "/home/dev");
        assert!(matches!(
            InstallLayout::resolve(Platform::Linux, &linux_environment),
            Err(LayoutError::InvalidLinuxLayout(_))
        ));
    }
}
