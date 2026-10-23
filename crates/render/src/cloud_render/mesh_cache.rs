//! Bounded speculative cloud windows. Admission still uses the exact requested view.

use std::{collections::HashMap, sync::Arc};

use assets::{AtmosphereRole, RuntimeAtmosphereAssets};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use meshing::{
    CLOUD_CELL_BLOCKS, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CLOUD_WORLD_PERIOD,
    cloud_viewport::{CloudViewport, ViewportCloudQuad, mesh_cloud_viewport},
};
use render_scene::{CloudGeometryDiagnostic, CloudRenderConfig};

const PREFETCH_RADIUS: i32 = 1;
pub(super) const MAX_CACHED_WINDOWS: usize = (PREFETCH_RADIUS as usize * 2 + 1).pow(2) * 2;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct WindowKey {
    centre: [i64; 2],
    above: bool,
}

impl WindowKey {
    /// Keys geometry by its finite cell window, independently of admission distance.
    fn new(viewport: CloudViewport, above: bool) -> Self {
        Self {
            centre: viewport.centre().map(|value| value as i64),
            above,
        }
    }
}

pub(super) struct CloudMesh {
    pub records: Box<[ViewportCloudQuad]>,
    pub diagnostic: Option<CloudGeometryDiagnostic>,
    #[cfg(test)]
    pub worker_thread: std::thread::ThreadId,
}

enum MeshJob {
    Pending(Task<Arc<CloudMesh>>),
    Ready(Arc<CloudMesh>),
}

#[derive(Default)]
pub(super) struct CloudMeshCache {
    identity: Option<[u8; 32]>,
    centre: Option<[i64; 2]>,
    jobs: HashMap<WindowKey, MeshJob>,
}

impl CloudMeshCache {
    /// Drops all speculative work when the texture generation changes.
    pub fn set_identity(&mut self, identity: [u8; 32]) {
        if self.identity != Some(identity) {
            self.jobs.clear();
            self.centre = None;
            self.identity = Some(identity);
        }
    }

    /// Returns exact records, meshing the window on this thread when no native worker has
    /// finished it, or yielding a pending browser view; the flag reports that miss.
    /// Cold views and teleports must not publish a late or different window.
    pub fn get(
        &mut self,
        runtime: &Arc<RuntimeAtmosphereAssets>,
        viewport: CloudViewport,
        above: bool,
        diagnostics: bool,
    ) -> Option<(Arc<CloudMesh>, bool)> {
        let key = WindowKey::new(viewport, above);
        #[cfg(target_arch = "wasm32")]
        {
            self.start(runtime, viewport, above, diagnostics);
            if let Some(MeshJob::Pending(task)) = self.jobs.get_mut(&key) {
                let mesh = block_on(bevy::tasks::poll_once(task))?;
                self.jobs.insert(key, MeshJob::Ready(Arc::clone(&mesh)));
                return Some((mesh, false));
            }
        }
        let (mesh, missed) = match self.jobs.remove(&key) {
            Some(MeshJob::Ready(mesh)) => (mesh, false),
            Some(MeshJob::Pending(task)) if task.is_finished() => (block_on(task), false),
            // An unfinished job can sit behind pipeline compiles on the shared pool; meshing
            // the one requested window here is bounded, waiting for that queue is not.
            _ => {
                let identity = self.identity.expect("cloud cache has an asset identity");
                (build(runtime, viewport, identity, diagnostics), true)
            }
        };
        self.jobs.insert(key, MeshJob::Ready(Arc::clone(&mesh)));
        Some((mesh, missed))
    }

