//! Shared instance, adapter and device requirements for probes and the real renderer.

use anyhow::{Context, Result};
use bevy::render::settings::{WgpuSettings, WgpuSettingsPriority};
use launcher::graphics_backend::{Adapter, Backend};
use std::io::Write;

/// Applies renderer limits and compiler policy to Bevy's defaults.
pub(crate) fn configure_render_settings(settings: &mut WgpuSettings) {
    // Shipping installs use FXC; a DLL in the launch directory must not change that.
    settings.dx12_shader_compiler = wgpu::Dx12Compiler::Fxc;
    settings.power_preference = wgpu::PowerPreference::HighPerformance;
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(render::required_vertex_storage_buffers());
}

/// Converts the policy's backend to wgpu's flags without keeping a separate order table.
pub(crate) fn flags(backend: Backend) -> wgpu::Backends {
    match backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::Dx12 => wgpu::Backends::DX12,
        Backend::Gl => wgpu::Backends::GL,
        Backend::Metal => wgpu::Backends::METAL,
    }
}

/// Filters the single policy order by the compiled or explicitly requested backend mask.
pub(crate) fn backends(mask: wgpu::Backends) -> Vec<Backend> {
    Backend::ALL
        .iter()
        .copied()
        .filter(|backend| mask.contains(flags(*backend)))
        .collect()
}

/// Copies diagnostic identity from the selected adapter rather than inferring a GPU from its name.
pub(crate) fn identity(adapter: &wgpu::Adapter) -> Adapter {
    let info = adapter.get_info();
    let mut identity = Adapter {
        name: info.name.clone(),
        vendor: info.vendor,
        device: info.device,
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
        driver_version: info.driver.clone(),
        integrated: info.device_type == wgpu::DeviceType::IntegratedGpu,
    };
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    if info.backend == wgpu::Backend::Vulkan {
        // SAFETY: This read-only properties query borrows the live adapter and its instance.
        if let Some(native) = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() } {
            let properties = unsafe {
                native
                    .shared_instance()
                    .raw_instance()
                    .get_physical_device_properties(native.raw_physical_device())
            };
            identity.driver_version =
                if cfg!(windows) && info.vendor == launcher::graphics_backend::INTEL_PCI_VENDOR {
                    format!(
                        "{}.{}",
                        properties.driver_version >> 14,
                        properties.driver_version & 0x3fff
                    )
                } else {
                    properties.driver_version.to_string()
                };
        }
    }
    #[cfg(target_os = "macos")]
    if info.backend == wgpu::Backend::Metal {
        static OS_BUILD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let build = OS_BUILD.get_or_init(|| {
            std::process::Command::new("/usr/bin/sw_vers")
                .arg("-buildVersion")
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
                .unwrap_or_default()
        });
        identity.driver_version.clone_from(build);
    }
    identity
}

/// Flushes each startup boundary to client.log before executing the next native call.
pub(crate) fn log(message: std::fmt::Arguments<'_>) {
    let _ = writeln!(diagnostics::console::stderr(), "{message}");
    diagnostics::console::flush();
}

/// Applies the existing automatic feature selection and constrained device limits.
pub(crate) fn device_requirements(
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
pub(crate) fn instance(
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
pub(crate) async fn select_adapter(
    instance: &wgpu::Instance,
    settings: &WgpuSettings,
    key: Option<&launcher::graphics_backend::AdapterKey>,
) -> Result<wgpu::Adapter> {
    let desired_name = std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .or_else(|| settings.adapter_name.clone());
    let force_fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
        .map_or(settings.force_fallback_adapter, |value| {
            !(value.is_empty() || value == "0" || value == "false")
        });
    if key.is_some() || (desired_name.is_some() && !force_fallback) {
        let adapters = instance
            .enumerate_adapters(wgpu::Instance::enabled_backend_features())
            .await;
        if let Some(adapter) = adapters.into_iter().find(|adapter| {
            let info = adapter.get_info();
            key.map_or_else(
                || {
                    desired_name
                        .as_ref()
                        .is_some_and(|name| info.name.to_lowercase().contains(&name.to_lowercase()))
                },
                |key| identity(adapter).key() == *key,
            )
        }) {
            return Ok(adapter);
        }
        if key.is_some() {
            anyhow::bail!("selected GPU is unavailable on this backend");
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

/// Opens a device with the same real feature and limit requirements in both processes.
pub(crate) async fn request_device(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<(wgpu::Device, wgpu::Queue)> {
    anyhow::ensure!(
        adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::VERTEX_STORAGE),
        "GPU lacks vertex-stage storage buffers required by the renderer"
    );
    anyhow::ensure!(
        adapter.limits().max_storage_buffers_per_shader_stage
            >= render::required_vertex_storage_buffers(),
        "GPU has too few vertex-stage storage buffers for the renderer"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

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
