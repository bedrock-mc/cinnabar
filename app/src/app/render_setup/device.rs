//! Instance, adapter and device requirements for renderer startup.

use anyhow::{Context, Result};
use bevy::render::settings::{WgpuSettings, WgpuSettingsPriority};
use launcher::graphics_backend::{Adapter, Backend};

/// Converts the policy's backend to wgpu's flags without keeping a separate order table.
pub(super) fn flags(backend: Backend) -> wgpu::Backends {
    match backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::Dx12 => wgpu::Backends::DX12,
        Backend::Gl => wgpu::Backends::GL,
        Backend::Metal => wgpu::Backends::METAL,
    }
}

/// Filters the single policy order by the compiled or explicitly requested backend mask.
pub(super) fn backends(mask: wgpu::Backends) -> Vec<Backend> {
    Backend::AVAILABLE
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

/// Flushes each startup boundary to the file without waiting for terminal output.
pub(super) fn log(message: std::fmt::Arguments<'_>) {
    diagnostics::console::console().write_synced(
        diagnostics::console::Stream::Stderr,
        format!("{message}\n").into_bytes(),
    );
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
    raise_window_texture_limits(&mut limits, &adapter.limits());
    if let Some(disabled) = settings.disabled_features {
        features.remove(disabled);
    }
    features |= settings.features;
    if let Some(constrained) = &settings.constrained_limits {
        limits = limits.or_worse_values_from(constrained);
    }
    (features, limits)
}

/// Window-sized targets follow the window, which can span monitors beyond the 8192 default, so
/// every priority takes the adapter's texture dimensions.
fn raise_window_texture_limits(limits: &mut wgpu::Limits, adapter: &wgpu::Limits) {
    limits.max_texture_dimension_1d = limits
        .max_texture_dimension_1d
        .max(adapter.max_texture_dimension_1d);
    limits.max_texture_dimension_2d = limits
        .max_texture_dimension_2d
        .max(adapter.max_texture_dimension_2d);
}

/// Builds exactly one backend with the given DX12 shader compiler and the shipping display policy.
pub(super) fn instance(
    backend: Backend,
    settings: &WgpuSettings,
    dx12_shader_compiler: wgpu::Dx12Compiler,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor {
        backends: flags(backend),
        flags: settings.instance_flags,
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        backend_options: Default::default(),
        display,
    };
    #[cfg(target_os = "macos")]
    if env_compat::var("CINNABAR_ASYNC_PRESENT").as_deref() != Ok("0") {
        descriptor
            .flags
            .insert(wgpu::InstanceFlags::METAL_ASYNC_WINDOWED_PRESENTATION);
    }
    descriptor.backend_options.dx12.shader_compiler = dx12_shader_compiler;
    descriptor.backend_options.dx12.presentation_system =
        wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
    descriptor.backend_options.dx12.latency_waitable_object =
        wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
    descriptor.backend_options.gl.gles_minor_version = settings.gles3_minor_version;
    wgpu::Instance::new(descriptor)
}

/// Whether a DX12 instance needs FXC instead: wgpu leaves out the DX12 backend, logging only at
/// debug level, when the DXC it was given fails to load.
pub(super) fn dxc_failed(
    backend: Backend,
    dx12_shader_compiler: &wgpu::Dx12Compiler,
    instance: &wgpu::Instance,
) -> bool {
    backend == Backend::Dx12
        && !matches!(dx12_shader_compiler, wgpu::Dx12Compiler::Fxc)
        && !has_dx12(instance)
}

#[cfg(windows)]
fn has_dx12(instance: &wgpu::Instance) -> bool {
    // SAFETY: Only the backend's presence is read; nothing is used or destroyed through it.
    unsafe { instance.as_hal::<wgpu::hal::api::Dx12>() }.is_some()
}

#[cfg(not(windows))]
fn has_dx12(_: &wgpu::Instance) -> bool {
    false
}

/// Reads the adapter-name restriction shared by all backend probes.
pub(super) fn requested_adapter_name(settings: &WgpuSettings) -> Option<String> {
    std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .or_else(|| settings.adapter_name.clone())
}

/// Preserves the environment override for software-adapter selection.
pub(super) fn force_fallback(settings: &WgpuSettings) -> bool {
    std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").map_or(settings.force_fallback_adapter, |value| {
        !(value.is_empty() || value == "0" || value == "false")
    })
}

/// Opens a device for a candidate that already passed renderer capability checks.
pub(super) async fn request_device(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<(wgpu::Device, wgpu::Queue)> {
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

/// Capabilities needed by terrain draws, texture views, material reads and MSAA shadows.
const REQUIRED_DOWNLEVEL_FLAGS: wgpu::DownlevelFlags = wgpu::DownlevelFlags::BASE_VERTEX
    .union(wgpu::DownlevelFlags::VERTEX_STORAGE)
    .union(wgpu::DownlevelFlags::FRAGMENT_STORAGE)
    .union(wgpu::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED)
    .union(wgpu::DownlevelFlags::VIEW_FORMATS)
    .union(wgpu::DownlevelFlags::NON_POWER_OF_TWO_MIPMAPPED_TEXTURES)
    .union(wgpu::DownlevelFlags::MULTISAMPLED_SHADING);

/// A candidate's capability snapshot, independent of native device creation.
pub(super) struct AdapterCapabilities {
    flags: wgpu::DownlevelFlags,
    limits: wgpu::Limits,
}

impl From<&wgpu::Adapter> for AdapterCapabilities {
    /// Reads the capabilities without opening a device.
    fn from(adapter: &wgpu::Adapter) -> Self {
        Self {
            flags: adapter.get_downlevel_capabilities().flags,
            limits: adapter.limits(),
        }
    }
}

/// Rejects candidates lacking the capabilities or limits used by the renderer.
pub(super) fn meets_requirements(adapter: impl Into<AdapterCapabilities>) -> Result<()> {
    let adapter = adapter.into();
    let missing = REQUIRED_DOWNLEVEL_FLAGS - adapter.flags;
    anyhow::ensure!(
        missing.is_empty(),
        "GPU lacks renderer capabilities: {missing:?}"
    );
    anyhow::ensure!(
        adapter.limits.max_storage_buffers_per_shader_stage
            >= render::required_vertex_storage_buffers(),
        "GPU has too few vertex-stage storage buffers for the renderer"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
