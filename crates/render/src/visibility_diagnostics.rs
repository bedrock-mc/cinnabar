use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use bevy::{
    prelude::{Entity, Resource},
    render::extract_resource::ExtractResource,
};
use world::SubChunkKey;

pub const MAX_VISIBILITY_DIAGNOSTIC_KEYS: usize = 65_536;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityKeyDigest {
    pub count: u64,
    pub hash: u64,
}

impl VisibilityKeyDigest {
    #[must_use]
    pub fn from_keys(keys: impl IntoIterator<Item = SubChunkKey>) -> Self {
        keys.into_iter().fold(Self::default(), |mut digest, key| {
            digest.insert(key);
            digest
        })
    }

    fn insert(&mut self, key: SubChunkKey) {
        self.count = self.count.saturating_add(1);
        self.hash = self.hash.wrapping_add(hash_sub_chunk_key(key));
    }

    #[must_use]
    pub const fn loss_to(self, next: Self) -> Self {
        Self {
            count: self.count.saturating_sub(next.count),
            hash: self.hash.wrapping_sub(next.hash),
        }
    }
}

fn hash_sub_chunk_key(key: SubChunkKey) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in key
        .dimension
        .to_le_bytes()
        .into_iter()
        .chain(key.x.to_le_bytes())
        .chain(key.y.to_le_bytes())
        .chain(key.z.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^ (hash >> 33)
}

#[derive(Resource, ExtractResource, Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityDiagnosticsInput {
    enabled: bool,
    frame_generation: u64,
    resident_mesh: VisibilityKeyDigest,
    cave_visible: VisibilityKeyDigest,
}

impl VisibilityDiagnosticsInput {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            frame_generation: 0,
            resident_mesh: VisibilityKeyDigest { count: 0, hash: 0 },
            cave_visible: VisibilityKeyDigest { count: 0, hash: 0 },
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn resident_mesh(&self) -> VisibilityKeyDigest {
        self.resident_mesh
    }

    #[must_use]
    pub const fn cave_visible(&self) -> VisibilityKeyDigest {
        self.cave_visible
    }

