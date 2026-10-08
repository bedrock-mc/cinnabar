//! Serves destination footing and its lighting dependencies before unrelated work.

use super::*;
use client_world::ingestion::PLAYER_NETWORK_OFFSET;
use world::SUB_CHUNK_SIDE;

pub(super) struct DimensionTransferPriority {
    dimension: i32,
    position: [f32; 3],
    meshes: [SubChunkKey; 2],
}

impl WorldStream {
    /// Changes work order during a transfer without changing dirty revisions,
    /// light dependencies, decode admission or publication readiness.
    pub fn set_dimension_transfer_priority(&mut self, position: Option<[f32; 3]>) {
        self.dimension_transfer_priority = position
            .and_then(|position| self.transfer_probe_position(position))
            .map(|position| DimensionTransferPriority {
                dimension: self.authority.current_dimension(),
                position,
                meshes: self
                    .transfer_mesh_keys(position)
                    .expect("finite transfer probe"),
            });
    }

    pub(super) fn transfer_mesh_keys(&self, position: [f32; 3]) -> Option<[SubChunkKey; 2]> {
        let position = self.transfer_probe_position(position)?;
        let dimension = self.authority.current_dimension();
        let side = SUB_CHUNK_SIDE as i32;
        let x = floor_to_i32(position[0]).div_euclid(side);
        let z = floor_to_i32(position[2]).div_euclid(side);
        let feet = floor_to_i32(position[1] - PLAYER_NETWORK_OFFSET);
        Some([
            SubChunkKey::new(dimension, x, feet.div_euclid(side), z),
            SubChunkKey::new(dimension, x, feet.saturating_sub(1).div_euclid(side), z),
        ])
    }

    fn transfer_priority(&self) -> Option<&DimensionTransferPriority> {
        self.dimension_transfer_priority
            .as_ref()
            .filter(|priority| priority.dimension == self.authority.current_dimension())
    }

    /// Probe every transfer poll: destination data may arrive after the camera
    /// stops moving, when ordinary scheduler refreshes no longer probe nearby.
    pub(super) fn transfer_light_candidates(&self) -> BinaryHeap<PendingSchedulerCandidate> {
        let Some(priority) = self.transfer_priority() else {
            return BinaryHeap::new();
        };
        let view = SchedulerView {
            position: priority.position,
            forward: None,
            startup_center: None,
        };
        let columns = priority
            .meshes
            .into_iter()
            .flat_map(SubChunkKey::mesh_neighbourhood_dependents)
            .map(SubChunkKey::chunk)
            .collect::<BTreeSet<_>>();
        columns
            .into_iter()
            .filter_map(|column| {
                self.near_light_column_candidate(
                    SubChunkKey::from_chunk(column, priority.meshes[0].y),
                    view,
                )
            })
            .map(|mut candidate| {
                candidate.transfer = true;
                candidate
            })
            .collect()
    }

    pub(super) fn prioritize_transfer_mesh_candidates(
        &self,
        candidates: &mut Vec<(PendingSchedulerCandidate, PendingMesh, bool)>,
    ) {
        // A deferred candidate can outlive the transfer that prioritized it.
        for (candidate, _, _) in candidates.iter_mut() {
            candidate.transfer = false;
        }
        let Some(priority) = self.transfer_priority() else {
            return;
        };
        let view = SchedulerView {
            position: priority.position,
            forward: None,
            startup_center: None,
        };
        for key in priority.meshes {
            let Some(pending) = self.mesh_jobs.pending.get(&key).copied() else {
                continue;
            };
            if !self.resident.contains(&key)
                || self.known_air.contains(&key)
                || self.mesh_jobs.in_flight.contains_key(&key)
                || !self.revisions.is_current(key, pending.revision)
            {
                continue;
            }
            let mut candidate =
                PendingSchedulerCandidate::new(key, pending.revision, view, pending.urgent);
            candidate.transfer = true;
            if let Some(existing) = candidates.iter_mut().find(|(entry, _, _)| entry.key == key) {
                existing.0 = candidate;
            } else {
                candidates.push((candidate, pending, false));
            }
        }
    }
}
