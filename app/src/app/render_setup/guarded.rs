//! Collects guarded adapter probes before creating the best available renderer.

use std::path::Path;

use super::device::{
    backends, flags, force_fallback, identity, instance, log, meets_requirements, request_device,
    requested_adapter_name,
};
use super::selection::{Candidate, eligible, probe_backend, rank, try_candidates, with_recovery};
use anyhow::Result;
use bevy::{
    app::OnAppExitSystems,
    prelude::{
        App, AppExit, IntoScheduleConfigs, Last, Local, MessageReader, Res, ResMut, Resource,
        Update,
    },
    render::{
        RenderPlugin,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::{RenderCreation, WgpuSettings},
    },
};
use launcher::graphics_backend::{Backend, Platform, candidates, default_order};
use launcher_host::renderer_startup::StartupGuard;

#[derive(Resource)]
struct RendererGuard(StartupGuard);

/// Applies explicit overrides, vendor defaults and crash recovery before device creation.
pub(super) fn render_plugin(
    app: &mut App,
    root: &Path,
    video: Option<Backend>,
    graphics_api_revision: u64,
    settings: WgpuSettings,
) -> Result<RenderPlugin> {
    let mut guard = StartupGuard::open(root)?;
    guard.refresh_settings(graphics_api_revision)?;
    if let Some(previous) = &guard.interrupted {
        log(format_args!("Renderer startup interrupted: {previous:?}"));
    }
    let platform = Platform::current();
    let supported: Vec<_> = backends(wgpu::Instance::enabled_backend_features())
        .into_iter()
        .filter(|backend| platform.backends().contains(backend))
        .collect();
    let environment =
        std::env::var_os("WGPU_BACKEND").map(|_| backends(settings.backends.unwrap_or_default()));
    let explicit = environment.is_some() || video.is_some();
    let defaults = guard
        .recovery
        .working
        .as_ref()
        .and_then(|attempt| attempt.adapter.as_ref())
        .map_or_else(
            || platform.backends().to_vec(),
            |adapter| default_order(platform, adapter),
        );
    let allowed = candidates(&defaults, &supported, environment.as_deref(), video, &[]);
    let requested_name = requested_adapter_name(&settings);
    let software = force_fallback(&settings);
    let creation = with_recovery(&mut guard, &allowed, explicit, |guard| {
        let order = candidates(
            &allowed,
            &allowed,
            environment.as_deref(),
            video,
            guard.recovery.failed_backends(),
        );
        let mut adapters = Vec::new();
        for backend in order {
            let found = probe_backend(guard, backend, explicit, || {
                log(format_args!(
                    "Renderer probing {backend:?}; DX12 shader compiler: {:?}",
                    settings.dx12_shader_compiler
                ));
                let display = app
                    .world()
                    .get_resource::<bevy::winit::DisplayHandleWrapper>()
                    .map(|display| {
                        Box::new(display.0.clone()) as Box<dyn wgpu::wgt::WgpuHasDisplayHandle>
                    });
                let instance = instance(backend, &settings, display);
                let mut found = Vec::new();
                for adapter in bevy::tasks::block_on(instance.enumerate_adapters(flags(backend))) {
                    if let Err(error) = meets_requirements(&adapter) {
                        log(format_args!(
                            "Renderer skipping incompatible adapter {:?}: {error:#}",
                            adapter.get_info()
                        ));
                        continue;
                    }
                    let candidate = Candidate {
                        backend,
                        identity: identity(&adapter),
                        device_type: adapter.get_info().device_type,
                        native: (instance.clone(), adapter),
                    };
                    if eligible(
                        &candidate,
                        platform,
                        requested_name.as_deref(),
                        software,
                        explicit,
                    ) {
                        found.push(candidate);
                    }
                }
                Ok(found)
            });
            match found {
                Ok(found) => adapters.extend(found),
                Err(error) => {
                    log(format_args!("Renderer {backend:?} probe failed: {error:#}"));
                }
            }
        }
        rank(&mut adapters, platform, settings.power_preference);
        try_candidates(adapters, guard, explicit, |candidate| {
            let (instance, adapter) = candidate.native;
            bevy::tasks::block_on(create_renderer(instance, adapter, &settings))
        })
    })?;
    install_guard(app, guard);
    Ok(RenderPlugin {
        render_creation: creation,
        ..Default::default()
    })
}

/// Tracks presentation and cancels startup on a controlled exit before window teardown.
fn install_guard(app: &mut App, guard: StartupGuard) {
    app.insert_resource(RendererGuard(guard))
        .add_systems(Update, mark_presented)
        .add_systems(Last, cancel_on_exit.in_set(OnAppExitSystems));
}

/// Leaves crash recovery untouched when the application requests an orderly shutdown.
fn cancel_on_exit(mut exits: MessageReader<AppExit>, mut guard: ResMut<RendererGuard>) {
    if exits.read().next().is_some()
        && let Err(error) = guard.0.cancel()
    {
        log(format_args!(
            "Renderer could not cancel startup sentinel: {error:#}"
        ));
    }
}

/// Builds the renderer from a candidate already guarded by the selection loop.
async fn create_renderer(
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<RenderCreation> {
    let info = adapter.get_info();
    log(format_args!("Renderer creating device: {info:?}"));
    let (device, queue) = request_device(&adapter, settings).await?;
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
mod tests;
