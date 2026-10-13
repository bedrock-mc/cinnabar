//! Selects a single backend and adapter before entering native GPU device creation.

use std::path::Path;

use crate::gpu_probe::device::{backends, identity, instance, log, request_device, select_adapter};
use anyhow::{Result, bail};
use bevy::{
    prelude::{App, Local, Res, ResMut, Resource, Update},
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
    Attempt, Stage, StartupGuard, load_probe_cache, probe, save_probe_cache,
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
    let mut cache = if explicit {
        Default::default()
    } else {
        load_probe_cache(root)?
    };
    let inventories = if explicit {
        Vec::new()
    } else {
        inventory(platform, &supported)?
    };
    let mut order = if explicit {
        candidates(
            platform.backends(),
            &supported,
            environment.as_deref(),
            video,
            &[],
            &[],
            guard.recovery.failed_backends(),
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
            if verdict != Verdict::Passed {
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
                if let Some(key) = &key {
                    cache.record(key.clone(), backend, Verdict::Failed);
                    save_probe_cache(root, &cache)?;
                }
                guard.failed()?;
            }
        }
    }
    bail!(
        "No safe supported graphics API remains. See client.log and crashes; update GPU drivers or choose an explicit Graphics API to retry."
    );
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
