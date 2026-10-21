//! Collects guarded adapter probes before creating the best available renderer.

use std::path::Path;

use super::device::{
    backends, flags, force_fallback, identity, instance, log, meets_requirements, request_device,
    requested_adapter_name,
};
use super::selection::{
    Candidate, VideoFallback, eligible, probe_backend, rank, summary, try_candidates,
    with_recovery, with_video_fallback,
};
use anyhow::Result;
use bevy::{
    app::OnAppExitSystems,
    prelude::{
        App, AppExit, Commands, IntoScheduleConfigs, Last, Local, MessageReader, Res, ResMut,
        Resource, Update, resource_exists,
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
    guard.refresh_settings(graphics_api_revision, video.is_some())?;
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
    let defaults = guard
        .recovery
        .working
        .as_ref()
        .and_then(|attempt| attempt.adapter.as_ref())
        .map_or_else(
            || platform.backends().to_vec(),
            |adapter| default_order(platform, adapter),
        );
    let requested_name = requested_adapter_name(&settings);
    let software = force_fallback(&settings);
    let crashed = guard.recovery.crashed_choice();
    let ((creation, chosen), fallback) = with_video_fallback(
        environment.is_some(),
        video,
        crashed,
        |video, excluded| {
            let explicit = environment.is_some() || video.is_some();
            let allowed = candidates(
                &defaults,
                &supported,
                environment.as_deref(),
                video,
                excluded,
            );
            with_recovery(&mut guard, &allowed, explicit, |guard| {
                let failed = guard.recovery.failed_backends();
                let order = candidates(&allowed, &allowed, environment.as_deref(), video, failed);
                log(format_args!(
                    "Renderer trying {order:?} (allowed {allowed:?}; excluded after interrupted startups: {failed:?})"
                ));
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
                                Box::new(display.0.clone())
                                    as Box<dyn wgpu::wgt::WgpuHasDisplayHandle>
                            });
                        let instance = instance(backend, &settings, display);
                        let mut found = Vec::new();
                        for adapter in
                            bevy::tasks::block_on(instance.enumerate_adapters(flags(backend)))
                        {
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
                    let backend = candidate.backend;
                    let (instance, adapter) = candidate.native;
                    bevy::tasks::block_on(create_renderer(instance, adapter, &settings))
                        .map(|creation| (creation, backend))
                })
            })
        },
    )?;
    log(format_args!(
        "{}",
        summary(environment.as_deref(), video, chosen, fallback.as_ref())
    ));
    install_guard(app, guard);
    if let Some(fallback) = fallback {
        install_notice(app, notice(&fallback, chosen, platform));
    }
    Ok(RenderPlugin {
        render_creation: creation,
        ..Default::default()
    })
}

/// A failed saved Graphics API, shown once the menu is on screen.
#[derive(Resource)]
struct FallbackNotice(String);

/// Tells the player which API replaced their saved choice and why.
fn notice(fallback: &VideoFallback, chosen: Backend, platform: Platform) -> String {
    format!(
        "{} could not start, so this launch uses {}. Update your graphics driver or choose another Graphics API in Video settings. Reason: {}",
        fallback.requested.label(platform),
        chosen.label(platform),
        fallback.reason
    )
}

/// Queues the fallback notice for the menu.
fn install_notice(app: &mut App, message: String) {
    app.insert_resource(FallbackNotice(message)).add_systems(
        Update,
        show_fallback_notice.run_if(resource_exists::<FallbackNotice>),
    );
}

/// Opens the notice once, after the menu exists and is visible.
fn show_fallback_notice(
    mut commands: Commands,
    notice: Res<FallbackNotice>,
    menu: Option<ResMut<client_shell::menu::MenuRuntime>>,
) {
    if let Some(mut menu) = menu.filter(|menu| menu.is_visible()) {
        menu.show_notice(&notice.0);
        commands.remove_resource::<FallbackNotice>();
    }
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
