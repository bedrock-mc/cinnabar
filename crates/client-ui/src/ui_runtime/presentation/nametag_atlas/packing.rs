//! Shelf placement, height recovery and reclamation of unused nametag cells.

use super::*;

impl NametagAtlas {
    /// Reserves a non-overlapping span in an existing shelf or appends a new shelf.
    pub(super) fn allocate(&mut self, width: u32, height: u32) -> Option<(usize, [u32; 2])> {
        if width > NAMETAG_ATLAS_SIDE || height > NAMETAG_ATLAS_SIDE {
            return None;
        }
        let available = self.shelves.iter().enumerate().find_map(|(index, shelf)| {
            let span = shelf
                .free
                .iter()
                .position(|span| span.end - span.start >= width)?;
            if shelf.height < height {
                let limit = self.shelves[index + 1..]
                    .iter()
                    .find(|next| next.height != 0 && !spans_whole_row(&next.free))
                    .map_or(NAMETAG_ATLAS_SIDE, |next| next.y);
                if shelf.y + height > limit {
                    return None;
                }
            }
            Some((index, span))
        });
        if let Some((index, span)) = available {
            self.grow_shelf(index, height);
            let index = self.trim_shelf(index, height);
            let shelf = &mut self.shelves[index];
            let x = shelf.free[span].start;
            shelf.free[span].start += width;
            return Some((index, [x, shelf.y]));
        }
        let y = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.y + shelf.height);
        if y + height > NAMETAG_ATLAS_SIDE {
            let mut used_heights = vec![0_u32; self.shelves.len()];
            for cell in &self.cells {
                used_heights[cell.shelf] = used_heights[cell.shelf].max(cell.allocation[3]);
            }
            if let Some(index) = self.shelves.iter().enumerate().position(|(index, shelf)| {
                used_heights[index] != 0 && shelf.height - used_heights[index] >= height
            }) {
                self.trim_shelf(index, used_heights[index]);
                // The released tail guarantees this retry a full-width row of sufficient height.
                return self.allocate(width, height);
            }
            self.exhausted = true;
            return None;
        }
        let index = self.shelves.len();
        self.shelves.push(AtlasShelf {
            y,
            height,
            free: free_from(width),
        });
        Some((index, [0, y]))
    }

    /// Extends a shelf through empty rows or the unallocated tail without moving cached cells.
    /// Allocation has already checked that no occupied lower row intersects the new height.
    fn grow_shelf(&mut self, index: usize, height: u32) {
        if height <= self.shelves[index].height {
            return;
        }
        self.shelves[index].height = height;
        let end_y = self.shelves[index].y + height;
        for next in &mut self.shelves[index + 1..] {
            if next.y >= end_y {
                break;
            }
            let old_end = next.y + next.height;
            next.y = end_y;
            next.height = old_end.saturating_sub(end_y);
            if next.height == 0 {
                next.free.clear();
            }
        }
        self.reclamation = None;
    }

    /// Releases a shelf's unused vertical tail while keeping every cached cell in place.
    /// Returns its index after removing merged slots and inserting the free tail.
    fn trim_shelf(&mut self, index: usize, height: u32) -> usize {
        if height == self.shelves[index].height {
            return index;
        }
        let used_height = self
            .cells
            .iter()
            .filter(|cell| cell.shelf == index)
            .map(|cell| cell.allocation[3])
            .max()
            .unwrap_or(0);
        let height = height.max(used_height);
        if height == self.shelves[index].height {
            return index;
        }
        let mut remap = Vec::with_capacity(self.shelves.len());
        let (mut next, mut trimmed) = (0, 0);
        for (old, shelf) in self.shelves.iter().enumerate() {
            remap.push(next);
            if old == index {
                trimmed = next;
            }
            if shelf.height != 0 {
                next += 1 + usize::from(old == index);
            }
        }
        self.shelves.retain(|shelf| shelf.height != 0);
        let shelf = &mut self.shelves[trimmed];
        let tail = AtlasShelf {
            y: shelf.y + height,
            height: shelf.height - height,
            free: free_from(0),
        };
        shelf.height = height;
        self.shelves.insert(trimmed + 1, tail);
        for cell in &mut self.cells {
            cell.shelf = remap[cell.shelf];
        }
        self.reclamation = None;
        trimmed
    }

    /// Reclaims unused lines from one shelf, or adjoining empty shelves for taller ink.
    /// A missing size only asks for a free line slot, independently of pixel placement.
    pub(super) fn reclaim_shelf(&mut self, size: Option<[u32; 2]>) -> bool {
        if self.requested.is_empty() {
            // Retained debug text has no frame-wide liveness set and must keep its cells.
            return false;
        }
        if self.reclamation.is_none() {
            self.reclamation = Some(self.unused_shelf_space());
        }
        let candidates = self.reclamation.as_ref().expect("prepared reclamation");
        let candidate = candidates
            .iter()
            .find(|candidate| match size {
                None => candidate.has_unused_lines,
                Some([width, height]) => {
                    let shelf_height = self.shelves[candidate.shelves.start].height;
                    shelf_height >= height
                        && (candidate
                            .free
                            .iter()
                            .any(|span| span.end - span.start >= width)
                            || shelf_height - candidate.retained_height >= height)
                }
            })
            .cloned()
            .or_else(|| {
                let [_, height] = size?;
                // Several wholly unused neighboring shelves can become one taller shelf.
                for first in candidates {
                    let start = first.shelves.start;
                    let mut end = start;
                    let mut available = 0;
                    let mut has_unused_lines = false;
                    for next in candidates.iter().filter(|next| next.shelves.start >= start) {
                        if self.shelves[end..next.shelves.start]
                            .iter()
                            .any(|shelf| shelf.height != 0)
                            || !spans_whole_row(&next.free)
                        {
                            break;
                        }
                        end = next.shelves.start;
                        available += self.shelves[end].height;
                        has_unused_lines |= next.has_unused_lines;
                        end += 1;
                        if available >= height {
                            return Some(ShelfReclamation {
                                shelves: start..end,
                                free: free_from(0),
                                retained_height: 0,
                                has_unused_lines,
                            });
                        }
                    }
                }
                None
            });
        let Some(candidate) = candidate else {
            return false;
        };
        let shelf = candidate.shelves.start;
        let mut removed_lines = false;
        let mut index = 0;
        while index < self.cells.len() {
            let cell = &self.cells[index];
            if !candidate.shelves.contains(&cell.shelf)
                || cell
                    .text
                    .as_ref()
                    .is_none_or(|text| self.requested.contains(text))
            {
                index += 1;
                continue;
            }
            let removed = self.cells.swap_remove(index);
            removed_lines = true;
            self.lines
                .remove(removed.text.as_ref().expect("evicted text cell"));
            if let Some(moved) = self.cells.get(index).and_then(|cell| cell.text.as_ref()) {
                *self.lines.get_mut(moved).expect("retained cell") = index;
            }
        }
        let end_y = self.shelves[candidate.shelves.end - 1].y
            + self.shelves[candidate.shelves.end - 1].height;
        self.shelves[shelf].height = end_y - self.shelves[shelf].y;
        self.shelves[shelf].free = candidate.free;
        for index in shelf + 1..candidate.shelves.end {
            self.shelves[index].y = end_y;
            self.shelves[index].height = 0;
            self.shelves[index].free.clear();
        }
        self.reclamation = None;
        self.exhausted = false;
        if removed_lines {
            self.published = None;
        }
        true
    }

    /// Plans reusable shelf spans in one pass over the bounded cache, then sorts each shelf.
    fn unused_shelf_space(&self) -> Vec<ShelfReclamation> {
        let mut occupied = vec![Vec::new(); self.shelves.len()];
        let mut unused = vec![false; self.shelves.len()];
        for cell in &self.cells {
            if cell
                .text
                .as_ref()
                .is_none_or(|text| self.requested.contains(text))
            {
                occupied[cell.shelf].push(cell.allocation);
            } else {
                unused[cell.shelf] = true;
            }
        }
        occupied
            .into_iter()
            .enumerate()
            .filter_map(|(shelf, mut cells)| {
                // Empty shelves still contribute contiguous height to spatial reclamation.
                if self.shelves[shelf].height == 0 || (!unused[shelf] && !cells.is_empty()) {
                    return None;
                }
                cells.sort_unstable_by_key(|cell| cell[0]);
                let retained_height = cells.iter().map(|cell| cell[3]).max().unwrap_or(0);
                let mut free = Vec::new();
                let mut x = 0;
                for [left, _, width, _] in cells {
                    if x < left {
                        free.push(x..left);
                    }
                    x = left + width;
                }
                if x < NAMETAG_ATLAS_SIDE {
                    free.push(x..NAMETAG_ATLAS_SIDE);
                }
                Some(ShelfReclamation {
                    shelves: shelf..shelf + 1,
                    free,
                    retained_height,
                    has_unused_lines: unused[shelf],
                })
            })
            .collect()
    }
}

/// Free spans of a shelf whose cells from `start` to the atlas edge are unused.
fn free_from(start: u32) -> Vec<Range<u32>> {
    let span = start..NAMETAG_ATLAS_SIDE;
    vec![span]
}

/// Whether free spans leave every cell of the shelf unused.
fn spans_whole_row(free: &[Range<u32>]) -> bool {
    matches!(free, [span] if *span == (0..NAMETAG_ATLAS_SIDE))
}
