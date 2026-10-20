use super::*;

/// A max-heap with one removable candidate per key, so completed revisions retain no lane history.
#[derive(Default)]
pub(in crate::stream) struct CandidateQueue {
    heap: Vec<PendingSchedulerCandidate>,
    positions: HashMap<SubChunkKey, usize>,
}

impl CandidateQueue {
    /// Returns the highest-ranked queued candidate without removing it.
    pub(in crate::stream) fn peek(&self) -> Option<&PendingSchedulerCandidate> {
        self.heap.first()
    }

    /// Reports whether the lane has no candidates.
    pub(in crate::stream) fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Returns the number of distinct keys owned by this queue.
    #[cfg(test)]
    pub(in crate::stream) fn len(&self) -> usize {
        self.heap.len()
    }

    /// Visits queued candidates without promising rank order.
    #[cfg(test)]
    pub(in crate::stream) fn iter(&self) -> impl Iterator<Item = &PendingSchedulerCandidate> {
        self.heap.iter()
    }

    /// Drops all candidates while retaining reusable storage.
    pub(in crate::stream) fn clear(&mut self) {
        self.heap.clear();
        self.positions.clear();
    }

    /// Replaces a key's rank in logarithmic work, preserving urgency earned by the same revision.
    pub(in crate::stream) fn push(&mut self, mut candidate: PendingSchedulerCandidate) {
        if let Some(&index) = self.positions.get(&candidate.key) {
            if self.heap[index].revision == candidate.revision {
                candidate.urgent |= self.heap[index].urgent;
            }
            self.heap[index] = candidate;
            self.repair(index);
        } else {
            let index = self.heap.len();
            self.positions.insert(candidate.key, index);
            self.heap.push(candidate);
            self.sift_up(index);
        }
    }

    /// Removes the highest-ranked candidate and its key index.
    pub(in crate::stream) fn pop(&mut self) -> Option<PendingSchedulerCandidate> {
        let key = self.heap.first()?.key;
        self.remove(&key)
    }

    /// Retires a key without scanning unrelated work and restores the displaced candidate's rank.
    pub(in crate::stream) fn remove(
        &mut self,
        key: &SubChunkKey,
    ) -> Option<PendingSchedulerCandidate> {
        let index = self.positions.remove(key)?;
        let removed = self.heap.swap_remove(index);
        if index < self.heap.len() {
            *self
                .positions
                .get_mut(&self.heap[index].key)
                .expect("indexed candidate") = index;
            self.repair(index);
        }
        Some(removed)
    }

    /// Repairs the only heap position whose candidate may have changed.
    fn repair(&mut self, index: usize) {
        if index != 0 && self.heap[index] > self.heap[(index - 1) / 2] {
            self.sift_up(index);
        } else {
            self.sift_down(index);
        }
    }

    /// Moves a raised candidate toward the root until its parent leads it.
    fn sift_up(&mut self, mut index: usize) {
        while index != 0 {
            let parent = (index - 1) / 2;
            if self.heap[parent] >= self.heap[index] {
                break;
            }
            self.swap(index, parent);
            index = parent;
        }
    }

    /// Moves a lowered candidate toward the leaves until it leads both children.
    fn sift_down(&mut self, mut index: usize) {
        loop {
            let left = index * 2 + 1;
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.heap.len() && self.heap[right] > self.heap[left] {
                right
            } else {
                left
            };
            if self.heap[index] >= self.heap[child] {
                break;
            }
            self.swap(index, child);
            index = child;
        }
    }

    /// Exchanges two candidates and keeps their key indexes synchronized.
    fn swap(&mut self, left: usize, right: usize) {
        self.heap.swap(left, right);
        *self
            .positions
            .get_mut(&self.heap[left].key)
            .expect("indexed candidate") = left;
        *self
            .positions
            .get_mut(&self.heap[right].key)
            .expect("indexed candidate") = right;
    }
}

impl Extend<PendingSchedulerCandidate> for CandidateQueue {
    /// Adds candidates with the same per-key replacement contract as individual insertion.
    fn extend<T: IntoIterator<Item = PendingSchedulerCandidate>>(&mut self, iter: T) {
        for candidate in iter {
            self.push(candidate);
        }
    }
}

impl FromIterator<PendingSchedulerCandidate> for CandidateQueue {
    /// Builds a priority queue, coalescing each key's rank and revision-specific urgency.
    fn from_iter<T: IntoIterator<Item = PendingSchedulerCandidate>>(iter: T) -> Self {
        let mut queue = Self::default();
        queue.extend(iter);
        queue
    }
}
