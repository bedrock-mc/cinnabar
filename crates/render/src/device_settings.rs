//! Device requirements shared by the native and browser application hosts.

use bevy::render::settings::WgpuSettings;

/// Applies the renderer's storage limits and the platform's device policy.
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
}
