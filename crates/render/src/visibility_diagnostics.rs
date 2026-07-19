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
use world::{ChunkKey, SubChunkKey};

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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityKeyDelta {
    pub missing: VisibilityKeyDigest,
    pub extra: VisibilityKeyDigest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VisibilityKeySet {
    keys: BTreeSet<SubChunkKey>,
    overflowed: bool,
}

impl VisibilityKeySet {
    pub(crate) fn from_keys(keys: impl IntoIterator<Item = SubChunkKey>, limit: usize) -> Self {
        let limit = limit.min(MAX_VISIBILITY_DIAGNOSTIC_KEYS);
        let mut set = Self::default();
        for key in keys {
            if set.keys.contains(&key) {
                continue;
            }
            if set.keys.len() >= limit {
                set.overflowed = true;
                continue;
            }
            set.keys.insert(key);
        }
        set
    }

    fn digest(&self) -> Option<VisibilityKeyDigest> {
        (!self.overflowed).then(|| VisibilityKeyDigest::from_keys(self.keys.iter().copied()))
    }

    fn delta_to(&self, next: &Self) -> Option<VisibilityKeyDelta> {
        (!self.overflowed && !next.overflowed).then(|| VisibilityKeyDelta {
            missing: VisibilityKeyDigest::from_keys(self.keys.difference(&next.keys).copied()),
            extra: VisibilityKeyDigest::from_keys(next.keys.difference(&self.keys).copied()),
        })
    }

    const fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn column_count(&self, column: Option<ChunkKey>) -> Option<u32> {
        let column = column?;
        (!self.overflowed)
            .then(|| self.keys.iter().filter(|key| key.chunk() == column).count())
            .and_then(|count| u32::try_from(count).ok())
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
    witness_column: Option<ChunkKey>,
    resident_mesh: VisibilityKeySet,
    cave_visible: VisibilityKeySet,
}

impl VisibilityDiagnosticsInput {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            frame_generation: 0,
            witness_column: None,
            resident_mesh: VisibilityKeySet {
                keys: BTreeSet::new(),
                overflowed: false,
            },
            cave_visible: VisibilityKeySet {
                keys: BTreeSet::new(),
                overflowed: false,
            },
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn resident_mesh(&self) -> Option<VisibilityKeyDigest> {
        self.resident_mesh.digest()
    }

    #[must_use]
    pub fn cave_visible(&self) -> Option<VisibilityKeyDigest> {
        self.cave_visible.digest()
    }

    pub fn set_witness_column(&mut self, column: Option<ChunkKey>) {
        self.witness_column = column;
    }

    pub fn advance(
        &mut self,
        resident_mesh: impl IntoIterator<Item = SubChunkKey>,
        cave_visible: impl IntoIterator<Item = SubChunkKey>,
    ) {
        if !self.enabled {
            return;
        }
        self.frame_generation = self.frame_generation.wrapping_add(1).max(1);
        self.resident_mesh =
            VisibilityKeySet::from_keys(resident_mesh, MAX_VISIBILITY_DIAGNOSTIC_KEYS);
        self.cave_visible =
            VisibilityKeySet::from_keys(cave_visible, MAX_VISIBILITY_DIAGNOSTIC_KEYS);
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
    pub witness_column: Option<ChunkKey>,
    pub resident_witness_subchunks: Option<u32>,
    pub frustum_witness_subchunks: Option<u32>,
    pub submitted_witness_subchunks: Option<u32>,
    pub gpu_completed_witness_subchunks: Option<u32>,
    pub resident_mesh: Option<VisibilityKeyDigest>,
    pub cave_visible: Option<VisibilityKeyDigest>,
    pub frustum_visible_opaque: Option<VisibilityKeyDigest>,
    pub submitted_opaque: Option<VisibilityKeyDigest>,
    pub gpu_completed_opaque: Option<VisibilityKeyDigest>,
    pub resident_to_cave: Option<VisibilityKeyDelta>,
    pub resident_to_frustum: Option<VisibilityKeyDelta>,
    pub cave_to_frustum: Option<VisibilityKeyDelta>,
    pub frustum_to_submitted: Option<VisibilityKeyDelta>,
    pub submitted_to_gpu_completed: Option<VisibilityKeyDelta>,
    pub draw_mode: OpaqueDrawMode,
    pub resident_overflowed: bool,
    pub cave_overflowed: bool,
    pub frustum_overflowed: bool,
    pub submitted_overflowed: bool,
}

pub(crate) struct VisibilityFrameProbe {
    input: VisibilityDiagnosticsInput,
    selected_view: Entity,
    camera: ExtractedCameraIdentity,
    generations: ExtractedViewGenerations,
    draw_mode: OpaqueDrawMode,
    frustum_visible_opaque: VisibilityKeySet,
    submitted: VisibilityKeySet,
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
        frustum_visible_opaque: impl IntoIterator<Item = SubChunkKey>,
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
        frustum_visible_opaque: impl IntoIterator<Item = SubChunkKey>,
        submitted_limit: usize,
    ) -> Self {
        Self {
            input,
            selected_view,
            camera,
            generations,
            draw_mode,
            frustum_visible_opaque: VisibilityKeySet::from_keys(
                frustum_visible_opaque,
                submitted_limit,
            ),
            submitted: VisibilityKeySet::default(),
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
        if self.submitted.keys.contains(&key) {
            return false;
        }
        if self.submitted.keys.len() >= self.submitted_limit {
            self.submitted_overflowed = true;
            self.submitted.overflowed = true;
            return false;
        }
        self.submitted.keys.insert(key)
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
        let submitted_opaque = self.submitted.digest();
        let witness_column = self.input.witness_column;
        VisibilityDiagnosticSnapshot {
            frame_generation: self.input.frame_generation,
            camera: self.camera,
            pose_generation: self.generations.pose,
            view_generation: self.generations.view,
            witness_column,
            resident_witness_subchunks: self.input.resident_mesh.column_count(witness_column),
            frustum_witness_subchunks: self.frustum_visible_opaque.column_count(witness_column),
            submitted_witness_subchunks: self.submitted.column_count(witness_column),
            gpu_completed_witness_subchunks: None,
            resident_mesh: self.input.resident_mesh.digest(),
            cave_visible: self.input.cave_visible.digest(),
            frustum_visible_opaque: self.frustum_visible_opaque.digest(),
            submitted_opaque,
            gpu_completed_opaque: None,
            resident_to_cave: self.input.resident_mesh.delta_to(&self.input.cave_visible),
            resident_to_frustum: self
                .input
                .resident_mesh
                .delta_to(&self.frustum_visible_opaque),
            cave_to_frustum: self
                .input
                .cave_visible
                .delta_to(&self.frustum_visible_opaque),
            frustum_to_submitted: self.frustum_visible_opaque.delta_to(&self.submitted),
            submitted_to_gpu_completed: None,
            draw_mode: self.draw_mode,
            resident_overflowed: self.input.resident_mesh.overflowed(),
            cave_overflowed: self.input.cave_visible.overflowed(),
            frustum_overflowed: self.frustum_visible_opaque.overflowed(),
            submitted_overflowed: self.submitted_overflowed,
        }
    }
}

impl VisibilityDiagnosticSnapshot {
    #[must_use]
    pub(crate) fn gpu_completed(mut self) -> Self {
        self.gpu_completed_opaque = self.submitted_opaque;
        self.gpu_completed_witness_subchunks = self.submitted_witness_subchunks;
        self.submitted_to_gpu_completed =
            self.submitted_opaque.map(|_| VisibilityKeyDelta::default());
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphicsAdapterMetadata {
    pub backend: String,
    pub adapter: String,
    pub driver: String,
    pub driver_info: String,
    pub requested_present_mode: String,
    pub effective_present_mode: String,
    pub present_mode_proven: bool,
}

#[derive(Debug, Default)]
struct VisibilityDiagnosticsState {
    snapshot: VisibilityDiagnosticSnapshot,
    graphics_adapter: Option<GraphicsAdapterMetadata>,
}

#[derive(Resource, Clone, Default)]
pub struct VisibilityDiagnostics {
    inner: Arc<Mutex<VisibilityDiagnosticsState>>,
}

impl VisibilityDiagnostics {
    #[must_use]
    pub fn snapshot(&self) -> VisibilityDiagnosticSnapshot {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .snapshot
    }

    #[must_use]
    pub fn graphics_adapter(&self) -> Option<GraphicsAdapterMetadata> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .graphics_adapter
            .clone()
    }

    pub(crate) fn publish(&self, snapshot: VisibilityDiagnosticSnapshot) -> bool {
        let mut current = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if snapshot.frame_generation < current.snapshot.frame_generation {
            return false;
        }
        current.snapshot = snapshot;
        true
    }

    pub(crate) fn publish_graphics_adapter(&self, metadata: GraphicsAdapterMetadata) -> bool {
        let mut current = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if current.graphics_adapter.is_some() {
            return false;
        }
        current.graphics_adapter = Some(metadata);
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
        input.advance([], []);
        let mut tracker = ExtractedCameraIdentityTracker::default();
        let generations = tracker.observe(camera(10, 20));
        let probe = VisibilityFrameProbe::begin(
            input,
            camera(10, 20),
            generations,
            OpaqueDrawMode::Direct,
            [],
            8,
        );

        let snapshot = probe.complete();

        assert_eq!(snapshot.resident_mesh, Some(VisibilityKeyDigest::default()));
        assert_eq!(snapshot.cave_visible, Some(VisibilityKeyDigest::default()));
        assert_eq!(
            snapshot.frustum_visible_opaque,
            Some(VisibilityKeyDigest::default())
        );
        assert_eq!(
            snapshot.submitted_opaque,
            Some(VisibilityKeyDigest::default())
        );
        assert_eq!(
            snapshot.resident_to_cave,
            Some(VisibilityKeyDelta::default())
        );
        assert_eq!(
            snapshot.cave_to_frustum,
            Some(VisibilityKeyDelta::default())
        );
        assert_eq!(
            snapshot.frustum_to_submitted,
            Some(VisibilityKeyDelta::default())
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
    fn graphics_adapter_metadata_is_available_to_acceptance_without_affecting_snapshots() {
        let diagnostics = VisibilityDiagnostics::default();
        let metadata = GraphicsAdapterMetadata {
            backend: "Dx12".to_owned(),
            adapter: "Test Adapter".to_owned(),
            driver: "test-driver".to_owned(),
            driver_info: "1.2.3".to_owned(),
            requested_present_mode: "Fifo".to_owned(),
            effective_present_mode: "Fifo".to_owned(),
            present_mode_proven: true,
        };

        assert!(diagnostics.publish_graphics_adapter(metadata.clone()));
        assert!(
            !diagnostics.publish_graphics_adapter(GraphicsAdapterMetadata {
                backend: "Vulkan".to_owned(),
                ..metadata.clone()
            })
        );
        assert_eq!(diagnostics.graphics_adapter(), Some(metadata));
        assert_eq!(
            diagnostics.snapshot(),
            VisibilityDiagnosticSnapshot::default()
        );
    }

    #[test]
    fn direct_and_mdi_submission_paths_have_identical_key_semantics() {
        let resident = [key(1), key(2), key(3)];
        let cave = [key(1), key(2)];
        let frustum = [key(1), key(2)];
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
            direct_snapshot.frustum_to_submitted,
            mdi_snapshot.frustum_to_submitted
        );
        assert_eq!(
            direct_snapshot.view_generation,
            mdi_snapshot.view_generation
        );
        assert_eq!(direct_snapshot.draw_mode, OpaqueDrawMode::Direct);
        assert_eq!(mdi_snapshot.draw_mode, OpaqueDrawMode::MultiDrawIndirect);
    }

    #[test]
    fn player_column_witness_tracks_exact_stage_membership() {
        let column = ChunkKey::new(0, 3, 7);
        let resident = [
            SubChunkKey::new(0, 3, -4, 7),
            SubChunkKey::new(0, 3, -3, 7),
            SubChunkKey::new(0, 4, -4, 7),
        ];
        let mut input = VisibilityDiagnosticsInput::new(true);
        input.set_witness_column(Some(column));
        input.advance(resident, resident);
        let mut probe = VisibilityFrameProbe::begin(
            input,
            camera(10, 20),
            ExtractedViewGenerations::new(1, 1),
            OpaqueDrawMode::Direct,
            [resident[0], resident[2]],
            8,
        );
        assert!(probe.record_direct(resident[0]));

        let submitted = probe.complete();
        assert_eq!(submitted.witness_column, Some(column));
        assert_eq!(submitted.resident_witness_subchunks, Some(2));
        assert_eq!(submitted.frustum_witness_subchunks, Some(1));
        assert_eq!(submitted.submitted_witness_subchunks, Some(1));
        assert_eq!(submitted.gpu_completed_witness_subchunks, None);

        let presented = submitted.gpu_completed();
        assert_eq!(presented.gpu_completed_witness_subchunks, Some(1));
    }

    #[test]
    fn key_mutation_changes_the_stage_and_loss_assertions() {
        let source = VisibilityKeySet::from_keys([key(1), key(2), key(3)], 8);
        let expected = VisibilityKeySet::from_keys([key(1), key(2)], 8);
        let mutated = VisibilityKeySet::from_keys([key(1), key(4)], 8);

        assert_ne!(expected, mutated);
        assert_ne!(source.delta_to(&expected), source.delta_to(&mutated));
    }

    #[test]
    fn disjoint_moving_sets_cannot_report_zero_missing_keys() {
        let source = VisibilityKeySet::from_keys([key(1)], 8);
        let next = VisibilityKeySet::from_keys([key(2), key(3)], 8);

        let delta = source.delta_to(&next).unwrap();

        assert_eq!(delta.missing, VisibilityKeyDigest::from_keys([key(1)]));
        assert_eq!(
            delta.extra,
            VisibilityKeyDigest::from_keys([key(2), key(3)])
        );
    }

    #[test]
    fn submitted_key_overflow_is_order_independent_and_invalidates_exact_digests() {
        let all = [key(1), key(2), key(3)];
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
            assert_eq!(snapshot.frustum_to_submitted, None);
            assert!(snapshot.submitted_overflowed);
        }
    }

    #[test]
    fn bounded_stage_overflow_invalidates_exact_deltas() {
        let source = VisibilityKeySet::from_keys([key(1), key(2)], 1);
        let next = VisibilityKeySet::from_keys([key(1)], 1);

        assert!(source.overflowed());
        assert_eq!(source.digest(), None);
        assert_eq!(source.delta_to(&next), None);
    }

    #[test]
    fn direct_and_mdi_gpu_completion_keep_the_same_view_generation_and_keys() {
        let snapshot_for_mode = |draw_mode| {
            let keys = [key(1), key(2)];
            let mut input = VisibilityDiagnosticsInput::new(true);
            input.advance(keys, keys);
            let mut probe = VisibilityFrameProbe::begin(
                input,
                camera(10, 20),
                ExtractedViewGenerations::new(4, 9),
                draw_mode,
                keys,
                8,
            );
            match draw_mode {
                OpaqueDrawMode::Direct => {
                    for key in keys {
                        assert!(probe.record_direct(key));
                    }
                }
                OpaqueDrawMode::MultiDrawIndirect => {
                    assert_eq!(probe.record_mdi(keys), keys.len());
                }
                OpaqueDrawMode::Unsupported => unreachable!("test covers supported draw modes"),
            }
            probe.complete().gpu_completed()
        };

        let direct = snapshot_for_mode(OpaqueDrawMode::Direct);
        let mdi = snapshot_for_mode(OpaqueDrawMode::MultiDrawIndirect);

        assert_eq!(direct.view_generation, 9);
        assert_eq!(direct.view_generation, mdi.view_generation);
        assert_eq!(direct.submitted_opaque, direct.gpu_completed_opaque);
        assert_eq!(direct.gpu_completed_opaque, mdi.gpu_completed_opaque);
        assert_eq!(
            direct.submitted_to_gpu_completed,
            Some(VisibilityKeyDelta::default())
        );
        assert_eq!(
            direct.submitted_to_gpu_completed,
            mdi.submitted_to_gpu_completed
        );
    }
}
