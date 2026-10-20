//! Device requirements shared by the native and browser application hosts.

use bevy::render::settings::WgpuSettings;

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
    // Shipping installs use FXC; a DLL in the launch directory must not change that.
    settings.dx12_shader_compiler = wgpu::Dx12Compiler::Fxc;
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
}
