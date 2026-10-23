//! Device requirements shared by the native and browser application hosts.

use bevy::render::settings::WgpuSettings;
use std::path::PathBuf;

/// Sub-allocation block sizes for the device's memory allocator: at most 16 MiB, so a block
/// holds about one full-screen target and anything larger gets memory of its own.
const SUBALLOCATED_BLOCK_BYTES: std::ops::Range<u64> = (4 << 20)..(16 << 20);

/// Applies the renderer's storage limits, allocator policy and the platform's device policy.
pub fn configure(settings: &mut WgpuSettings) {
    #[cfg(target_arch = "wasm32")]
    {
        settings.priority = bevy::render::settings::WgpuSettingsPriority::Functionality;
        settings.backends = Some(wgpu::Backends::BROWSER_WEBGPU);
    }
    settings.dx12_shader_compiler = dx12_shader_compiler(
        wgpu::Dx12Compiler::from_env(),
        bundled_dxc().filter(|path| path.is_file()),
    );
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(crate::required_vertex_storage_buffers());
    // Bevy's default (`Performance`) sub-allocates from 128-256 MiB blocks the allocator
    // keeps while any allocation in them lives, so a block with one live buffer pins the
    // rest of its memory.
    settings.memory_hints = wgpu::MemoryHints::Manual {
        suballocated_device_memory_block_size: SUBALLOCATED_BLOCK_BYTES,
    };
}

/// Where Windows installs and development builds place DXC: beside the client executable.
/// `None` off Windows or when the executable's path is unknown.
pub fn bundled_dxc() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let executable = std::env::current_exe().ok()?;
    Some(executable.parent()?.join(dxc_library()))
}

/// The DXC library's file name, from the pin the Windows packagers install.
fn dxc_library() -> String {
    let pin: serde_json::Value =
        serde_json::from_str(include_str!("../../../packaging/windows/dxc.json"))
            .expect("the Windows DXC pin parses");
    pin["library"]
        .as_str()
        .expect("the Windows DXC pin names its library")
        .to_owned()
}

/// DXC compiles shaders much faster than FXC, and DX12 compiles every pipeline afresh at each
/// launch. Only the bundled copy is used: wgpu's and Bevy's defaults load whichever
/// `dxcompiler.dll` the launch directory or DLL search path holds, and an older copy cannot sign
/// DXIL. Without the bundled copy, or with `WGPU_DX12_COMPILER=fxc`, DX12 keeps FXC.
fn dx12_shader_compiler(
    requested: Option<wgpu::Dx12Compiler>,
    bundled: Option<PathBuf>,
) -> wgpu::Dx12Compiler {
    let bundled = bundled.and_then(|path| path.into_os_string().into_string().ok());
    match (requested, bundled) {
        (Some(wgpu::Dx12Compiler::Fxc), _) | (_, None) => wgpu::Dx12Compiler::Fxc,
        (_, Some(dxc_path)) => wgpu::Dx12Compiler::DynamicDxc { dxc_path },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_sub_allocates_from_blocks_of_at_most_16_mib() {
        let mut settings = WgpuSettings::default();
        configure(&mut settings);
        assert!(
            matches!(
                &settings.memory_hints,
                wgpu::MemoryHints::Manual { suballocated_device_memory_block_size }
                    if suballocated_device_memory_block_size.end <= 16 << 20
            ),
            "{:?}",
            settings.memory_hints
        );
    }

    #[test]
    fn dx12_uses_only_the_bundled_dxc_unless_fxc_is_requested() {
        let bundled = PathBuf::from(r"C:\Program Files\Cinnabar\dxcompiler.dll");
        for requested in [None, Some(wgpu::Dx12Compiler::default_dynamic_dxc())] {
            assert!(matches!(
                dx12_shader_compiler(requested.clone(), Some(bundled.clone())),
                wgpu::Dx12Compiler::DynamicDxc { dxc_path } if PathBuf::from(&dxc_path) == bundled
            ));
            assert!(matches!(
                dx12_shader_compiler(requested, None),
                wgpu::Dx12Compiler::Fxc
            ));
        }
        assert!(matches!(
            dx12_shader_compiler(Some(wgpu::Dx12Compiler::Fxc), Some(bundled)),
            wgpu::Dx12Compiler::Fxc
        ));
    }

    #[test]
    fn shader_compiler_does_not_inherit_a_launch_directory_dxc() {
        let mut settings = WgpuSettings {
            dx12_shader_compiler: wgpu::Dx12Compiler::default_dynamic_dxc(),
            ..Default::default()
        };
        configure(&mut settings);
        match settings.dx12_shader_compiler {
            wgpu::Dx12Compiler::Fxc => {}
            wgpu::Dx12Compiler::DynamicDxc { dxc_path } => {
                assert_eq!(Some(PathBuf::from(dxc_path)), bundled_dxc());
            }
            other => panic!("unexpected DX12 shader compiler {other:?}"),
        }
    }

    #[test]
    fn pinned_library_is_the_dxc_wgpu_loads() {
        let wgpu::Dx12Compiler::DynamicDxc { dxc_path } = wgpu::Dx12Compiler::default_dynamic_dxc()
        else {
            unreachable!("default_dynamic_dxc builds a dynamic compiler");
        };
        assert_eq!(dxc_library(), dxc_path);
    }

    #[cfg(windows)]
    #[test]
    fn bundled_dxc_sits_beside_the_executable() {
        let executable = std::env::current_exe().unwrap();
        assert_eq!(
            bundled_dxc().as_deref().and_then(std::path::Path::parent),
            executable.parent()
        );
    }
}
