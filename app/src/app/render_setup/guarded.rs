//! Selects a single backend and adapter before entering native GPU device creation.

use std::{io::Write, path::Path};

use anyhow::{Context, Result, bail};
use bevy::{
    prelude::{App, Local, Res, ResMut, Resource, Update},
    render::{
        RenderPlugin,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::{RenderCreation, WgpuSettings, WgpuSettingsPriority},
    },
};
use launcher_host::renderer_startup::{Adapter, Attempt, Backend, Stage, StartupGuard};

#[derive(Resource)]
struct RendererGuard(StartupGuard);

/// Creates only supported, unblocked backends and keeps the sentinel through first presentation.
pub(super) fn render_plugin(
    app: &mut App,
    root: &Path,
    settings: WgpuSettings,
) -> Result<RenderPlugin> {
    let mut guard = StartupGuard::open(root)?;
    if let Some(previous) = &guard.interrupted {
        log(format_args!(
            "Renderer startup interrupted: {previous:?}; choosing a fallback unless WGPU_BACKEND is set"
        ));
    }
    let supported = backends(wgpu::Instance::enabled_backend_features());
    let explicit =
        std::env::var_os("WGPU_BACKEND").map(|_| backends(settings.backends.unwrap_or_default()));
    let candidates = guard.recovery.candidates(&supported, explicit.as_deref());
    if candidates.is_empty() {
        bail!(
            "No untried supported renderer backend remains. See client.log and crashes/*.json; set WGPU_BACKEND explicitly to retry after updating GPU drivers."
        );
    }
    for backend in candidates {
        let preferred = guard
            .recovery
            .working
            .as_ref()
            .filter(|working| working.backend == backend)
            .and_then(|working| working.adapter.clone());
        guard.begin(Attempt {
            backend,
            adapter: None,
            stage: Stage::EnumeratingAdapters,
            explicit: explicit.is_some(),
        })?;
        log(format_args!(
            "Renderer enumerating {backend:?}; DX12 shader compiler: {:?}",
            settings.dx12_shader_compiler
        ));
        match bevy::tasks::block_on(create_renderer(
            backend,
            preferred.as_ref(),
            &settings,
            &mut guard,
        )) {
            Ok(creation) => {
                app.insert_resource(RendererGuard(guard))
                    .add_systems(Update, mark_presented);
                return Ok(RenderPlugin {
                    render_creation: creation,
                    ..Default::default()
                });
            }
            Err(error) => {
                log(format_args!(
                    "Renderer {backend:?} initialization failed: {error:#}"
                ));
                guard.failed()?;
            }
        }
    }
    bail!(
        "All requested renderer backends failed. See client.log and crashes/*.json; WGPU_BACKEND=dx12 selects DX12 explicitly."
    )
}

/// Converts the policy's backend to wgpu's flags without keeping a separate order table.
fn flags(backend: Backend) -> wgpu::Backends {
    match backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::Dx12 => wgpu::Backends::DX12,
        Backend::Gl => wgpu::Backends::GL,
    }
}

/// Filters the single policy order by the compiled or explicitly requested backend mask.
fn backends(mask: wgpu::Backends) -> Vec<Backend> {
    Backend::ORDER
        .iter()
        .copied()
        .filter(|backend| mask.contains(flags(*backend)))
        .collect()
}

/// Copies diagnostic identity from the selected adapter rather than inferring a GPU from its name.
fn identity(info: &wgpu::AdapterInfo) -> Adapter {
    Adapter {
        name: info.name.clone(),
        vendor: info.vendor,
        device: info.device,
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
    }
}

/// Flushes each startup boundary to client.log before executing the next native call.
fn log(message: std::fmt::Arguments<'_>) {
    let _ = writeln!(diagnostics::console::stderr(), "{message}");
    diagnostics::console::flush();
}

/// Initializes one exact adapter, preserving Bevy's feature and limit policy.
async fn create_renderer(
    backend: Backend,
    preferred: Option<&Adapter>,
    settings: &WgpuSettings,
    guard: &mut StartupGuard,
) -> Result<RenderCreation> {
    let mut descriptor = wgpu::InstanceDescriptor {
        backends: flags(backend),
        flags: settings.instance_flags,
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        backend_options: Default::default(),
        display: None,
    };
    descriptor.backend_options.dx12.shader_compiler = settings.dx12_shader_compiler.clone();
    descriptor.backend_options.dx12.presentation_system =
        wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
    descriptor.backend_options.dx12.latency_waitable_object =
        wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
    descriptor.backend_options.gl.gles_minor_version = settings.gles3_minor_version;
    let instance = wgpu::Instance::new(descriptor);
    let adapters = instance.enumerate_adapters(flags(backend)).await;
    for adapter in &adapters {
        log(format_args!("Renderer adapter: {:?}", adapter.get_info()));
    }
    let desired_name = std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .or_else(|| settings.adapter_name.clone());
    let force_fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
        .map_or(settings.force_fallback_adapter, |value| {
            !(value.is_empty() || value == "0" || value == "false")
        });
    let adapter = if !force_fallback {
        adapters.into_iter().find(|adapter| {
            let info = adapter.get_info();
            if let Some(name) = &desired_name {
                info.name.to_lowercase().contains(&name.to_lowercase())
            } else {
                preferred.is_some_and(|preferred| identity(&info) == *preferred)
            }
        })
    } else {
        None
    };
    let adapter = match adapter {
        Some(adapter) => adapter,
        None => instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: settings.power_preference,
                force_fallback_adapter: force_fallback,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .context("select renderer adapter")?,
    };
    let info = adapter.get_info();
    let explicit = std::env::var_os("WGPU_BACKEND").is_some();
    guard.begin(Attempt {
        backend,
        adapter: Some(identity(&info)),
        stage: Stage::CreatingDevice,
        explicit,
    })?;
    log(format_args!("Renderer creating device: {info:?}"));
    let (features, limits) = device_requirements(&adapter, settings);
    let (device, queue) = adapter
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
        .context("create renderer device")?;
    guard.begin(Attempt {
        backend,
        adapter: Some(identity(&info)),
        stage: Stage::FirstPresentation,
        explicit,
    })?;
    log(format_args!(
        "Renderer device ready; awaiting first presentation: {info:?}"
    ));
    Ok(RenderCreation::manual(
        RenderDevice::from(device),
        RenderQueue::new(queue),
        RenderAdapterInfo::new(info),
        RenderAdapter::new(adapter),
        RenderInstance::new(instance),
    ))
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

/// Clears the guard only after a primary-window presentation and GPU completion.
fn mark_presented(
    feedback: Res<render_scene::startup_presentation::StartupPresentation>,
    mut guard: ResMut<RendererGuard>,
    mut done: Local<bool>,
) {
    if !*done && feedback.has_presented() {
        match guard.0.presented() {
            Ok(()) => {
                *done = true;
                log(format_args!(
                    "Renderer first frame completed; saved working backend and adapter"
                ));
            }
            Err(error) => {
                *done = true;
                log(format_args!(
                    "Renderer could not clear startup sentinel: {error:#}"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_mask_uses_the_policy_order_and_skips_unsupported_gl() {
        assert_eq!(
            backends(wgpu::Backends::VULKAN | wgpu::Backends::DX12),
            [Backend::Vulkan, Backend::Dx12]
        );
        assert_eq!(backends(wgpu::Backends::DX12), [Backend::Dx12]);
        assert!(backends(wgpu::Backends::empty()).is_empty());
    }
}