    pub fn advance(
        &mut self,
        resident_mesh: VisibilityKeyDigest,
        cave_visible: VisibilityKeyDigest,
    ) {
        if !self.enabled {
            return;
        }
        self.frame_generation = self.frame_generation.wrapping_add(1).max(1);
        self.resident_mesh = resident_mesh;
        self.cave_visible = cave_visible;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractedCameraIdentity {
    pub stable_id: u64,
    pub pose_hash: u64,
    pub frustum_hash: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractedViewGenerations {
    pub pose: u64,
    pub view: u64,
}

impl ExtractedViewGenerations {
    #[must_use]
    pub const fn new(pose: u64, view: u64) -> Self {
        Self { pose, view }
    }
}

#[derive(Resource, Debug, Default)]
pub(crate) struct ExtractedCameraIdentityTracker {
    current: Option<ExtractedCameraIdentity>,
    generations: ExtractedViewGenerations,
}

impl ExtractedCameraIdentityTracker {
    pub(crate) fn observe(&mut self, camera: ExtractedCameraIdentity) -> ExtractedViewGenerations {
        let pose_changed = self.current.is_none_or(|current| {
            current.stable_id != camera.stable_id || current.pose_hash != camera.pose_hash
        });
        let view_changed = pose_changed
            || self
                .current
                .is_none_or(|current| current.frustum_hash != camera.frustum_hash);
        if pose_changed {
            self.generations.pose = self.generations.pose.wrapping_add(1).max(1);
        }
        if view_changed {
            self.generations.view = self.generations.view.wrapping_add(1).max(1);
        }
        self.current = Some(camera);
        self.generations
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OpaqueDrawMode {
    Direct,
    MultiDrawIndirect,
    #[default]
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityDiagnosticSnapshot {
    pub frame_generation: u64,
    pub camera: ExtractedCameraIdentity,
    pub pose_generation: u64,
    pub view_generation: u64,
    pub resident_mesh: VisibilityKeyDigest,
    pub cave_visible: VisibilityKeyDigest,
    pub frustum_visible_opaque: VisibilityKeyDigest,
    pub submitted_opaque: Option<VisibilityKeyDigest>,
    pub resident_to_cave_loss: VisibilityKeyDigest,
    pub cave_to_frustum_loss: VisibilityKeyDigest,
    pub frustum_to_submitted_loss: Option<VisibilityKeyDigest>,
    pub draw_mode: OpaqueDrawMode,
    pub submitted_overflowed: bool,
}

pub(crate) struct VisibilityFrameProbe {
    input: VisibilityDiagnosticsInput,
    selected_view: Entity,
    camera: ExtractedCameraIdentity,
    generations: ExtractedViewGenerations,
    draw_mode: OpaqueDrawMode,
    frustum_visible_opaque: VisibilityKeyDigest,
    submitted: BTreeSet<SubChunkKey>,
    submitted_limit: usize,
    submitted_overflowed: bool,
}

impl VisibilityFrameProbe {
    #[cfg(test)]
    pub(crate) fn begin(
        input: VisibilityDiagnosticsInput,
        camera: ExtractedCameraIdentity,
        generations: ExtractedViewGenerations,
        draw_mode: OpaqueDrawMode,
        frustum_visible_opaque: VisibilityKeyDigest,
        submitted_limit: usize,
    ) -> Self {
        Self::begin_for_view(
            input,
            Entity::PLACEHOLDER,
            camera,
            generations,
            draw_mode,
            frustum_visible_opaque,
            submitted_limit,
        )
    }

    pub(crate) fn begin_for_view(
        input: VisibilityDiagnosticsInput,
        selected_view: Entity,
        camera: ExtractedCameraIdentity,
        generations: ExtractedViewGenerations,
        draw_mode: OpaqueDrawMode,
        frustum_visible_opaque: VisibilityKeyDigest,
        submitted_limit: usize,
    ) -> Self {
        Self {
            input,
            selected_view,
            camera,
            generations,
            draw_mode,
            frustum_visible_opaque,
            submitted: BTreeSet::new(),
            submitted_limit: submitted_limit.min(MAX_VISIBILITY_DIAGNOSTIC_KEYS),
            submitted_overflowed: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn record_direct(&mut self, key: SubChunkKey) -> bool {
        self.record_direct_for_view(Entity::PLACEHOLDER, key)
    }

    fn record_direct_for_view(&mut self, view: Entity, key: SubChunkKey) -> bool {
        if view != self.selected_view {
            return false;
        }
        if self.submitted.contains(&key) {
            return false;
        }
        if self.submitted.len() >= self.submitted_limit {
            self.submitted_overflowed = true;
            return false;
        }
        self.submitted.insert(key)
    }

    #[cfg(test)]
    pub(crate) fn record_mdi(&mut self, keys: impl IntoIterator<Item = SubChunkKey>) -> usize {
        self.record_mdi_for_view(Entity::PLACEHOLDER, keys)
    }

    fn record_mdi_for_view(
        &mut self,
        view: Entity,
        keys: impl IntoIterator<Item = SubChunkKey>,
    ) -> usize {
        keys.into_iter()
            .filter(|&key| self.record_direct_for_view(view, key))
            .count()
    }

    pub(crate) fn complete(self) -> VisibilityDiagnosticSnapshot {
        let submitted_opaque =
            (!self.submitted_overflowed).then(|| VisibilityKeyDigest::from_keys(self.submitted));
        VisibilityDiagnosticSnapshot {
            frame_generation: self.input.frame_generation,
            camera: self.camera,
            pose_generation: self.generations.pose,
            view_generation: self.generations.view,
            resident_mesh: self.input.resident_mesh,
            cave_visible: self.input.cave_visible,
            frustum_visible_opaque: self.frustum_visible_opaque,
            submitted_opaque,
            resident_to_cave_loss: self.input.resident_mesh.loss_to(self.input.cave_visible),
            cave_to_frustum_loss: self.input.cave_visible.loss_to(self.frustum_visible_opaque),
            frustum_to_submitted_loss: submitted_opaque
                .map(|submitted| self.frustum_visible_opaque.loss_to(submitted)),
            draw_mode: self.draw_mode,
            submitted_overflowed: self.submitted_overflowed,
        }
    }
}

#[derive(Resource, Clone, Default)]
pub struct VisibilityDiagnostics {
    inner: Arc<Mutex<VisibilityDiagnosticSnapshot>>,
}

impl VisibilityDiagnostics {
    #[must_use]
    pub fn snapshot(&self) -> VisibilityDiagnosticSnapshot {
        *self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(crate) fn publish(&self, snapshot: VisibilityDiagnosticSnapshot) -> bool {
        let mut current = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if snapshot.frame_generation < current.frame_generation {
            return false;
        }
        *current = snapshot;
        true
    }
}

#[derive(Resource, Default)]
pub(crate) struct ActiveVisibilityFrameProbe {
    active: AtomicBool,
    probe: Mutex<Option<VisibilityFrameProbe>>,
}

impl ActiveVisibilityFrameProbe {
    pub(crate) fn begin(&self, probe: VisibilityFrameProbe) {
        *self
            .probe
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(probe);
        self.active.store(true, Ordering::Release);
    }

    pub(crate) fn clear(&self) {
        self.active.store(false, Ordering::Release);
        self.probe
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
    }

    pub(crate) fn record_direct(&self, view: Entity, key: SubChunkKey) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }
        self.probe
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_mut()
            .is_some_and(|probe| probe.record_direct_for_view(view, key))
    }

    pub(crate) fn record_mdi(
        &self,
        view: Entity,
        keys: impl IntoIterator<Item = SubChunkKey>,
    ) -> usize {
        if !self.active.load(Ordering::Acquire) {
            return 0;
        }
        self.probe
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_mut()
            .map_or(0, |probe| probe.record_mdi_for_view(view, keys))
    }

    pub(crate) fn take_completed(&self) -> Option<VisibilityDiagnosticSnapshot> {
        self.active.store(false, Ordering::Release);
        self.probe
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
            .map(VisibilityFrameProbe::complete)
    }
}

pub(crate) fn hash_f32_words(words: impl IntoIterator<Item = f32>) -> u64 {
    words
        .into_iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |mut hash, word| {
            for byte in word.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            hash
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::SubChunkKey;

    fn key(x: i32) -> SubChunkKey {
        SubChunkKey::new(0, x, -4, 7)
    }

    fn camera(pose_hash: u64, frustum_hash: u64) -> ExtractedCameraIdentity {
        ExtractedCameraIdentity {
            stable_id: 41,
            pose_hash,
            frustum_hash,
        }
    }

    #[test]
    fn key_digests_are_deterministic_and_order_independent() {
        let forward = VisibilityKeyDigest::from_keys([key(-1), key(0), key(9)]);
        let reverse = VisibilityKeyDigest::from_keys([key(9), key(0), key(-1)]);

        assert_eq!(forward, reverse);
        assert_eq!(forward.count, 3);
        assert_ne!(forward.hash, 0);
    }

    #[test]
    fn empty_frame_has_zero_stage_and_loss_digests() {
        let mut input = VisibilityDiagnosticsInput::new(true);
        input.advance(
            VisibilityKeyDigest::default(),
            VisibilityKeyDigest::default(),
        );
        let mut tracker = ExtractedCameraIdentityTracker::default();
        let generations = tracker.observe(camera(10, 20));
        let probe = VisibilityFrameProbe::begin(
            input,
            camera(10, 20),
            generations,
            OpaqueDrawMode::Direct,
            VisibilityKeyDigest::default(),
            8,
        );

        let snapshot = probe.complete();

        assert_eq!(snapshot.resident_mesh, VisibilityKeyDigest::default());
        assert_eq!(snapshot.cave_visible, VisibilityKeyDigest::default());
        assert_eq!(
            snapshot.frustum_visible_opaque,
            VisibilityKeyDigest::default()
        );
        assert_eq!(
            snapshot.submitted_opaque,
            Some(VisibilityKeyDigest::default())
        );
        assert_eq!(
            snapshot.resident_to_cave_loss,
            VisibilityKeyDigest::default()
        );
        assert_eq!(
            snapshot.cave_to_frustum_loss,
            VisibilityKeyDigest::default()
        );
        assert_eq!(
            snapshot.frustum_to_submitted_loss,
            Some(VisibilityKeyDigest::default())
        );
        assert!(!snapshot.submitted_overflowed);
    }

    #[test]
    fn camera_pose_and_view_generations_advance_coherently() {
        let mut tracker = ExtractedCameraIdentityTracker::default();

        let first = tracker.observe(camera(10, 20));
        let stable = tracker.observe(camera(10, 20));
        let frustum_only = tracker.observe(camera(10, 21));
        let moved = tracker.observe(camera(11, 22));

        assert_eq!(first, ExtractedViewGenerations::new(1, 1));
        assert_eq!(stable, first);
        assert_eq!(frustum_only, ExtractedViewGenerations::new(1, 2));
        assert_eq!(moved, ExtractedViewGenerations::new(2, 3));
    }

    #[test]
    fn stale_frame_completion_cannot_replace_a_newer_snapshot() {
        let diagnostics = VisibilityDiagnostics::default();
        let newer = VisibilityDiagnosticSnapshot {
            frame_generation: 8,
            ..VisibilityDiagnosticSnapshot::default()
        };
        let stale = VisibilityDiagnosticSnapshot {
            frame_generation: 7,
            ..VisibilityDiagnosticSnapshot::default()
        };

        assert!(diagnostics.publish(newer));
        assert!(!diagnostics.publish(stale));
        assert_eq!(diagnostics.snapshot().frame_generation, 8);
    }

    #[test]
    fn direct_and_mdi_submission_paths_have_identical_key_semantics() {
        let resident = VisibilityKeyDigest::from_keys([key(1), key(2), key(3)]);
        let cave = VisibilityKeyDigest::from_keys([key(1), key(2)]);
        let frustum = VisibilityKeyDigest::from_keys([key(1), key(2)]);
        let mut input = VisibilityDiagnosticsInput::new(true);
        input.advance(resident, cave);
        let generations = ExtractedViewGenerations::new(4, 6);
        let mut direct = VisibilityFrameProbe::begin(
            input.clone(),
            camera(10, 20),
            generations,
            OpaqueDrawMode::Direct,
            frustum,
            8,
        );
        let mut mdi = VisibilityFrameProbe::begin(
            input,
            camera(10, 20),
            generations,
            OpaqueDrawMode::MultiDrawIndirect,
            frustum,
            8,
        );

        assert!(direct.record_direct(key(1)));
        assert!(direct.record_direct(key(2)));
        assert!(!direct.record_direct(key(1)));
        assert_eq!(mdi.record_mdi([key(2), key(1), key(1)]), 2);

        let direct_snapshot = direct.complete();
        let mdi_snapshot = mdi.complete();
        assert_eq!(
            direct_snapshot.submitted_opaque,
            mdi_snapshot.submitted_opaque
        );
        assert_eq!(
            direct_snapshot.frustum_to_submitted_loss,
            mdi_snapshot.frustum_to_submitted_loss
        );
        assert_eq!(direct_snapshot.draw_mode, OpaqueDrawMode::Direct);
        assert_eq!(mdi_snapshot.draw_mode, OpaqueDrawMode::MultiDrawIndirect);
    }

    #[test]
    fn key_mutation_changes_the_stage_and_loss_assertions() {
        let source = VisibilityKeyDigest::from_keys([key(1), key(2), key(3)]);
        let expected = VisibilityKeyDigest::from_keys([key(1), key(2)]);
        let mutated = VisibilityKeyDigest::from_keys([key(1), key(4)]);

        assert_ne!(expected, mutated);
        assert_ne!(source.loss_to(expected), source.loss_to(mutated));
    }

    #[test]
    fn submitted_key_overflow_is_order_independent_and_invalidates_exact_digests() {
        let all = VisibilityKeyDigest::from_keys([key(1), key(2), key(3)]);
        let snapshot_for_order = |keys| {
            let mut input = VisibilityDiagnosticsInput::new(true);
            input.advance(all, all);
            let mut probe = VisibilityFrameProbe::begin(
                input,
                camera(10, 20),
                ExtractedViewGenerations::new(1, 1),
                OpaqueDrawMode::Direct,
                all,
                2,
            );
            probe.record_mdi(keys);
            probe.complete()
        };

        let forward = snapshot_for_order([key(1), key(2), key(3)]);
        let reverse = snapshot_for_order([key(3), key(2), key(1)]);

        for snapshot in [forward, reverse] {
            assert_eq!(snapshot.submitted_opaque, None);
            assert_eq!(snapshot.frustum_to_submitted_loss, None);
            assert!(snapshot.submitted_overflowed);
        }
    }
}
