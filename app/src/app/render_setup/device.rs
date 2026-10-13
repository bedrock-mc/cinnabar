//! Instance, adapter and device requirements for renderer startup.

use anyhow::{Context, Result};
use bevy::render::settings::{WgpuSettings, WgpuSettingsPriority};
use launcher::graphics_backend::{Adapter, Backend};
use std::io::Write;

/// Applies renderer limits and compiler policy to Bevy's defaults.
pub(super) fn configure_render_settings(settings: &mut WgpuSettings) {
    // Shipping installs use FXC; a DLL in the launch directory must not change that.
    settings.dx12_shader_compiler = wgpu::Dx12Compiler::Fxc;
    settings.power_preference = wgpu::PowerPreference::HighPerformance;
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(render::required_vertex_storage_buffers());
}

/// Converts the policy's backend to wgpu's flags without keeping a separate order table.
fn flags(backend: Backend) -> wgpu::Backends {
    match backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::Dx12 => wgpu::Backends::DX12,
        Backend::Gl => wgpu::Backends::GL,
        Backend::Metal => wgpu::Backends::METAL,
    }
}

/// Filters the single policy order by the compiled or explicitly requested backend mask.
pub(super) fn backends(mask: wgpu::Backends) -> Vec<Backend> {
    Backend::ALL
        .iter()
        .copied()
        .filter(|backend| mask.contains(flags(*backend)))
        .collect()
}

/// Copies diagnostic identity from the selected adapter rather than inferring a GPU from its name.
pub(super) fn identity(adapter: &wgpu::Adapter) -> Adapter {
    let info = adapter.get_info();
    let driver_version = info.driver.clone();
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    let driver_version = if info.backend == wgpu::Backend::Vulkan {
        // SAFETY: This read-only properties query borrows the live adapter and its instance.
        if let Some(native) = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() } {
            let properties = unsafe {
                native
                    .shared_instance()
                    .raw_instance()
                    .get_physical_device_properties(native.raw_physical_device())
            };
            if cfg!(windows) && info.vendor == launcher::graphics_backend::INTEL_PCI_VENDOR {
                format!(
                    "{}.{}",
                    properties.driver_version >> 14,
                    properties.driver_version & 0x3fff
                )
            } else {
                properties.driver_version.to_string()
            }
        } else {
            driver_version
        }
    } else {
        driver_version
    };
    Adapter {
        name: info.name.clone(),
        vendor: info.vendor,
        device: info.device,
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
        driver_version,
        integrated: info.device_type == wgpu::DeviceType::IntegratedGpu,
    }
}

/// Flushes each startup boundary to client.log before executing the next native call.
pub(super) fn log(message: std::fmt::Arguments<'_>) {
    let _ = writeln!(diagnostics::console::stderr(), "{message}");
    diagnostics::console::flush();
}

/// Applies the existing automatic feature selection and constrained device limits.
fn device_requirements(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> (wgpu::Features, wgpu::Limits) {
    let mut features = wgpu::Features::empty();
    let mut limits = settings.limits.clone();
    if matches!(settings.priority, WgpuSettingsPriority::Functionality) {
        features = adapter.features();
        limits = adapter.limits();
        if adapter.get_info().device_type == wgpu::DeviceType::DiscreteGpu {
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }
    }
    if let Some(disabled) = settings.disabled_features {
        features.remove(disabled);
    }
    features |= settings.features;
    if let Some(constrained) = &settings.constrained_limits {
        limits = limits.or_worse_values_from(constrained);
    }
    (features, limits)
}

/// Builds one backend with the shipping compiler and display policy.
pub(super) fn instance(
    backend: Backend,
    settings: &WgpuSettings,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor {
        backends: flags(backend),
        flags: settings.instance_flags,
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        backend_options: Default::default(),
        display,
    };
    descriptor.backend_options.dx12.shader_compiler = settings.dx12_shader_compiler.clone();
    descriptor.backend_options.dx12.presentation_system =
        wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
    descriptor.backend_options.dx12.latency_waitable_object =
        wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
    descriptor.backend_options.gl.gles_minor_version = settings.gles3_minor_version;
    wgpu::Instance::new(descriptor)
}

/// Uses the renderer's requested power preference and any explicit adapter-name override.
pub(super) async fn select_adapter(
    instance: &wgpu::Instance,
    settings: &WgpuSettings,
) -> Result<wgpu::Adapter> {
    let desired_name = std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .or_else(|| settings.adapter_name.clone());
    let force_fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
        .map_or(settings.force_fallback_adapter, |value| {
            !(value.is_empty() || value == "0" || value == "false")
        });
    if let Some(name) = desired_name.filter(|_| !force_fallback) {
        let adapters = instance
            .enumerate_adapters(wgpu::Instance::enabled_backend_features())
            .await;
        if let Some(adapter) = adapters.into_iter().find(|adapter| {
            adapter
                .get_info()
                .name
                .to_lowercase()
                .contains(&name.to_lowercase())
        }) {
            return Ok(adapter);
        }
    }
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: settings.power_preference,
            force_fallback_adapter: force_fallback,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .context("select high-performance renderer adapter")
}

/// Opens a device only when it supports the renderer's terrain and storage requirements.
pub(super) async fn request_device(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<(wgpu::Device, wgpu::Queue)> {
    validate_capabilities(
        adapter.get_downlevel_capabilities().flags,
        adapter.limits().max_storage_buffers_per_shader_stage,
    )?;
    let (features, limits) = device_requirements(adapter, settings);
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: settings.device_label.as_deref(),
            required_features: features,
            required_limits: limits,
            // SAFETY: These are the same experimental features enabled by Bevy's renderer.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            memory_hints: settings.memory_hints.clone(),
            trace: wgpu::Trace::Off,
        })
        .await
        .context("create renderer device")
}

/// Rejects adapters that would accept startup but silently omit terrain draws.
fn validate_capabilities(flags: wgpu::DownlevelFlags, storage_buffers: u32) -> Result<()> {
    anyhow::ensure!(
        flags.contains(wgpu::DownlevelFlags::BASE_VERTEX),
        "GPU lacks BASE_VERTEX required by the terrain renderer"
    );
    anyhow::ensure!(
        flags.contains(wgpu::DownlevelFlags::VERTEX_STORAGE),
        "GPU lacks vertex-stage storage buffers required by the renderer"
    );
    anyhow::ensure!(
        storage_buffers >= render::required_vertex_storage_buffers(),
        "GPU has too few vertex-stage storage buffers for the renderer"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_must_support_terrain_base_vertex_and_storage() {
        let storage = wgpu::DownlevelFlags::VERTEX_STORAGE;
        let base_vertex = wgpu::DownlevelFlags::BASE_VERTEX;
        let required = render::required_vertex_storage_buffers();
        assert!(validate_capabilities(storage, required).is_err());
        assert!(validate_capabilities(base_vertex, required).is_err());
        assert!(validate_capabilities(storage | base_vertex, required - 1).is_err());
        assert!(validate_capabilities(storage | base_vertex, required).is_ok());
    }

    #[test]
    fn configured_storage_limits_cover_the_renderer_without_lowering_existing_limits() {
        let required = render::required_vertex_storage_buffers();
        for initial in [0, required, required + 1] {
            let mut settings = WgpuSettings::default();
            settings.limits.max_storage_buffers_per_shader_stage = initial;
            configure_render_settings(&mut settings);
            assert_eq!(
                settings.limits.max_storage_buffers_per_shader_stage,
                initial.max(required)
            );
        }
    }
}
