//! Selects a single backend and adapter before entering native GPU device creation.

use std::path::Path;

use crate::gpu_probe::device::{backends, identity, instance, log, request_device, select_adapter};
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
use launcher::graphics_backend::{
    AdapterKey, Backend, INVENTORY_FLAG, Inventory, PROBE_FLAG, Platform, Verdict, blocklist,
    candidates, default_order,
};
use launcher_host::renderer_startup::{
    Attempt, Recovery, Stage, StartupGuard, load_probe_cache, probe, save_probe_cache,
};

#[derive(Resource)]
struct RendererGuard(StartupGuard);

/// Applies override, probe and vendor policy before loading one native backend in the parent.
pub(super) fn render_plugin(
    app: &mut App,
    root: &Path,
    video: Option<Backend>,
    settings: WgpuSettings,
) -> Result<RenderPlugin> {
    let mut guard = StartupGuard::open(root)?;
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
    let isolated = uses_isolated_probe(platform, &supported, explicit);
    let mut cache = if !isolated {
        Default::default()
    } else {
        load_probe_cache(root)?
    };
    let inventories = if !isolated {
        Vec::new()
    } else {
        inventory(platform, &supported)?
    };
    let mut order = if !isolated {
        direct_candidates(
            platform,
            &supported,
            environment.as_deref(),
            video,
            &guard.recovery,
        )
    } else {
        let selected = &inventories
            .first()
            .ok_or_else(|| {
                anyhow::anyhow!("No graphics adapter could be inventoried on the supported APIs")
            })?
            .1
            .selected;
        let fingerprint: Vec<_> = inventories
            .iter()
            .flat_map(|(_, inventory)| inventory.fingerprint())
            .collect();
        let known = cache.has_inventory();
        if cache.refresh(fingerprint) && known {
            guard.reset_recovery()?;
        }
        // A sentinel failure overrides a formerly successful probe for this driver revision.
        for (backend, inventory) in &inventories {
            if guard.recovery.failed_backends().contains(backend) {
                cache.record(inventory.selected.key(), *backend, Verdict::Failed);
            }
        }
        let defaults = default_order(platform, selected);
        let available: Vec<_> = inventories.iter().map(|(backend, _)| *backend).collect();
        let verdicts: Vec<_> = inventories
            .iter()
            .filter_map(|(backend, inventory)| {
                cache
                    .get(&inventory.selected.key(), *backend)
                    .map(|verdict| (*backend, verdict))
            })
            .collect();
        let blocked: Vec<_> = inventories
            .iter()
            .filter_map(|(backend, inventory)| {
                blocklist::rules()
                    .iter()
                    .find(|rule| {
                        rule.backend == *backend && rule.matches(platform, &inventory.selected)
                    })
                    .map(|rule| {
                        log(format_args!(
                            "Renderer avoiding {backend:?}: {}",
                            rule.reason
                        ));
                        *backend
                    })
            })
            .collect();
        for (backend, verdict) in &verdicts {
            log(format_args!("GPU probe cached: {backend:?}: {verdict:?}"));
        }
        save_probe_cache(root, &cache)?;
        candidates(
            &defaults,
            &available,
            None,
            None,
            &verdicts,
            &blocked,
            guard.recovery.failed_backends(),
        )
    };
    order.retain(|backend| supported.contains(backend));
    for backend in order {
        let key = inventories
            .iter()
            .find(|(api, _)| *api == backend)
            .map(|(_, inventory)| inventory.selected.key());
        if let Some(key) = &key {
            let verdict = probe::cached(root, &mut cache, key, backend, || {
                log(format_args!("GPU probe starting: {backend:?} {key:?}"));
                probe::run(&[
                    PROBE_FLAG.into(),
                    backend.name().into(),
                    serde_json::to_string(key)?,
                ])
            })?;
            log(format_args!(
                "GPU probe verdict: {backend:?} {key:?}: {verdict:?}"
            ));
            if matches!(verdict, Verdict::Failed | Verdict::TimedOut) {
                continue;
            }
        }
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
        match bevy::tasks::block_on(create_renderer(
            backend,
            key.as_ref(),
            &settings,
            explicit,
            app.world()
                .get_resource::<bevy::winit::DisplayHandleWrapper>()
                .map(|display| {
                    Box::new(display.0.clone()) as Box<dyn wgpu::wgt::WgpuHasDisplayHandle>
                }),
            &mut guard,
        )) {
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

/// Keeps a proven direct-startup fallback first while preserving overrides and crash exclusions.
fn direct_candidates(
    platform: Platform,
    supported: &[Backend],
    environment: Option<&[Backend]>,
    video: Option<Backend>,
    recovery: &Recovery,
) -> Vec<Backend> {
    let mut order = candidates(
        platform.backends(),
        supported,
        environment,
        video,
        &[],
        &[],
        if supported.len() == 1 {
            &[]
        } else {
            recovery.failed_backends()
        },
    );
    if environment.is_none()
        && video.is_none()
        && let Some(working) = &recovery.working
    {
        order.sort_by_key(|backend| *backend != working.backend);
    }
    order
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

/// Probes only automatic multi-API desktop startup, where native lifecycle events can run.
fn uses_isolated_probe(platform: Platform, supported: &[Backend], explicit: bool) -> bool {
    !explicit && supported.len() > 1 && platform != Platform::Android
}

/// Inventories the requested high-performance GPU outside the parent, falling back after ICD failure.
fn inventory(platform: Platform, supported: &[Backend]) -> Result<Vec<(Backend, Inventory)>> {
    let mut inventories = Vec::new();
    for backend in platform
        .inventory_order()
        .iter()
        .filter(|backend| supported.contains(backend))
    {
        log(format_args!("GPU inventory starting: {backend:?}"));
        match probe::run(&[INVENTORY_FLAG.into(), backend.name().into()]) {
            Ok(output) if output.verdict == Verdict::Passed => {
                match serde_json::from_str::<Inventory>(&output.stdout) {
                    Ok(inventory) => {
                        log(format_args!(
                            "GPU inventory selected: {:?}",
                            inventory.selected
                        ));
                        inventories.push((*backend, inventory));
                    }
                    Err(error) => log(format_args!("GPU inventory invalid: {error}")),
                }
            }
            Ok(output) => log(format_args!(
                "GPU inventory verdict: {backend:?}: {:?}",
                output.verdict
            )),
            Err(error) => log(format_args!(
                "GPU inventory transport failed: {backend:?}: {error:#}"
            )),
        }
    }
    Ok(inventories)
}

/// Initializes the exact probed adapter, retaining the sentinel until the real first frame.
async fn create_renderer(
    backend: Backend,
    key: Option<&AdapterKey>,
    settings: &WgpuSettings,
    explicit: bool,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
    guard: &mut StartupGuard,
) -> Result<RenderCreation> {
    let instance = instance(backend, settings, display);
    let adapter = select_adapter(&instance, settings, key).await?;
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
    fn android_keeps_a_successful_fallback_on_subsequent_launches() {
        let dir = tempfile::tempdir().unwrap();
        let select = |recovery: &Recovery, environment: Option<&[Backend]>, video| {
            direct_candidates(
                Platform::Android,
                Platform::Android.backends(),
                environment,
                video,
                recovery,
            )
        };
        let mut first = StartupGuard::open(dir.path()).unwrap();
        assert_eq!(select(&first.recovery, None, None)[0], Backend::Vulkan);
        first.begin(pending_attempt(Backend::Vulkan)).unwrap();
        drop(first);

        let mut fallback = StartupGuard::open(dir.path()).unwrap();
        assert_eq!(select(&fallback.recovery, None, None), [Backend::Gl]);
        fallback.begin(pending_attempt(Backend::Gl)).unwrap();
        fallback.presented().unwrap();
        let mut next = StartupGuard::open(dir.path()).unwrap();
        assert_eq!(select(&next.recovery, None, None)[0], Backend::Gl);
        assert_eq!(
            select(&next.recovery, None, Some(Backend::Vulkan)),
            [Backend::Vulkan]
        );
        assert_eq!(
            select(
                &next.recovery,
                Some(&[Backend::Vulkan, Backend::Gl]),
                Some(Backend::Gl)
            ),
            [Backend::Vulkan, Backend::Gl]
        );
        assert_eq!(
            direct_candidates(
                Platform::Android,
                &[Backend::Vulkan],
                None,
                None,
                &next.recovery
            ),
            [Backend::Vulkan]
        );
        next.begin(pending_attempt(Backend::Gl)).unwrap();
        drop(next);
        let interrupted = StartupGuard::open(dir.path()).unwrap();
        assert_eq!(select(&interrupted.recovery, None, None), [Backend::Vulkan]);
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
            app.update();
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
    fn lone_backend_and_android_never_start_synchronous_helpers() {
        assert!(!uses_isolated_probe(
            Platform::Macos,
            &[Backend::Metal],
            false
        ));
        assert!(!uses_isolated_probe(
            Platform::Linux,
            &[Backend::Vulkan],
            false
        ));
        assert!(!uses_isolated_probe(
            Platform::Android,
            Platform::Android.backends(),
            false
        ));
        assert!(uses_isolated_probe(
            Platform::Windows,
            Platform::Windows.backends(),
            false
        ));
        assert!(!uses_isolated_probe(
            Platform::Windows,
            Platform::Windows.backends(),
            true
        ));
    }

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
