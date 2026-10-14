//! One walk of the explored tree: screen-space refinement with hysteresis, the
//! resident-column rule and 2:1 balance between face neighbours.

use crate::{
    geometry::{Face, Octant, SectionKey, SectionPos, section_blocks},
    section::{ChildMask, ChildState},
    select::{
        KeySet, ROOT_LEVEL, View, horizontal_distance_squared, projected_area,
        resident::ResidentColumns,
        tree::{ExploredTree, Node},
    },
};

/// Counters of one walk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkStats {
    /// Sections the walk looked at.
    pub visited: u64,
    /// Sections split so a face neighbour is at most one level finer.
    pub balanced: u64,
    /// Sections that would refine but stay whole until their children are read.
    pub waiting_for_children: u64,
}

/// What a walk chose.
#[derive(Debug, Clone, Default)]
pub struct Walk {
    /// The selected sections, in ascending key order; disjoint, at most one level apart
    /// across faces once every needed section is known.
    pub selected: Vec<SectionPos>,
    /// Positions the walk needed that the tree does not know or knows stale, in ascending
    /// key order: read them and walk again.
    pub unknown: Vec<SectionPos>,
    pub stats: WalkStats,
}

/// Walks with hysteresis: remembers which sections the last walk refined.
#[derive(Debug, Default)]
pub struct Selector {
    refined: KeySet,
}

impl Selector {
    /// Forgets which sections were refined, as after a teleport or dimension change.
    pub fn reset(&mut self) {
        self.refined.clear();
    }

    /// Chooses the sections to draw for `view` from what `tree` knows, refining every
    /// section over a column of `resident` to level 0, into `out`.
    pub fn walk(
        &mut self,
        view: &View,
        tree: &ExploredTree,
        resident: &ResidentColumns,
        out: &mut Walk,
    ) {
        let mut pass = Pass {
            view,
            tree,
            resident,
            refined: &self.refined,
            next_refined: KeySet::default(),
            selected: KeySet::default(),
            order: Vec::new(),
            unknown: KeySet::default(),
            stats: WalkStats::default(),
        };
        for root in roots(view) {
            pass.visit(root);
        }
        pass.balance();
        out.selected.clear();
        out.selected
            .extend(pass.selected.iter().map(|key| key.pos()));
        out.selected.sort_unstable_by_key(|pos| pos.key());
        out.unknown.clear();
        out.unknown.extend(pass.unknown.iter().map(|key| key.pos()));
        out.unknown.sort_unstable_by_key(|pos| pos.key());
        out.stats = pass.stats;
        self.refined = pass.next_refined;
    }
}

/// The [`ROOT_LEVEL`] sections whose footprint lies within the view's distance and whose
/// height overlaps the dimension's.
fn roots(view: &View) -> impl Iterator<Item = SectionPos> + '_ {
    let edge = section_blocks(ROOT_LEVEL) as f64;
    let distance = view.clamped_distance();
    let span = move |centre: f64| {
        let low = ((centre - distance) / edge).floor() as i64;
        let high = ((centre + distance) / edge).floor() as i64;
        low..=high
    };
    let low_y = (f64::from(view.min_block_y) / edge).floor() as i64;
    let high_y = (f64::from(view.max_block_y - 1) / edge).floor() as i64;
    let limit = distance * distance;
    span(view.camera[0]).flat_map(move |x| {
        span(view.camera[2]).flat_map(move |z| {
            (low_y..=high_y).filter_map(move |y| {
                let pos = SectionPos::new(
                    ROOT_LEVEL,
                    i32::try_from(x).ok()?,
                    i32::try_from(y).ok()?,
                    i32::try_from(z).ok()?,
                )
                .ok()?;
                (horizontal_distance_squared(view.camera, pos) <= limit).then_some(pos)
            })
        })
    })
}

