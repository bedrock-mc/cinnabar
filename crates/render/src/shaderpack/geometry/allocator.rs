//! First-fit face ranges in the expanded vertex arena.

use std::collections::BTreeMap;
use std::ops::Range;

/// Free ranges of a fixed-size face arena, coalesced on release.
#[derive(Debug, Default)]
pub(crate) struct FaceAllocator {
    /// Free ranges by start, never adjacent to each other.
    free: BTreeMap<u32, u32>,
    capacity: u32,
    /// Faces in `free`.
    free_faces: u32,
}

impl FaceAllocator {
    /// An arena of `capacity` faces, all free.
    pub(crate) fn new(capacity: u32) -> Self {
        let mut faces = Self {
            free: BTreeMap::new(),
            capacity,
            free_faces: 0,
        };
        faces.release(0..capacity);
        faces
    }

    pub(crate) fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Faces allocated.
    pub(crate) fn used(&self) -> u32 {
        self.capacity - self.free_faces
    }

    /// The lowest free range of `len` faces, or `None` when none fits.
    pub(crate) fn allocate(&mut self, len: u32) -> Option<Range<u32>> {
        if len == 0 {
            return Some(0..0);
        }
        let (&start, &end) = self.free.iter().find(|(start, end)| *end - *start >= len)?;
        self.free.remove(&start);
        if end - start > len {
            self.free.insert(start + len, end);
        }
        self.free_faces -= len;
        Some(start..start + len)
    }

    /// Takes exactly `range` when all of it is free; returns whether it did.
    pub(crate) fn allocate_at(&mut self, range: Range<u32>) -> bool {
        if range.is_empty() {
            return true;
        }
        let Some((&start, &end)) = self.free.range(..=range.start).next_back() else {
            return false;
        };
        if end < range.end {
            return false;
        }
        self.free.remove(&start);
        if start < range.start {
            self.free.insert(start, range.start);
        }
        if range.end < end {
            self.free.insert(range.end, end);
        }
        self.free_faces -= range.end - range.start;
        true
    }

    /// Returns `range` to the free list, merging it with free neighbours.
    pub(crate) fn release(&mut self, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        self.free_faces += range.end - range.start;
        let mut start = range.start;
        let mut end = range.end;
        if let Some((&before, &before_end)) = self.free.range(..start).next_back()
            && before_end == start
        {
            self.free.remove(&before);
            start = before;
        }
        if let Some(after_end) = self.free.remove(&end) {
            end = after_end;
        }
        self.free.insert(start, end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_never_overlap_and_released_neighbours_coalesce() {
        let mut faces = FaceAllocator::new(10);
        assert_eq!(faces.allocate(0), Some(0..0));
        let a = faces.allocate(4).unwrap();
        let b = faces.allocate(3).unwrap();
        let c = faces.allocate(3).unwrap();
        assert_eq!((a.clone(), b.clone(), c.clone()), (0..4, 4..7, 7..10));
        assert_eq!(faces.allocate(1), None);
        assert_eq!(faces.used(), 10);
        faces.release(a);
        faces.release(c);
        assert_eq!(faces.used(), 3);
        assert_eq!(faces.allocate(5), None, "free space is split around b");
        faces.release(b);
        assert_eq!(faces.used(), 0);
        assert_eq!(
            faces.allocate(10),
            Some(0..10),
            "released ranges coalesce whole"
        );
    }

    #[test]
    fn exact_ranges_are_taken_only_when_wholly_free() {
        let mut faces = FaceAllocator::new(10);
        assert!(faces.allocate_at(3..6));
        assert!(!faces.allocate_at(5..8), "overlaps the taken range");
        assert_eq!(faces.allocate(3), Some(0..3));
        assert_eq!(faces.allocate(4), Some(6..10));
        assert_eq!(faces.used(), 10);
    }
}
