//! Bounded immutable constructor analysis, owned by one finalized catalog layout.

use std::{
    mem::{align_of, size_of},
    sync::{Mutex, Weak},
};

use super::*;
use render_frame::motion_sampling::MotionSampling;

const MAX_ENTRIES: usize = 32;
const MAX_BYTES: usize = 4 * 1024 * 1024;
// These template elements must not acquire unaccounted nested allocations.
const _: () = assert!(
    !std::mem::needs_drop::<RuntimeBone>()
        && !std::mem::needs_drop::<ControllerState>()
        && !std::mem::needs_drop::<BoneTransform>()
);
// Include all reserved entry slots and the outer catalog allocation a Weak can retain.
// Allocator bookkeeping is additional bounded overhead, not part of this payload budget.
const FIXED_BYTES: usize = size_of::<RigPlans>()
    + MAX_ENTRIES * size_of::<Entry>()
    + size_of::<RuntimeEntityAssets>()
    + 2 * size_of::<usize>()
    + align_of::<RuntimeEntityAssets>();

/// One finalized layout's analysis cache guarded by the exact catalog allocation.
#[derive(Debug, Default)]
pub(super) struct RigPlans {
    catalog: Weak<RuntimeEntityAssets>,
    cache: Mutex<Cache>,
    #[cfg(test)]
    rig_builds: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    motion_builds: std::sync::atomic::AtomicUsize,
}

#[derive(Debug)]
struct Cache {
    entries: Vec<Entry>,
    bytes: usize,
    limit: usize,
}

#[derive(Debug)]
struct Entry {
    plan: Plan,
    bytes: usize,
}

#[derive(Debug)]
enum Plan {
    Rig {
        binding: usize,
        geometry: usize,
        value: Arc<RigPlan>,
    },
    Motion {
        binding: usize,
        geometry: usize,
        controllers: Box<[usize]>,
        clips: Box<[usize]>,
        value: Arc<MotionSampling>,
    },
}

/// Authored templates are copied into each actor before any mutable runtime work.
#[derive(Debug)]
pub(super) struct RigPlan {
    pub(super) bones: Box<[RuntimeBone]>,
    pub(super) composition: Arc<pose::Composition>,
    pub(super) names: Box<[Box<str>]>,
    pub(super) rest: Box<[BoneTransform]>,
    pub(super) controllers: Box<[ControllerState]>,
    pub(super) camera: bool,
    pub(super) swing: bool,
    pub(super) frames: bool,
    pub(super) presentation: evaluation::PresentationReads,
    pub(super) always_depth: bool,
}

impl Default for Cache {
    /// Reserves the entire fixed slot cost even before its first heap allocation.
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            bytes: FIXED_BYTES,
            limit: MAX_BYTES,
        }
    }
}

impl RigPlans {
    /// Binds this cache to an owning catalog after the layout's analysis is finalized.
    pub(super) fn new(catalog: &Arc<RuntimeEntityAssets>) -> Self {
        Self {
            catalog: Arc::downgrade(catalog),
            ..Default::default()
        }
    }

    /// Releases cached payload without changing layout definitions or per-actor state.
    pub(super) fn clear(&self) {
        *self
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Cache::default();
    }

    /// Compares the supplied catalog while an owning upgrade protects its allocation identity.
    fn matches(&self, assets: &RuntimeEntityAssets) -> bool {
        self.catalog
            .upgrade()
            .is_some_and(|catalog| std::ptr::eq(catalog.as_ref(), assets))
    }
}

impl Cache {
    /// Admits only fully accounted payload, evicting old entries within the aggregate limits.
    fn insert(&mut self, bytes: Option<usize>, plan: impl FnOnce() -> Plan) {
        let Some(room) = self.limit.checked_sub(FIXED_BYTES) else {
            return;
        };
        let Some(bytes) = bytes.filter(|bytes| *bytes <= room) else {
            return;
        };
        while self.entries.len() == MAX_ENTRIES || self.bytes > self.limit - bytes {
            self.bytes -= self.entries.remove(0).bytes;
        }
        if self.entries.capacity() == 0 {
            self.entries.reserve_exact(MAX_ENTRIES);
        }
        self.entries.push(Entry {
            plan: plan(),
            bytes,
        });
        self.bytes += bytes;
    }

    /// Reuses a rig template and moves its entry to the most recently used position.
    fn rig(&mut self, binding: usize, geometry: usize) -> Option<Arc<RigPlan>> {
        let index = self.entries.iter().position(|entry| {
            matches!(&entry.plan,
            Plan::Rig { binding: b, geometry: g, .. } if (*b, *g) == (binding, geometry))
        })?;
        let entry = self.entries.remove(index);
        let Plan::Rig { value, .. } = &entry.plan else {
            unreachable!()
        };
        let result = Arc::clone(value);
        self.entries.push(entry);
        Some(result)
    }

    /// Matches exact topology IDs; live controller clocks, states and variables are not keys.
    fn motion(
        &mut self,
        binding: usize,
        geometry: usize,
        controllers: &[ControllerState],
        clips: &[usize],
    ) -> Option<Arc<MotionSampling>> {
        let index = self.entries.iter().position(|entry| {
            matches!(&entry.plan,
            Plan::Motion { binding: b, geometry: g, controllers: ids, clips: extra, .. }
            if (*b, *g) == (binding, geometry) && extra.as_ref() == clips
                && ids.len() == controllers.len()
                && ids.iter().zip(controllers).all(|(id, runtime)| *id == runtime.controller))
        })?;
        let entry = self.entries.remove(index);
        let Plan::Motion { value, .. } = &entry.plan else {
            unreachable!()
        };
        let result = Arc::clone(value);
        self.entries.push(entry);
        Some(result)
    }
}

