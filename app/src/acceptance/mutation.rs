use std::{
    io::Write,
    time::{Duration, Instant},
};

use render::{PresentedFrameAck, TargetRenderExpectation};
use world::SubChunkKey;

use super::{
    markers::{MOVE_PLAYER_INGRESS, MUTATION_COORDINATE, TARGET_MUTATION_ARMED, WORLD_READY},
    teleport::presented_ack_matches,
    world_ready::{WorldReadySnapshot, authoritative_publisher_radius},
};

const MUTATION_X_OFFSET_BLOCKS: i32 = 4;
const LEAF_FOREST_FAR_OFFSET_CHUNKS: i32 = 65;
const LEAF_FOREST_FAR_OFFSET_BLOCKS: i32 = LEAF_FOREST_FAR_OFFSET_CHUNKS * 16;
const LEAF_FOREST_MUTATION_Z_OFFSET_BLOCKS: i32 = 12;

#[derive(Debug, Clone)]
pub(crate) struct PendingMutation {
    pub(crate) key: SubChunkKey,
    pub(crate) observed_at: Instant,
    pub(crate) uploaded_generation: Option<u64>,
    pub(crate) expectation: Option<TargetRenderExpectation>,
}

#[derive(Debug)]
pub(crate) struct MutationTracker {
    pub(crate) coordinate: [i32; 3],
    pub(crate) armed_at: Instant,
    pub(crate) pending: Option<PendingMutation>,
    pub(crate) visible_count: u64,
    pub(crate) next_view_generation: u64,
}

impl MutationTracker {
    pub(crate) fn new(coordinate: [i32; 3]) -> Self {
        Self::armed(coordinate, Instant::now())
    }

    pub(crate) const fn armed(coordinate: [i32; 3], armed_at: Instant) -> Self {
        Self {
            coordinate,
            armed_at,
            pending: None,
            visible_count: 0,
            next_view_generation: 0,
        }
    }

    pub(crate) const fn coordinate(&self) -> [i32; 3] {
        self.coordinate
    }

    pub(crate) fn observe(&mut self, event: &protocol::WorldEvent, observed_at: Instant) -> bool {
        if observed_at < self.armed_at {
            return false;
        }
        let protocol::WorldEvent::BlockUpdates(updates) = event else {
            return false;
        };
        let Some(update) = updates
            .iter()
            .find(|update| update.position == self.coordinate)
        else {
            return false;
        };
        self.pending = Some(PendingMutation {
            key: SubChunkKey::new(
                update.dimension,
                update.position[0].div_euclid(16),
                update.position[1].div_euclid(16),
                update.position[2].div_euclid(16),
            ),
            observed_at,
            uploaded_generation: None,
            expectation: None,
        });
        true
    }

    #[cfg(test)]
    pub(crate) fn acknowledge(
        &mut self,
        key: SubChunkKey,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        self.acknowledge_upload(key, 0, dirty_since, applied_at, false)
    }

    pub(crate) fn acknowledge_upload(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
        requires_presented_frame: bool,
    ) -> Option<Duration> {
        let pending = self.pending.as_mut()?;
        if pending.key != key
            || dirty_since < pending.observed_at
            || applied_at < pending.observed_at
        {
            return None;
        }
        if requires_presented_frame {
            pending.uploaded_generation = Some(generation);
            pending.expectation = None;
            return None;
        }
        let observed_at = pending.observed_at;
        self.pending = None;
        self.visible_count = self.visible_count.saturating_add(1);
        Some(applied_at.saturating_duration_since(observed_at))
    }

    pub(crate) fn reconcile_presented_expectation(
        &mut self,
        mut proposed: TargetRenderExpectation,
        minimum_view_generation: u64,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let pending = self.pending.as_ref()?;
        let generation = pending.uploaded_generation?;
        let expected_entry = (pending.key, generation);
        if proposed.manifest.is_empty() || !proposed.manifest.contains(&expected_entry) {
            self.pending
                .as_mut()
                .expect("the mutation pending state was just observed")
                .expectation = None;
            return None;
        }
        if let Some(expectation) = &pending.expectation
            && expectation.cohort == proposed.cohort
            && expectation.source_cohort == proposed.source_cohort
            && expectation.manifest == proposed.manifest
        {
            return Some(expectation.clone());
        }

        self.next_view_generation = self
            .next_view_generation
            .max(minimum_view_generation)
            .wrapping_add(1)
            .max(1);
        proposed.view_generation = self.next_view_generation;
        proposed.render_ready_at = now;
        self.pending
            .as_mut()
            .expect("the mutation pending state was just observed")
            .expectation = Some(proposed.clone());
        Some(proposed)
    }