    /// Keeps the current cell and its eight neighbours ready on both sides of
    /// the layer on native workers. Browser turns only prepare the requested window.
    pub fn prefetch(
        &mut self,
        runtime: &Arc<RuntimeAtmosphereAssets>,
        viewport: CloudViewport,
        diagnostics: bool,
    ) {
        let centre = WindowKey::new(viewport, false).centre;
        if self.centre == Some(centre) {
            return;
        }
        self.centre = Some(centre);
        self.jobs.retain(|key, _| {
            key.centre.iter().zip(centre).all(|(old, new)| {
                old.abs_diff(new) <= CLOUD_CELL_BLOCKS as u64 * PREFETCH_RADIUS as u64
            })
        });
        #[cfg(target_arch = "wasm32")]
        let _ = (runtime, diagnostics);
        #[cfg(not(target_arch = "wasm32"))]
        let config = CloudRenderConfig::legacy_fancy();
        #[cfg(not(target_arch = "wasm32"))]
        for z in -PREFETCH_RADIUS..=PREFETCH_RADIUS {
            for x in -PREFETCH_RADIUS..=PREFETCH_RADIUS {
                for above in [false, true] {
                    let sample = [
                        centre[0] as f64 + f64::from(x) * f64::from(CLOUD_CELL_BLOCKS),
                        centre[1] as f64 + f64::from(z) * f64::from(CLOUD_CELL_BLOCKS),
                    ];
                    if let Some(next) = CloudViewport::try_new(
                        sample,
                        config.mesh_size(),
                        config.grid_size(),
                        above,
                        false,
                    ) {
                        self.start(runtime, next, above, diagnostics);
                    }
                }
            }
        }
        debug_assert!(self.jobs.len() <= MAX_CACHED_WINDOWS);
    }

    /// Starts at most one worker for a cell window and cap orientation.
    fn start(
        &mut self,
        runtime: &Arc<RuntimeAtmosphereAssets>,
        viewport: CloudViewport,
        above: bool,
        diagnostics: bool,
    ) {
        let identity = self.identity.expect("cloud cache has an asset identity");
        self.jobs
            .entry(WindowKey::new(viewport, above))
            .or_insert_with(|| {
                let runtime = Arc::clone(runtime);
                MeshJob::Pending(
                    AsyncComputeTaskPool::get()
                        .spawn(async move { build(&runtime, viewport, identity, diagnostics) }),
                )
            });
    }

    #[cfg(test)]
    /// Waits for speculative workers without advancing any simulated frame.
    pub fn finish_pending(&self) {
        test_time::eventually("cloud mesh workers", || {
            self.jobs.values().all(|job| match job {
                MeshJob::Pending(task) => task.is_finished(),
                MeshJob::Ready(_) => true,
            })
        });
    }

    #[cfg(test)]
    /// Counts retained and pending cell windows for the cache bound regression.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }
}

/// Meshes one cell window on the calling thread.
fn build(
    runtime: &RuntimeAtmosphereAssets,
    viewport: CloudViewport,
    identity: [u8; 32],
    diagnostics: bool,
) -> Arc<CloudMesh> {
    let span = bevy::log::info_span!("cloud.rebuild", quads = 0, bytes = 0);
    let _guard = span.enter();
    let texture = runtime
        .texture(AtmosphereRole::Clouds)
        .expect("validated atmosphere contains clouds");
    let records = mesh_cloud_viewport(texture, viewport)
        .expect("validated cloud texture satisfies the finite window contract")
        .into_boxed_slice();
    span.record("quads", records.len());
    span.record("bytes", std::mem::size_of_val(records.as_ref()));
    let diagnostic = diagnostics.then(|| {
        CloudGeometryDiagnostic::from_viewport_layout(
            CloudRenderConfig::legacy_fancy(),
            identity,
            texture,
            &records,
            CLOUD_WORLD_PERIOD as u32 * 1_000,
            (CLOUD_UNDERSIDE_Y * 1_000.0) as i32,
            (CLOUD_TOP_Y * 1_000.0) as i32,
        )
        .expect("validated cloud geometry satisfies the diagnostic contract")
    });
    Arc::new(CloudMesh {
        records,
        diagnostic,
        #[cfg(test)]
        worker_thread: std::thread::current().id(),
    })
}