impl VariableLayout {
    /// Reuses pure analysis only after the caller has selected this actor's geometry.
    pub(super) fn rig_plan(
        &self,
        assets: &RuntimeEntityAssets,
        binding: usize,
        geometry: usize,
    ) -> Option<Arc<RigPlan>> {
        let build = || {
            #[cfg(test)]
            self.plans
                .rig_builds
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            RigPlan::new(assets, self, binding, geometry).map(Arc::new)
        };
        if !self.plans.matches(assets) {
            return build();
        }
        let mut cache = self
            .plans
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(plan) = cache.rig(binding, geometry) {
            return Some(plan);
        }
        let plan = build()?;
        cache.insert(plan.retained_bytes().and_then(arc_bytes::<RigPlan>), || {
            Plan::Rig {
                binding,
                geometry,
                value: Arc::clone(&plan),
            }
        });
        Some(plan)
    }

    /// Shares immutable dependency analysis while keeping runtime topology and execution private.
    pub(super) fn motion_plan(
        &self,
        assets: &RuntimeEntityAssets,
        binding: usize,
        geometry: usize,
        controllers: &[ControllerState],
        clips: &[usize],
    ) -> Arc<MotionSampling> {
        let build = || {
            #[cfg(test)]
            self.plans
                .motion_builds
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            MotionSampling::new(assets, self, binding, geometry, controllers, clips)
        };
        if !self.plans.matches(assets) {
            return build();
        }
        let mut cache = self
            .plans
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(plan) = cache.motion(binding, geometry, controllers, clips) {
            return plan;
        }
        let plan = build();
        let bytes = plan
            .retained_bytes()
            .and_then(arc_bytes::<MotionSampling>)
            .and_then(|bytes| {
                bytes.checked_add(
                    controllers
                        .len()
                        .checked_add(clips.len())?
                        .checked_mul(size_of::<usize>())?,
                )
            });
        cache.insert(bytes, || Plan::Motion {
            binding,
            geometry,
            controllers: controllers
                .iter()
                .map(|runtime| runtime.controller)
                .collect(),
            clips: clips.into(),
            value: Arc::clone(&plan),
        });
        plan
    }
}

impl RigPlan {
    /// Resolves only authored skeleton, initial topology and static presentation dependencies.
    fn new(
        assets: &RuntimeEntityAssets,
        layout: &VariableLayout,
        binding: usize,
        geometry: usize,
    ) -> Option<Self> {
        let candidate = assets.rig_geometries().get(geometry)?;
        if candidate.animation_count as usize + candidate.controller_count as usize
            > MAX_RUNTIME_BINDINGS_PER_RIG
        {
            return None;
        }
        let (bones, names) = resolve_bones(assets, candidate.geometry as usize)?;
        let composition = Arc::new(pose::Composition::new(&bones)?);
        let rest = compose_pose(&bones, &[])?;
        let first = candidate.first_controller as usize;
        let end = first.checked_add(candidate.controller_count as usize)?;
        let mut controllers = Vec::new();
        for entry in assets.rig_controllers().get(first..end)? {
            collect_controllers(assets, entry.controller as usize, 0, &mut controllers)?;
        }
        let camera = render_frame::camera::needs_camera_sampling(
            assets,
            layout,
            binding,
            geometry,
            &controllers,
        );
        let swing = render_frame::camera::needs_swing_sampling(
            assets,
            layout,
            binding,
            geometry,
            &controllers,
        );
        Some(Self {
            bones: bones.into(),
            composition,
            names: names.into(),
            rest: rest.into(),
            controllers: controllers.into(),
            camera,
            swing,
            frames: camera || render_frame::sampling::needs_frame_sampling(assets, binding),
            presentation: render_frame::camera::presentation_expressions(
                assets, layout, binding, geometry,
            ),
            always_depth: assets
                .render_layers(binding)
                .iter()
                .any(|layer| layer.material_state.is_some_and(|state| state.depth_always)),
        })
    }

    /// Accounts every boxed template array and owned lowercase name, without allocator guesses.
    fn retained_bytes(&self) -> Option<usize> {
        // Exhaustive matching makes new retained fields require an accounting review.
        let Self {
            bones,
            composition,
            names,
            rest,
            controllers,
            camera: _,
            swing: _,
            frames: _,
            presentation: _,
            always_depth: _,
        } = self;
        let arrays = arc_bytes::<pose::Composition>(
            size_of::<pose::Composition>().checked_add(composition.heap_bytes())?,
        )?
        .checked_add(bones.len().checked_mul(size_of::<RuntimeBone>())?)?
        .checked_add(names.len().checked_mul(size_of::<Box<str>>())?)?
        .checked_add(rest.len().checked_mul(size_of::<BoneTransform>())?)?
        .checked_add(
            controllers
                .len()
                .checked_mul(size_of::<ControllerState>())?,
        )?;
        names
            .iter()
            .try_fold(size_of::<Self>().checked_add(arrays)?, |bytes, name| {
                bytes.checked_add(name.len())
            })
    }
}

/// Charges reference-count header and alignment headroom in addition to an Arc's full payload.
fn arc_bytes<T>(payload: usize) -> Option<usize> {
    payload
        .checked_add(2 * size_of::<usize>())?
        .checked_add(align_of::<T>())
}

#[cfg(test)]
mod tests;