    pub(crate) fn observe_presented_frame(
        &mut self,
        acknowledgement: PresentedFrameAck,
    ) -> Option<Duration> {
        let pending = self.pending.as_ref()?;
        let expectation = pending.expectation.as_ref()?;
        let generation = pending.uploaded_generation?;
        if !presented_ack_matches(pending.observed_at, expectation, &acknowledgement)
            || !acknowledgement
                .drawn_manifest
                .contains(&(pending.key, generation))
        {
            return None;
        }
        let latency = acknowledgement
            .gpu_completed_at
            .checked_duration_since(pending.observed_at)?;
        self.pending = None;
        self.visible_count = self.visible_count.saturating_add(1);
        Some(latency)
    }

    pub(crate) const fn visible_count(&self) -> u64 {
        self.visible_count
    }
}

pub(crate) fn deterministic_mutation_coordinate(
    surface_eye_position: [f32; 3],
    surface_anchor: [i32; 2],
) -> [i32; 3] {
    let surface_y = surface_eye_position[1]
        .floor()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32;
    [
        surface_anchor[0].saturating_add(MUTATION_X_OFFSET_BLOCKS),
        surface_y.saturating_sub(1),
        surface_anchor[1],
    ]
}

pub(crate) fn leaf_forest_target_mutation_coordinate(
    position: [f32; 3],
    source: [i32; 3],
) -> Option<[i32; 3]> {
    let [x, _, z] = position;
    if !x.is_finite() || !z.is_finite() {
        return None;
    }
    let floor_to_i32 = |value: f32| value.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
    let target_x = floor_to_i32(x);
    let target_z = floor_to_i32(z);
    if target_x != source[0].saturating_add(LEAF_FOREST_FAR_OFFSET_BLOCKS)
        || target_z != source[2].saturating_add(LEAF_FOREST_FAR_OFFSET_BLOCKS)
    {
        return None;
    }
    Some([
        target_x,
        source[1],
        target_z.saturating_add(LEAF_FOREST_MUTATION_Z_OFFSET_BLOCKS),
    ])
}

pub(crate) fn move_player_ingress_marker(sequence: u64, position: [f32; 3]) -> Option<String> {
    let [x, y, z] = position;
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    Some(format!(
        "{MOVE_PLAYER_INGRESS} sequence={sequence} position={x},{y},{z}"
    ))
}

pub(crate) fn accepted_move_player_ingress_marker(
    accepted: bool,
    sequence: u64,
    event: &protocol::WorldEvent,
) -> Option<String> {
    if !accepted {
        return None;
    }
    let protocol::WorldEvent::MovePlayer(movement) = event else {
        return None;
    };
    move_player_ingress_marker(sequence, movement.position)
}

pub(crate) fn write_move_player_ingress_before_source_capture(
    writer: &mut impl Write,
    marker: &str,
    source_capture: impl FnOnce(),
) {
    write_stdout_marker(writer, marker);
    source_capture();
}

pub(crate) fn write_stdout_marker(writer: &mut impl Write, marker: &str) {
    let _ = writeln!(writer, "{marker}");
    let _ = writer.flush();
}

pub(crate) fn target_mutation_armed_marker(
    source: [i32; 3],
    target: [i32; 3],
    view_generation: u64,
) -> String {
    format!(
        "{TARGET_MUTATION_ARMED} source={},{},{} target={},{},{} view_generation={view_generation}",
        source[0], source[1], source[2], target[0], target[1], target[2]
    )
}

pub(crate) fn world_ready_markers(snapshot: WorldReadySnapshot) -> Option<[String; 2]> {
    let coordinate = snapshot.mutation_coordinate?;
    let publisher_radius = authoritative_publisher_radius(
        snapshot.received_radius_chunks,
        snapshot.publisher_radius_chunks,
    )?;
    if snapshot.rendered_sub_chunks == 0
        || snapshot.resident_sub_chunks == 0
        || snapshot.visible_sub_chunks == 0
        || !snapshot.mutation_target_rendered
        || !snapshot.mutation_target_visible
        || !snapshot.mutation_target_clean
        || !snapshot.work.is_empty()
    {
        return None;
    }
    Some([
        format!(
            "{MUTATION_COORDINATE}={},{},{}",
            coordinate[0], coordinate[1], coordinate[2]
        ),
        format!(
            "{WORLD_READY} radius={} rendered={} resident={} visible={}",
            publisher_radius,
            snapshot.rendered_sub_chunks,
            snapshot.resident_sub_chunks,
            snapshot.visible_sub_chunks,
        ),
    ])
}
