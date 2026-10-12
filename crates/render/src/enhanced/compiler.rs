//! Select a compiler that can optimize the Enhanced shaders without overflowing its stack.

use bevy::render::settings::WgpuSettings;
use std::path::{Path, PathBuf};
use wgpu::{Backends, Dx12Compiler};

/// Enhanced-enabled Windows builds use DXC for DX12, or Vulkan when DXC is absent.
/// This also covers switching to Enhanced after renderer initialization.
pub fn configure_enhanced_shader_compiler(settings: &mut WgpuSettings) {
    let backends = settings.backends.unwrap_or(Backends::all());
    if !backends.contains(Backends::DX12)
        || matches!(settings.dx12_shader_compiler, Dx12Compiler::StaticDxc)
    {
        return;
    }
    let mut directories = Vec::new();
    if let Dx12Compiler::DynamicDxc { dxc_path, .. } = &settings.dx12_shader_compiler {
        if let Ok(path) = std::fs::canonicalize(dxc_path) {
            if let Some(path) = path.to_str() {
                let path = path.to_owned();
                if let Dx12Compiler::DynamicDxc { dxc_path, .. } =
                    &mut settings.dx12_shader_compiler
                {
                    *dxc_path = path;
                }
                return;
            }
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            directories.push(parent.to_owned());
        }
    }
    if let Ok(directory) = std::env::current_dir() {
        directories.push(directory);
    }
    let mut sdk_bins = Vec::new();
    if let Some(root) = std::env::var_os("WindowsSdkDir") {
        sdk_bins.push(PathBuf::from(root).join("bin"));
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(root) = std::env::var_os(variable) {
            sdk_bins.push(PathBuf::from(root).join("Windows Kits/10/bin"));
        }
    }
    for bin in sdk_bins {
        directories.extend(sdk_compiler_directories(&bin, sdk_architecture()));
    }
    if let Some(root) = std::env::var_os("SystemRoot") {
        directories.push(PathBuf::from(root).join("System32"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&paths));
    }
    let compiler = find_compiler(&directories);
    if let Some(path) = &compiler {
        bevy::log::info!("Enhanced DX12 shader compiler: {}", path.display());
    } else {
        bevy::log::warn!("Enhanced DX12 requires DXC; no installed compiler found, using Vulkan");
    }
    apply_compiler(settings, compiler);
}

fn apply_compiler(settings: &mut WgpuSettings, path: Option<PathBuf>) {
    if let Some(path) = path.and_then(|path| path.to_str().map(str::to_owned)) {
        let mut compiler = Dx12Compiler::default_dynamic_dxc();
        if let Dx12Compiler::DynamicDxc { dxc_path, .. } = &mut compiler {
            *dxc_path = path;
        }
        settings.dx12_shader_compiler = compiler;
    } else {
        // FXC crashes while optimizing the combined cloud/AO fragment shader.
        let mut backends = settings.backends.unwrap_or(Backends::all());
        backends.remove(Backends::DX12);
        if backends.is_empty() {
            backends = Backends::VULKAN;
        }
        settings.backends = Some(backends);
    }
}

fn find_compiler(directories: &[PathBuf]) -> Option<PathBuf> {
    let Dx12Compiler::DynamicDxc { dxc_path, .. } = Dx12Compiler::default_dynamic_dxc() else {
        unreachable!()
    };
    directories.iter().find_map(|directory| {
        let path = directory.join(&dxc_path);
        path.is_file()
            .then(|| std::fs::canonicalize(path).ok())
            .flatten()
            .filter(|path| path.to_str().is_some())
    })
}

fn sdk_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        architecture => architecture,
    }
}

fn sdk_compiler_directories(bin: &Path, architecture: &str) -> Vec<PathBuf> {
    let mut versions = std::fs::read_dir(bin)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name
                .to_str()?
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (version.len() == 4).then(|| (version, entry.path().join(architecture)))
        })
        .collect::<Vec<_>>();
    versions.sort_unstable_by(|left, right| right.0.cmp(&left.0));
    let mut directories = versions
        .into_iter()
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    directories.push(bin.join(architecture));
    directories
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "cinnabar-dxc-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn compiler(&self, directory: &str) -> PathBuf {
            let path = self.0.join(directory);
            std::fs::create_dir_all(&path).unwrap();
            let Dx12Compiler::DynamicDxc { dxc_path, .. } = Dx12Compiler::default_dynamic_dxc()
            else {
                unreachable!()
            };
            let path = path.join(dxc_path);
            std::fs::write(&path, []).unwrap();
            std::fs::canonicalize(path).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn installed_sdk_selects_newest_available_compiler_for_native_architecture() {
        let fixture = Fixture::new();
        fixture.compiler("10.0.9000.0/x64");
        let newest = fixture.compiler("10.0.26100.0/x64");
        fixture.compiler("10.0.30000.0/arm64");
        let paths = sdk_compiler_directories(&fixture.0, "x64");
        assert_eq!(find_compiler(&paths), Some(newest.clone()));
        let mut settings = WgpuSettings {
            backends: Some(Backends::DX12),
            dx12_shader_compiler: Dx12Compiler::Fxc,
            ..Default::default()
        };
        apply_compiler(&mut settings, Some(newest.clone()));
        assert!(matches!(settings.dx12_shader_compiler,
            Dx12Compiler::DynamicDxc { dxc_path, .. } if Path::new(&dxc_path) == newest));
        assert_eq!(settings.backends, Some(Backends::DX12));
    }

    #[test]
    fn sidecar_precedes_sdk_compiler() {
        let fixture = Fixture::new();
        let sidecar = fixture.compiler("executable");
        fixture.compiler("sdk/10.0.26100.0/x64");
        let mut paths = vec![fixture.0.join("executable")];
        paths.extend(sdk_compiler_directories(&fixture.0.join("sdk"), "x64"));
        assert_eq!(find_compiler(&paths), Some(sidecar));
    }

    #[test]
    fn missing_dxc_excludes_crashing_fxc_backend() {
        for backends in [Backends::DX12, Backends::DX12 | Backends::VULKAN] {
            let mut settings = WgpuSettings {
                backends: Some(backends),
                dx12_shader_compiler: Dx12Compiler::Fxc,
                ..Default::default()
            };
            apply_compiler(&mut settings, None);
            assert_eq!(settings.backends, Some(Backends::VULKAN));
        }
    }

    #[test]
    fn other_backends_keep_their_compiler_configuration() {
        let mut settings = WgpuSettings {
            backends: Some(Backends::VULKAN),
            dx12_shader_compiler: Dx12Compiler::Fxc,
            ..Default::default()
        };
        configure_enhanced_shader_compiler(&mut settings);
        assert_eq!(settings.backends, Some(Backends::VULKAN));
        assert!(matches!(settings.dx12_shader_compiler, Dx12Compiler::Fxc));
    }
}
