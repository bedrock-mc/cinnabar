use super::*;
use crate::chunk::queue::{RetainedUploadPriorities, upload_distance_key};
use std::cmp::Reverse;

type UploadOrder = (Reverse<bool>, i128, Reverse<bool>, u32, SubChunkKey);

/// Keeps distance buckets across frames while aging only changes scheduling tiers.
#[derive(Default)]
pub(super) struct GpuUpdatePriorities {
    camera: Option<[u32; 3]>,
    order: RetainedUploadPriorities<Entity, UploadOrder>,
    #[cfg(test)]
    distance_evaluations: usize,
}

/// Returns the original total order, retaining unchanged buckets and camera distances.
pub(super) fn plan_updates(
    candidates: Vec<GpuUpdateCandidate>,
    allocations: &HashMap<Entity, ArenaAllocation>,
    camera_position: Vec3,
    active_tint_identity: ChunkBiomeTintIdentity,
    fairness: &GpuUpdateFairness,
) -> Vec<Entity> {
    let mut priorities = fairness.priorities.lock().unwrap();
    let camera = camera_position.to_array().map(f32::to_bits);
    let camera_unchanged = priorities.camera == Some(camera);
    priorities.camera = Some(camera);
    #[cfg(test)]
    {
        priorities.distance_evaluations = 0;
    }
    priorities.order.begin();
    for candidate in candidates {
        if !chunk_tint_identity_is_active(candidate.tint_identity, active_tint_identity)
            || allocations
                .get(&candidate.entity)
                .is_some_and(|allocation| {
                    allocation.generation == candidate.generation
                        && allocation.tint_identity == candidate.tint_identity
                })
        {
            continue;
        }
        let cached = priorities
            .order
            .priority(candidate.entity)
            .filter(|order| camera_unchanged && order.4 == candidate.key);
        let distance = if let Some(order) = cached {
            order.3
        } else {
            #[cfg(test)]
            {
                priorities.distance_evaluations += 1;
            }
            upload_distance_key(
                ChunkUploadPriority::from_camera(candidate.key, camera_position).distance_squared(),
            )
        };
        let age = fairness.wait_age(candidate.entity);
        // Birth frames retain oldest-first order without rekeying every starved waiter each frame.
        // Saturated ages tie, just as they do in the original age comparison.
        let birth = if age == u32::MAX {
            i128::MIN
        } else if age >= GPU_UPDATE_STARVED_FRAMES {
            i128::from(fairness.priority_frame) - i128::from(age)
        } else {
            i128::MAX
        };
        priorities.order.update(
            candidate.entity,
            (
                Reverse(candidate.priority.is_urgent() || fairness.is_urgent(candidate.entity)),
                birth,
                Reverse(age >= GPU_UPDATE_OVERDUE_FRAMES),
                distance,
                candidate.key,
            ),
        );
    }
    priorities.order.finish();
    priorities.order.ordered().collect()
}

#[cfg(test)]
mod tests;
