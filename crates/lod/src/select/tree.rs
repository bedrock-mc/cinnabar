//! What a walk knows of the stored section tree: for each section read so far, whether a
//! record holds it and its children's tri-state existence. The owner reads the record
//! headers a walk asks for ([`crate::codec::RecordHeader`]) and marks sections written
//! again stale, so the next walk reads them afresh while still using what it knew.
//!
//! A tree may be given a hard limit of known sections: reads past it are refused, and
//! [`ExploredTree::trim`] forgets the farthest sections to make room again.

use crate::{
    codec::RecordHeader,
    geometry::{SectionKey, SectionPos},
    section::ChildMask,
    select::{KeyMap, horizontal_distance_squared},
};

/// What a store read found at one position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Node {
    /// No usable record: none stored, or a corrupt one.
    Missing,
    /// A record with this child mask.
    Present(ChildMask),
}

impl Node {
    /// The node a read of a record found; `None` for no record or one that does not
    /// validate.
    #[must_use]
    pub fn of_record(record: Option<&[u8]>) -> Self {
        record
            .and_then(|record| RecordHeader::read(record).ok())
            .map_or(Self::Missing, |header| Self::Present(header.children))
    }
}

/// Heap bytes one known section takes at most: its entry and control byte at the hash
/// table's lowest load after growing.
pub const NODE_BYTES: usize = 2 * (size_of::<(SectionKey, Entry)>() + 1);

/// What is known at one position, and whether it may be out of date.
#[derive(Debug, Clone, Copy)]
struct Entry {
    node: Node,
    stale: bool,
}

/// See the module documentation.
#[derive(Debug, Clone)]
pub struct ExploredTree {
    nodes: KeyMap<Entry>,
    limit: usize,
}

impl Default for ExploredTree {
    /// A tree without a limit.
    fn default() -> Self {
        Self::with_limit(usize::MAX)
    }
}

impl ExploredTree {
    /// An empty tree that never knows more than `limit` sections.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            nodes: KeyMap::default(),
            limit,
        }
    }

    /// The most sections the tree knows at once.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// What is known at `pos`, possibly out of date; `None` until it was read.
    #[must_use]
    pub fn get(&self, pos: SectionPos) -> Option<Node> {
        self.nodes.get(&pos.key()).map(|entry| entry.node)
    }

    /// True when `pos` is known but was written or shown differently since it was read.
    #[must_use]
    pub fn is_stale(&self, pos: SectionPos) -> bool {
        self.nodes.get(&pos.key()).is_some_and(|entry| entry.stale)
    }

    /// Records what a read of `key` found; false, leaving `key` unknown, when the tree
    /// already knows [`Self::limit`] other sections.
    pub fn insert(&mut self, key: SectionKey, node: Node) -> bool {
        if self.nodes.len() >= self.limit && !self.nodes.contains_key(&key) {
            return false;
        }
        self.nodes.insert(key, Entry { node, stale: false });
        true
    }

    /// Marks `key` out of date, as after a write to it: walks keep using what is known
    /// and ask for it to be read again, so drawn terrain never drops out while it is.
    pub fn mark_stale(&mut self, key: SectionKey) {
        if let Some(entry) = self.nodes.get_mut(&key) {
            entry.stale = true;
        }
    }

    /// Marks every known position `touched` names out of date.
    pub fn mark_stale_where(&mut self, mut touched: impl FnMut(SectionPos) -> bool) {
        for (key, entry) in &mut self.nodes {
            if touched(key.pos()) {
                entry.stale = true;
            }
        }
    }

    /// Forgets everything, as in another dimension.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Forgets positions whose footprint lies farther than `distance` blocks horizontally
    /// from `camera`, bounding what the tree holds as the player travels.
    pub fn retain_near(&mut self, camera: [f64; 3], distance: f64) {
        let limit = distance * distance;
        self.nodes
            .retain(|key, _| horizontal_distance_squared(camera, key.pos()) <= limit);
    }

    /// Forgets positions farther than `distance` blocks horizontally from `camera`, then the
    /// farthest others until at most `keep` remain. Of positions equally far, finer ones go
    /// first, so a known section's ancestors, never farther than it, stay known.
    pub fn trim(&mut self, camera: [f64; 3], distance: f64, keep: usize) {
        self.retain_near(camera, distance);
        if self.nodes.len() <= keep {
            return;
        }
        let mut order: Vec<(f64, u8, SectionKey)> = self
            .nodes
            .keys()
            .map(|key| {
                let pos = key.pos();
                (horizontal_distance_squared(camera, pos), pos.level(), *key)
            })
            .collect();
        let farther = |a: &(f64, u8, SectionKey), b: &(f64, u8, SectionKey)| {
            a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2))
        };
        order.select_nth_unstable_by(keep, farther);
        for (_, _, key) in &order[keep..] {
            self.nodes.remove(key);
        }
    }

    /// Positions known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::{Octant, SectionPos},
        section::ChildMask,
    };

    fn pos(level: u8, x: i32, z: i32) -> SectionPos {
        SectionPos::new(level, x, 0, z).expect("position")
    }

    #[test]
    fn a_limited_tree_refuses_reads_past_its_limit() {
        let mut tree = ExploredTree::with_limit(2);
        let node = Node::Present(ChildMask::UNKNOWN);
        assert!(tree.insert(pos(0, 0, 0).key(), node));
        assert!(tree.insert(pos(0, 1, 0).key(), node));
        assert!(!tree.insert(pos(0, 2, 0).key(), node), "full");
        assert_eq!(tree.get(pos(0, 2, 0)), None, "a refused read stays unknown");
        assert!(
            tree.insert(pos(0, 1, 0).key(), Node::Missing),
            "known ones update"
        );
        assert_eq!(tree.len(), tree.limit());
    }

    #[test]
    fn trims_forget_the_farthest_and_keep_ancestors_of_what_stays() {
        let mut tree = ExploredTree::default();
        let node = Node::Present(ChildMask::UNKNOWN);
        let parent = pos(1, 0, 0);
        tree.insert(parent.key(), node);
        for octant in Octant::ALL {
            tree.insert(parent.child(octant).expect("child").key(), node);
        }
        for x in 1..20 {
            tree.insert(pos(1, x, 0).key(), node);
        }
        tree.insert(pos(1, 1_000, 0).key(), node);
        tree.trim([1.0, 0.0, 1.0], 20.0 * 64.0, 5);
        assert_eq!(tree.len(), 5);
        assert!(
            tree.get(parent).is_some(),
            "the nearest children's parent stays"
        );
        assert!(tree.get(pos(1, 1_000, 0)).is_none());
        assert!(tree.get(pos(1, 19, 0)).is_none());
    }
}