/// The state of one walk.
struct Pass<'a> {
    view: &'a View,
    tree: &'a ExploredTree,
    resident: &'a ResidentColumns,
    /// Sections the previous walk refined: they coarsen at the lower threshold.
    refined: &'a KeySet,
    next_refined: KeySet,
    selected: KeySet,
    /// Every selection in the order made, so balance finds what a split added.
    order: Vec<SectionKey>,
    unknown: KeySet,
    stats: WalkStats,
}

impl Pass<'_> {
    /// Selects `pos`, or what replaces it below.
    fn visit(&mut self, pos: SectionPos) {
        self.stats.visited += 1;
        let Some(node) = self.tree.get(pos) else {
            self.unknown.insert(pos.key());
            return;
        };
        if self.tree.is_stale(pos) {
            self.unknown.insert(pos.key());
        }
        let Node::Present(children) = node else {
            return;
        };
        if self.resident.covers(pos) {
            // Resident chunks draw all of it; every cluster would be culled.
            return;
        }
        if pos.level() == 0 {
            self.select(pos);
            return;
        }
        let over_resident = self.resident.overlaps(pos);
        let refine = over_resident || {
            let area = projected_area(self.view.camera, self.view.focal_px, pos);
            let limit = if self.refined.contains(&pos.key()) {
                self.view.coarsen_area()
            } else {
                self.view.refine_area()
            };
            area > limit
        };
        if refine {
            self.split(pos, children, over_resident);
        } else {
            self.select(pos);
        }
    }

    /// Selects the non-empty children of `pos` within the distance instead of `pos`.
    /// While some are unread, `pos` stays selected whole, unless it overlaps a resident
    /// column (`over_resident`): then only the known children are drawn, as `pos` itself
    /// must not be.
    fn split(&mut self, pos: SectionPos, children: ChildMask, over_resident: bool) {
        let limit = self.view.clamped_distance().powi(2);
        let mut kept = [None; 8];
        let mut unread = false;
        for (slot, octant) in kept.iter_mut().zip(Octant::ALL) {
            if children.get(octant) != ChildState::NonEmpty {
                continue;
            }
            let Some(child) = pos.child(octant) else {
                continue;
            };
            if horizontal_distance_squared(self.view.camera, child) > limit {
                continue;
            }
            if self.tree.get(child).is_none() {
                self.unknown.insert(child.key());
                unread = true;
            }
            *slot = Some(child);
        }
        let none_kept = kept.iter().all(Option::is_none);
        if !over_resident && (none_kept || unread) {
            if unread {
                self.stats.waiting_for_children += 1;
            }
            self.select(pos);
            return;
        }
        self.next_refined.insert(pos.key());
        for child in kept.into_iter().flatten() {
            self.visit(child);
        }
    }

    /// Adds `pos` to the selection once, remembering the order selections were made in.
    fn select(&mut self, pos: SectionPos) {
        if self.selected.insert(pos.key()) {
            self.order.push(pos.key());
        }
    }

    /// Splits selected sections more than one level coarser than a selected face
    /// neighbour until none is, or until the children a split needs are unread.
    fn balance(&mut self) {
        let mut work: Vec<SectionKey> = self.order.clone();
        while let Some(key) = work.pop() {
            if !self.selected.contains(&key) {
                continue;
            }
            let pos = key.pos();
            for face in Face::ALL {
                let Some(across) = pos.neighbour(face) else {
                    continue;
                };
                let Some(coarse) = (2..=ROOT_LEVEL.saturating_sub(pos.level()))
                    .map_while(|up| across.ancestor(up))
                    .find(|coarse| self.selected.contains(&coarse.key()))
                else {
                    continue;
                };
                let Some(Node::Present(children)) = self.tree.get(coarse) else {
                    continue;
                };
                self.selected.remove(&coarse.key());
                let before = self.order.len();
                self.split(coarse, children, false);
                if self.selected.contains(&coarse.key()) {
                    // Its children are unread; the next walk splits it.
                    continue;
                }
                self.stats.balanced += 1;
                work.extend_from_slice(&self.order[before..]);
            }
        }
    }
}
