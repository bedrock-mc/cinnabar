//! Selects a single backend and adapter before entering native GPU device creation.

use std::path::Path;

use super::device::{
    backends, identity, instance, log, request_device, requested_adapter_name, search_backends,
    select_adapter,
};
use anyhow::{Result, bail};
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
use launcher::graphics_backend::{Backend, Platform, blocklist, candidates, default_order};
use launcher_host::renderer_startup::{Attempt, Stage, StartupGuard};

#[derive(Resource)]
struct RendererGuard(StartupGuard);

/// Applies explicit overrides, vendor defaults and crash recovery before device creation.
pub(super) fn render_plugin(
    app: &mut App,
    root: &Path,
    video: Option<Backend>,
    settings: WgpuSettings,
) -> Result<RenderPlugin> {
    let mut guard = StartupGuard::open(root)?;
    guard.refresh_direct_driver()?;
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
    let allowed = candidates(
        platform.backends(),
        &supported,
        environment.as_deref(),
        video,
        &[],
    );
    let requested_name = requested_adapter_name(&settings);
    let select = |backend, name: Option<&str>, guard: &mut StartupGuard| {
        guard.begin(Attempt {
            backend,
            adapter: None,
            stage: Stage::EnumeratingAdapters,
            explicit,
        })?;
        log(format_args!(
            "Renderer opening {backend:?}; DX12 shader compiler: {:?}",
            settings.dx12_shader_compiler
        ));
        let display = app
            .world()
            .get_resource::<bevy::winit::DisplayHandleWrapper>()
            .map(|display| Box::new(display.0.clone()) as Box<dyn wgpu::wgt::WgpuHasDisplayHandle>);
        let instance = instance(backend, &settings, display);
        let adapter = bevy::tasks::block_on(select_adapter(&instance, &settings, name))?;
        if let Some(adapter) = &adapter {
            guard.begin(Attempt {
                backend,
                adapter: Some(identity(adapter)),
                stage: Stage::EnumeratingAdapters,
                explicit,
            })?;
        }
        Ok::<_, anyhow::Error>(adapter.map(|adapter| (instance, adapter)))
    };
    let mut retried = false;
    loop {
        let mut order = candidates(
            &allowed,
            &allowed,
            environment.as_deref(),
            video,
            &guard.recovery.enumeration_exclusions(),
        );
        // Discover Windows vendor policy through DX12, using only the requested GPU in the name pass.
        let mut windows_adapter = None;
        if !explicit && platform == Platform::Windows && order.contains(&Backend::Dx12) {
            let mut selected = select(Backend::Dx12, requested_name.as_deref(), &mut guard);
            let reusable = !matches!(selected, Ok(None));
            if !reusable {
                guard.failed()?;
                // Learn fallback vendor policy, but do not accept this unnamed GPU yet.
                selected = select(Backend::Dx12, None, &mut guard);
            }
            match selected {
                Ok(Some((instance, adapter))) => {
                    let defaults = default_order(platform, &identity(&adapter));
                    order.sort_by_key(|backend| defaults.iter().position(|api| api == backend));
                    if reusable && order.first() == Some(&Backend::Dx12) {
                        windows_adapter = Some((instance, adapter));
                    } else {
                        // Retain this API's sentinel while native cleanup runs.
                        drop((adapter, instance));
                        guard.failed()?;
                    }
                }
                Ok(None) => guard.failed()?,
                Err(error) => {
                    log(format_args!(
                        "Renderer DX12 adapter selection failed: {error:#}"
                    ));
                    guard.failed()?;
                }
            }
        }
        let creation = search_backends(&order, requested_name.as_deref(), |backend, name| {
            let selected = if backend == Backend::Dx12 {
                windows_adapter
                    .take()
                    .map(|selected| Ok(Some(selected)))
                    .unwrap_or_else(|| select(backend, name, &mut guard))
            } else {
                select(backend, name, &mut guard)
            };
            let result = selected.and_then(|selected| {
                let Some((instance, adapter)) = selected else {
                    return Ok(None);
                };
                if !explicit && guard.adapter_excluded(backend, &identity(&adapter))? {
                    return Ok(None);
                }
                bevy::tasks::block_on(create_renderer(
                    backend, instance, adapter, &settings, explicit, &mut guard,
                ))
                .map(Some)
            });
            match result {
                Ok(Some(creation)) => Ok(Some(creation)),
                other => {
                    if let Err(error) = other {
                        log(format_args!(
                            "Renderer {backend:?} initialization failed: {error:#}"
                        ));
                    }
                    guard.failed()?;
                    Ok(None)
                }
            }
        })?;
        if let Some(creation) = creation {
            install_guard(app, guard);
            return Ok(RenderPlugin {
                render_creation: creation,
                ..Default::default()
            });
        }
        if explicit || retried || !guard.retry_exclusions(&allowed)? {
            break;
        }
        retried = true;
    }
    bail!(
        "No safe supported graphics API remains. See client.log and crashes; update GPU drivers or choose an explicit Graphics API to retry."
    );
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

/// Creates the chosen device, retaining the sentinel through the first completed frame.
async fn create_renderer(
    backend: Backend,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    settings: &WgpuSettings,
    explicit: bool,
    guard: &mut StartupGuard,
) -> Result<RenderCreation> {
    let info = adapter.get_info();
    if !explicit {
        let identity = identity(&adapter);
        if let Some(rule) = blocklist::rules()
            .iter()
            .find(|rule| rule.backend == backend && rule.matches(Platform::current(), &identity))
        {
            bail!("Graphics API {backend:?} is blocklisted: {}", rule.reason);
        }
    }
    let attempt = |stage| Attempt {
        backend,
        adapter: Some(identity(&adapter)),
        stage,
        explicit,
    };
    guard.begin(attempt(Stage::CreatingDevice))?;
    log(format_args!("Renderer creating device: {info:?}"));
    let (device, queue) = request_device(&adapter, settings).await?;
    guard.begin(attempt(Stage::FirstPresentation))?;
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
