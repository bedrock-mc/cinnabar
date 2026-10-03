use super::*;

pub(super) const NEAR_CAMERA_RADIUS: i32 = 4;

/// Re-ranks a bounded part of existing queues after the view changes.
pub(super) struct SchedulerRefresh<const N: usize> {
    previous: [BinaryHeap<PendingSchedulerCandidate>; N],
    view: Option<SchedulerView>,
}

impl<const N: usize> Default for SchedulerRefresh<N> {
    fn default() -> Self {
        Self {
            previous: std::array::from_fn(|_| BinaryHeap::new()),
            view: None,
        }
    }
}

impl<const N: usize> SchedulerRefresh<N> {
    /// Refreshes bounded queue work and reports whether the view still needs direct local probes.
    pub(super) fn refresh(
        &mut self,
        view: SchedulerView,
        mut queues: [&mut BinaryHeap<PendingSchedulerCandidate>; N],
        deadline: Option<Instant>,
        is_current: impl Fn(SubChunkKey, u64) -> bool,
    ) -> bool {
        let moved = self
            .view
            .is_none_or(|previous| previous.cell() != view.cell());
        if self.previous.iter().all(BinaryHeap::is_empty) && moved {
            for (old, current) in self.previous.iter_mut().zip(&mut queues) {
                std::mem::swap(old, current);
            }
            self.view = Some(view);
        }
        let probe_near = moved || self.previous.iter().any(|queue| !queue.is_empty());
        let mut refreshed = false;
        for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
            if refreshed && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }
            let Some(index) = self.previous.iter().position(|queue| !queue.is_empty()) else {
                break;
            };
            let mut candidate = self.previous[index].pop().expect("nonempty refresh queue");
            if is_current(candidate.key, candidate.revision) {
                candidate.distance_squared = view.rank(candidate.key);
                queues[index].push(candidate);
                refreshed = true;
            }
        }
        probe_near
    }
}

/// Probes the camera's immediate sub-chunk neighbourhood without scanning pending maps.
pub(super) fn near_camera_keys(
    view: SchedulerView,
    dimension: i32,
) -> impl Iterator<Item = SubChunkKey> {
    let [x, y, z] = view
        .position
        .map(|value| floor_to_i32(value).div_euclid(16));
    (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).flat_map(move |dx| {
        (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).flat_map(move |dy| {
            (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).filter_map(move |dz| {
                Some(SubChunkKey::new(
                    dimension,
                    x.checked_add(dx)?,
                    y.checked_add(dy)?,
                    z.checked_add(dz)?,
                ))
            })
        })
    })
}

/// Includes the horizontal light halo supporting the directly prioritized mesh area.
pub(super) fn near_light_columns(
    view: SchedulerView,
    dimension: i32,
) -> impl Iterator<Item = SubChunkKey> {
    let radius = NEAR_CAMERA_RADIUS + 1;
    let [x, y, z] = view
        .position
        .map(|value| floor_to_i32(value).div_euclid(16));
    (-radius..=radius).flat_map(move |dx| {
        (-radius..=radius).filter_map(move |dz| {
            Some(SubChunkKey::new(
                dimension,
                x.checked_add(dx)?,
                y,
                z.checked_add(dz)?,
            ))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Once queue priorities match a stationary camera, local probes add no new information.
    #[test]
    fn stationary_view_does_not_repeat_local_probes() {
        let view = SchedulerView {
            position: [0.0; 3],
            forward: None,
        };
        let mut queue = BinaryHeap::new();
        let mut refresh = SchedulerRefresh::<1>::default();
        assert!(refresh.refresh(view, [&mut queue], None, |_, _| true));
        assert!(!refresh.refresh(view, [&mut queue], None, |_, _| true));
    }

    #[test]
    fn camera_refresh_is_bounded_and_preserves_every_current_record() {
        let view = SchedulerView {
            position: [0.0; 3],
            forward: None,
        };
        let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 4;
        let mut ready = (0..count)
            .map(|x| {
                PendingSchedulerCandidate::new(SubChunkKey::new(0, x as i32, 0, 0), 1, view, false)
            })
            .collect::<BinaryHeap<_>>();
        let mut refresh = SchedulerRefresh::<1>::default();
        refresh.refresh(view, [&mut ready], None, |_, _| true);
        assert_eq!(ready.len(), MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        for _ in 1..4 {
            refresh.refresh(view, [&mut ready], None, |_, _| true);
        }
        assert_eq!(ready.len(), count);
        assert!(refresh.previous[0].is_empty());
        let turned = SchedulerView {
            position: [16_384.0, 0.0, 0.0],
            forward: None,
        };
        refresh.refresh(turned, [&mut ready], None, |key, _| key.x % 2 == 0);
        assert!(ready.len() <= MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        for _ in 1..4 {
            refresh.refresh(turned, [&mut ready], None, |key, _| key.x % 2 == 0);
        }
        assert_eq!(ready.len(), count / 2);
    }
}
