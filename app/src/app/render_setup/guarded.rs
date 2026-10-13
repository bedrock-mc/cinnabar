//! Selects a single backend and adapter before entering native GPU device creation.

use std::path::Path;

use super::device::{backends, identity, instance, log, request_device, select_adapter};
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
    let mut order = candidates(
        platform.backends(),
        &supported,
        environment.as_deref(),
        video,
        guard.recovery.failed_backends(),
    );
    let select = |backend, guard: &mut StartupGuard| {
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
        let adapter = bevy::tasks::block_on(select_adapter(&instance, &settings))?;
        Ok::<_, anyhow::Error>((instance, adapter))
    };
    // Read the Windows adapter through DX12 before loading a potentially broken Vulkan ICD.
    // Reuse it if selected; other platforms already have a fixed API preference.
    let mut windows_adapter = None;
    if !explicit && platform == Platform::Windows && order.contains(&Backend::Dx12) {
        match select(Backend::Dx12, &mut guard) {
            Ok((instance, adapter)) => {
                order = candidates(
                    &default_order(platform, &identity(&adapter)),
                    &supported,
                    None,
                    None,
                    guard.recovery.failed_backends(),
                );
                if order.first() == Some(&Backend::Dx12) {
                    windows_adapter = Some((instance, adapter));
                } else {
                    // Drop the unused API while its own sentinel still describes native cleanup.
                    drop((adapter, instance));
                    guard.failed()?;
                }
            }
            Err(error) => {
                log(format_args!(
                    "Renderer DX12 adapter selection failed: {error:#}"
                ));
                guard.failed()?;
                order.retain(|backend| *backend != Backend::Dx12);
            }
        }
    }
    for backend in order {
        let selected = if backend == Backend::Dx12 {
            windows_adapter
                .take()
                .map(Ok)
                .unwrap_or_else(|| select(backend, &mut guard))
        } else {
            select(backend, &mut guard)
        };
        let result = selected.and_then(|(instance, adapter)| {
            bevy::tasks::block_on(create_renderer(
                backend, instance, adapter, &settings, explicit, &mut guard,
            ))
        });
        match result {
            Ok(creation) => {
                install_guard(app, guard);
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
mod tests {
    use super::*;
    use bevy::{
        prelude::{Entity, With},
        window::{PrimaryWindow, WindowCloseRequested, WindowPlugin},
    };
    use render_scene::startup_presentation::StartupPresentation;

    /// Builds an unfinished first frame without loading a graphics driver.
    fn pending_attempt(backend: Backend) -> Attempt {
        Attempt {
            backend,
            adapter: None,
            stage: Stage::FirstPresentation,
            explicit: false,
        }
    }

    #[test]
    fn closing_the_window_before_presentation_does_not_poison_recovery() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..2 {
            let mut guard = StartupGuard::open(dir.path()).unwrap();
            assert!(guard.interrupted.is_none());
            assert!(guard.recovery.failed_backends().is_empty());
            assert!(guard.recovery.working.is_none());
            guard.begin(pending_attempt(Backend::Vulkan)).unwrap();
            let mut app = App::new();
            app.add_plugins(WindowPlugin::default())
                .init_resource::<StartupPresentation>();
            install_guard(&mut app, guard);
            let mut windows = app
                .world_mut()
                .query_filtered::<Entity, With<PrimaryWindow>>();
            let window = windows.single(app.world()).unwrap();
            app.world_mut()
                .write_message(WindowCloseRequested { window });
            test_time::eventually("window closure requests application exit", || {
                app.update();
                app.should_exit().is_some()
            });
            assert_eq!(app.should_exit(), Some(AppExit::Success));
            assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
        }
        let next = StartupGuard::open(dir.path()).unwrap();
        assert!(next.interrupted.is_none());
        assert!(next.recovery.failed_backends().is_empty());
        assert!(next.recovery.working.is_none());
    }

    #[test]
    fn unfinished_startup_without_an_exit_message_remains_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = StartupGuard::open(dir.path()).unwrap();
        let pending = pending_attempt(Backend::Vulkan);
        guard.begin(pending.clone()).unwrap();
        let mut app = App::new();
        app.init_resource::<StartupPresentation>();
        install_guard(&mut app, guard);
        app.update();
        assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
        drop(app);
        let next = StartupGuard::open(dir.path()).unwrap();
        assert_eq!(next.recovery.failed_backends(), [Backend::Vulkan]);
    }

    #[test]
    fn backend_mask_uses_the_policy_order() {
        assert_eq!(
            backends(wgpu::Backends::VULKAN | wgpu::Backends::DX12),
            [Backend::Vulkan, Backend::Dx12]
        );
        assert_eq!(backends(wgpu::Backends::DX12), [Backend::Dx12]);
        assert!(backends(wgpu::Backends::empty()).is_empty());
    }
}
