//! Measurements shared by the size and placement passes of one layout, or kept
//! across one tree's layouts by a [`MeasureCache`].

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use super::{Axis, LayoutEnv, Rect, ResolvedControl, axis_index, grid, size, stack};
use crate::expr::{Length, Unit};

type Key = (usize, Option<u64>, Option<u64>);

/// Every child's `[w, h]`, shared so memo hits do not copy.
pub(super) type Sizes = std::sync::Arc<[[f64; 2]]>;

/// Memo maps keyed by control addresses and small integers, hashed cheaply.
pub(super) type Memo<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<AddressHasher>>;

/// A multiply-rotate hasher for address keys; SipHash dominated layout time.
#[derive(Default)]
pub(super) struct AddressHasher(u64);

impl std::hash::Hasher for AddressHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Children {
    /// Summed visible children per axis (`%c`).
    pub(super) content: [f64; 2],
    /// Largest visible child per axis (`%cm`).
    pub(super) maximum: [f64; 2],
}

type PlaceMemo = Memo<(usize, u64, u64), Placements>;

/// Stable target addresses and visibility masks, shared with the emit pass.
pub(super) type Targets = std::sync::Arc<[(usize, u8)]>;
type TargetMemo = Memo<(usize, bool), Targets>;

/// Contiguous grid rows in placement order, including earlier rows' overhang.
struct Row {
    top: f64,
    bottom: f64,
    start: usize,
}

struct Placements {
    children: Vec<(usize, Rect)>,
    rows: Vec<Row>,
}

impl Placements {
    /// Index grids whose rows advance monotonically; arbitrary placement stays unindexed.
    fn new(children: Vec<(usize, Rect)>, grid: bool) -> Self {
        let mut rows: Vec<Row> = Vec::new();
        if grid {
            let mut bottom = f64::NEG_INFINITY;
            for (index, (_, rect)) in children.iter().enumerate() {
                if !rect.y.is_finite()
                    || !rect.h.is_finite()
                    || rows.last().is_some_and(|row| rect.y < row.top)
                {
                    rows.clear();
                    break;
                }
                bottom = bottom.max(rect.y + rect.h);
                if let Some(row) = rows.last_mut().filter(|row| row.top == rect.y) {
                    row.bottom = bottom;
                } else {
                    rows.push(Row {
                        top: rect.y,
                        bottom,
                        start: index,
                    });
                }
            }
        }
        Self { children, rows }
    }

    /// Select possibly intersecting rows while preserving child order and indexes.
    fn visible(&self, clip: Option<Rect>, origin: Rect) -> &[(usize, Rect)] {
        let Some(clip) = clip.filter(|_| !self.rows.is_empty()) else {
            return &self.children;
        };
        let first = self
            .rows
            .partition_point(|row| row.bottom <= clip.y - origin.y);
        let end = self
            .rows
            .partition_point(|row| row.top < clip.y + clip.h - origin.y);
        let start = self
            .rows
            .get(first)
            .map_or(self.children.len(), |row| row.start);
        let end = self
            .rows
            .get(end)
            .map_or(self.children.len(), |row| row.start);
        &self.children[start.min(end)..end]
    }
}

thread_local! {
    static CHILDREN: RefCell<Memo<Key, Children>> = RefCell::new(Memo::default());
    static NATURAL: RefCell<Memo<Key, Option<[f64; 2]>>> = RefCell::new(Memo::default());
    /// [`sizes`] by parent address and known extent. Children sizes are pure in
    /// the subtree, `env` and that extent, but content extents re-derive them, so
    /// without this the cost is exponential in tree depth.
    static SIZES: RefCell<Memo<Key, Sizes>> = RefCell::new(Memo::default());
    /// Parsed `size`/`min_size`/`max_size` lengths by control address and slot.
    pub(super) static LENGTHS: RefCell<Memo<(usize, u8), Option<Length>>> =
        RefCell::new(Memo::default());
    /// Per-control rule flags derived from the lengths.
    pub(super) static FLAGS: RefCell<Memo<usize, size::Flags>> = RefCell::new(Memo::default());
    /// [`placed_children`]: child indices and rects relative to the parent's
    /// origin, by parent address and size.
    static PLACED: RefCell<PlaceMemo> = RefCell::new(PlaceMemo::default());
    /// Bound placement properties by control address.
    static STYLES: RefCell<Memo<usize, super::style::Style>> = RefCell::new(Memo::default());
    /// Widget state masks by control address and ancestor lock state.
    static TARGETS: RefCell<TargetMemo> = RefCell::new(TargetMemo::default());
    /// Scroll bar panels hidden while their content fits, by address.
    static SUPPRESSED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Discard measurements before borrowing a new tree or measurement environment.
pub(super) fn reset() {
    CHILDREN.with(|memo| memo.borrow_mut().clear());
    NATURAL.with(|memo| memo.borrow_mut().clear());
    SIZES.with(|memo| memo.borrow_mut().clear());
    LENGTHS.with(|memo| memo.borrow_mut().clear());
    FLAGS.with(|memo| memo.borrow_mut().clear());
    PLACED.with(|memo| memo.borrow_mut().clear());
    TARGETS.with(|memo| memo.borrow_mut().clear());
    STYLES.with(|memo| memo.borrow_mut().clear());
    SUPPRESSED.with(|set| set.borrow_mut().clear());
    super::scroll::reset();
}

/// Whether a scroll view hides `control`, its bar panel.
pub(super) fn suppressed(control: &ResolvedControl) -> bool {
    SUPPRESSED.with(|set| {
        let set = set.borrow();
        !set.is_empty() && set.contains(&std::ptr::from_ref(control).addr())
    })
}

/// Hide or show a scroll view's bar panel; when that changes, the measurements
/// that saw the old visibility are dropped. Returns whether it changed.
pub(super) fn suppress(panel: usize, hidden: bool) -> bool {
    let changed = SUPPRESSED.with(|set| {
        let mut set = set.borrow_mut();
        if hidden {
            set.insert(panel)
        } else {
            set.remove(&panel)
        }
    });
    if changed {
        CHILDREN.with(|memo| memo.borrow_mut().clear());
        SIZES.with(|memo| memo.borrow_mut().clear());
        PLACED.with(|memo| memo.borrow_mut().clear());
    }
    changed
}

/// Measurements of one bound tree, reused by its later layouts. Start a new one
/// whenever the tree, the root size or the measurement environment changes.
#[derive(Default)]
pub struct MeasureCache {
    children: Memo<Key, Children>,
    natural: Memo<Key, Option<[f64; 2]>>,
    sizes: Memo<Key, Sizes>,
    lengths: Memo<(usize, u8), Option<Length>>,
    flags: Memo<usize, size::Flags>,
    placed: PlaceMemo,
    targets: TargetMemo,
    styles: Memo<usize, super::style::Style>,
    suppressed: HashSet<usize>,
    roles: super::scroll::RoleMemo,
    /// The root's address last layout; a moved root's entries go stale.
    root: usize,
}

impl MeasureCache {
    /// Replace changed controls in place and retain measurements of untouched subtrees.
    pub fn update_tree(&mut self, tree: &mut ResolvedControl, next: ResolvedControl) {
        let mut dirty = HashSet::new();
        if !super::refresh::update(tree, next, &mut dirty) {
            return;
        }
        // The root moves into the render call; its cached address differs from `tree`.
        dirty.insert(self.root);
        if !self.suppressed.is_empty() {
            *self = Self::default();
            return;
        }
        self.children.retain(|key, _| !dirty.contains(&key.0));
        self.natural.retain(|key, _| !dirty.contains(&key.0));
        self.sizes.retain(|key, _| !dirty.contains(&key.0));
        self.lengths.retain(|key, _| !dirty.contains(&key.0));
        self.flags.retain(|key, _| !dirty.contains(key));
        self.placed.retain(|key, _| !dirty.contains(&key.0));
        self.targets.retain(|key, _| !dirty.contains(&key.0));
        self.styles.retain(|key, _| !dirty.contains(key));
        self.roles.clear();
    }

    fn swap(&mut self) {
        CHILDREN.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.children));
        NATURAL.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.natural));
        SIZES.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.sizes));
        LENGTHS.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.lengths));
        FLAGS.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.flags));
        PLACED.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.placed));
        TARGETS.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.targets));
        STYLES.with(|memo| std::mem::swap(&mut *memo.borrow_mut(), &mut self.styles));
        SUPPRESSED.with(|set| std::mem::swap(&mut *set.borrow_mut(), &mut self.suppressed));
        super::scroll::swap(&mut self.roles);
    }

    /// Make these the live memos for a layout of `root`.
    pub(super) fn enter(&mut self, root: &ResolvedControl) {
        self.swap();
        let address = std::ptr::from_ref(root).addr();
        if self.root != address {
            let stale = self.root;
            CHILDREN.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            NATURAL.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            SIZES.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            LENGTHS.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            FLAGS.with(|memo| memo.borrow_mut().retain(|key, _| *key != stale));
            PLACED.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            TARGETS.with(|memo| memo.borrow_mut().retain(|key, _| key.0 != stale));
            STYLES.with(|memo| {
                memo.borrow_mut().remove(&stale);
            });
            SUPPRESSED.with(|set| {
                set.borrow_mut().remove(&stale);
            });
            super::scroll::forget(stale);
            self.root = address;
        }
    }

    /// Park the live memos again after the layout.
    pub(super) fn leave(&mut self) {
        self.swap();
    }
}

/// Reuse typed bound properties until an update changes this control or its allocation.
pub(super) fn style(control: &ResolvedControl) -> super::style::Style {
    let address = std::ptr::from_ref(control).addr();
    STYLES.with(|memo| {
        *memo
            .borrow_mut()
            .entry(address)
            .or_insert_with(|| super::style::Style::read(control))
    })
}

/// Resolve a widget's masks once per bound tree and ancestor lock state.
pub(super) fn state_targets(control: &ResolvedControl, locked: bool) -> Option<Targets> {
    if !crate::widgets::has_state_targets(control) {
        return None;
    }
    let key = (std::ptr::from_ref(control).addr(), locked);
    if let Some(targets) = TARGETS.with(|memo| memo.borrow().get(&key).cloned()) {
        return Some(targets);
    }
    let targets: Targets = crate::widgets::state_targets(control, locked)
        .into_iter()
        .map(|target| (std::ptr::from_ref(target.control).addr(), target.mask))
        .collect();
    TARGETS.with(|memo| memo.borrow_mut().insert(key, targets.clone()));
    Some(targets)
}

/// Identify a control and its exact known `[width, height]` for the lifetime of this layout.
fn key(control: &ResolvedControl, own: [Option<f64>; 2]) -> Key {
    (
        std::ptr::from_ref(control).addr(),
        own[0].map(f64::to_bits),
        own[1].map(f64::to_bits),
    )
}

/// Measure visible children once at `own`, the control's known extent. A
/// templated grid's `%c` builds no terms, so it sums to zero.
pub(super) fn children(
    control: &ResolvedControl,
    env: &LayoutEnv,
    own: [Option<f64>; 2],
) -> Children {
    if control.children.is_empty() {
        return Children::default();
    }
    let key = key(control, own);
    if let Some(cached) = CHILDREN.with(|memo| memo.borrow().get(&key).copied()) {
        return cached;
    }
    let sizes = sizes(control, own, env);
    let mut sum = [0.0; 2];
    let mut maximum = [0.0_f64; 2];
    let resting = crate::widgets::rest_hidden_children(control);
    let templated = grid::is_grid(control);
    for (child, size) in control.children.iter().zip(sizes.iter()) {
        if !super::visible(child)
            || (templated && grid::is_template_node(child))
            || resting.contains(&child.name.as_str())
        {
            continue;
        }
        for axis in 0..2 {
            sum[axis] += size[axis];
            maximum[axis] = maximum[axis].max(size[axis]);
        }
    }
    if grid::has_template(control) {
        sum = [0.0; 2];
    }
    let measured = Children {
        content: sum,
        maximum,
    };
    CHILDREN.with(|memo| memo.borrow_mut().insert(key, measured));
    measured
}

/// Every child's `[w, h]` under `parent` of known `extent`, by the parent's
/// kind: stack items, grid cells, or ordinary relative rules.
pub(super) fn sizes(parent: &ResolvedControl, extent: [Option<f64>; 2], env: &LayoutEnv) -> Sizes {
    let key = key(parent, extent);
    if let Some(cached) = SIZES.with(|memo| memo.borrow().get(&key).cloned()) {
        return cached;
    }
    let measured: Sizes = if stack::orientation(parent).is_some() {
        stack::child_sizes(parent, extent, env).into()
    } else if grid::is_grid(parent) {
        grid::child_sizes(parent, extent, env).into()
    } else {
        relative_sizes(parent, extent, env).into()
    };
    SIZES.with(|memo| memo.borrow_mut().insert(key, measured.clone()));
    measured
}

/// Children sized by their own rules against `extent`.
pub(super) fn relative_sizes(
    parent: &ResolvedControl,
    extent: [Option<f64>; 2],
    env: &LayoutEnv,
) -> Vec<[f64; 2]> {
    let mut sizes = resolve_children(parent, extent, env, |_| true);
    apply_inherit(parent, &mut sizes, |_| true);
    sizes
}

/// Resolve the children `include` selects (others stay zero): those reading
/// `%sm` after the rest, against the largest resolved sibling.
pub(super) fn resolve_children(
    parent: &ResolvedControl,
    extent: [Option<f64>; 2],
    env: &LayoutEnv,
    include: impl Fn(&ResolvedControl) -> bool,
) -> Vec<[f64; 2]> {
    let mut sizes = vec![[0.0; 2]; parent.children.len()];
    let mut deferred = false;
    for (child, slot) in parent.children.iter().zip(sizes.iter_mut()) {
        if !include(child) {
            continue;
        }
        if reads_sibling_max(child) {
            deferred = true;
            continue;
        }
        *slot = size::resolve_size(child, extent, [0.0; 2], env);
    }
    if deferred {
        let siblings = sibling_maxima(parent, &sizes);
        for (child, slot) in parent.children.iter().zip(sizes.iter_mut()) {
            if include(child) && reads_sibling_max(child) {
                *slot = size::resolve_size(child, extent, siblings, env);
            }
        }
    }
    sizes
}

fn reads_sibling_max(child: &ResolvedControl) -> bool {
    size::flags(child).reads_sibling_max
}

/// The largest visible child per axis among those whose size does not itself
/// read `%sm` (the client leaves those out to avoid a cycle).
pub(super) fn sibling_maxima(parent: &ResolvedControl, sizes: &[[f64; 2]]) -> [f64; 2] {
    let mut maxima = [0.0_f64; 2];
    let templated = grid::is_grid(parent);
    for (child, size) in parent.children.iter().zip(sizes) {
        if !super::visible(child) || (templated && grid::is_template_node(child)) {
            continue;
        }
        for axis in [Axis::X, Axis::Y] {
            let index = axis_index(axis);
            if !size::with_size(child, axis, |length| {
                length.is_some_and(|length| length.uses(Unit::PercentSiblingMax))
            }) {
                maxima[index] = maxima[index].max(size[index]);
            }
        }
    }
    maxima
}

/// `inherit_max_sibling_width`/`height`: after solving, each inheriting child
/// (of those `include` selects) takes the largest same-axis value among itself
/// and its visible siblings, all read before any is replaced.
pub(super) fn apply_inherit(
    parent: &ResolvedControl,
    sizes: &mut [[f64; 2]],
    include: impl Fn(&ResolvedControl) -> bool,
) {
    let inherits = |child: &ResolvedControl, index: usize| size::flags(child).inherits[index];
    if !parent
        .children
        .iter()
        .any(|child| inherits(child, 0) || inherits(child, 1))
    {
        return;
    }
    let templated = grid::is_grid(parent);
    let mut maxima = [0.0_f64; 2];
    for (child, size) in parent.children.iter().zip(sizes.iter()) {
        if super::visible(child) && !(templated && grid::is_template_node(child)) {
            maxima = [maxima[0].max(size[0]), maxima[1].max(size[1])];
        }
    }
    for (child, size) in parent.children.iter().zip(sizes.iter_mut()) {
        for index in 0..2 {
            if include(child) && inherits(child, index) {
                size[index] = size[index].max(maxima[index]);
            }
        }
    }
}

/// Measure a label or texture once for each known width, including absent sizes.
pub(super) fn natural(
    control: &ResolvedControl,
    width: Option<f64>,
    read: impl FnOnce() -> Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    let key = key(control, [width, None]);
    if let Some(cached) = NATURAL.with(|memo| memo.borrow().get(&key).copied()) {
        return cached;
    }
    let measured = read();
    NATURAL.with(|memo| memo.borrow_mut().insert(key, measured));
    measured
}

/// [`super::layout_children`], memoized by the parent and its size: children sit at the
/// same offsets from their parent wherever it is placed.
pub(super) fn placed_children<'a>(
    parent: &'a ResolvedControl,
    rect: Rect,
    env: &LayoutEnv,
    clip: Option<Rect>,
) -> Vec<(&'a ResolvedControl, Rect)> {
    let key = (
        std::ptr::from_ref(parent).addr(),
        rect.w.to_bits(),
        rect.h.to_bits(),
    );
    let shift = |(index, at): &(usize, Rect)| {
        let moved = Rect::new(at.x + rect.x, at.y + rect.y, at.w, at.h);
        (&parent.children[*index], moved)
    };
    let memoized = PLACED.with(|memo| {
        let memo = memo.borrow();
        memo.get(&key)
            .map(|placed| placed.visible(clip, rect).iter().map(shift).collect())
    });
    if let Some(placed) = memoized {
        return placed;
    }
    let relative: Vec<(usize, Rect)> =
        super::layout_children(parent, Rect::new(0.0, 0.0, rect.w, rect.h), env)
            .into_iter()
            .map(|(child, at)| (child_index(parent, child), at))
            .collect();
    let relative = Placements::new(relative, grid::is_grid(parent));
    let placed = relative.visible(clip, rect).iter().map(shift).collect();
    PLACED.with(|memo| memo.borrow_mut().insert(key, relative));
    placed
}

/// A child's position among its parent's children.
pub(super) fn child_index(parent: &ResolvedControl, child: &ResolvedControl) -> usize {
    let base = parent.children.as_ptr().addr();
    let size = std::mem::size_of::<ResolvedControl>().max(1);
    (std::ptr::from_ref(child).addr() - base) / size
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    /// Indexed clipping must retain tall earlier cells and original collection order.
    #[test]
    fn visible_rows_match_full_placement_culling() {
        let children: Vec<_> = (0..60)
            .map(|index| {
                let height = if index == 2 { 80.0 } else { 10.0 };
                (
                    index,
                    Rect::new(
                        (index % 3) as f64 * 10.0,
                        (index / 3) as f64 * 10.0,
                        10.0,
                        height,
                    ),
                )
            })
            .collect();
        let reverse = Placements::new(children.iter().copied().rev().collect(), true);
        assert!(reverse.rows.is_empty());
        let placements = Placements::new(children, true);
        let origin = Rect::new(2.0, -37.0, 30.0, 200.0);
        for y in [0.0, 10.0, 32.0, 100.0, 250.0] {
            let clip = Rect::new(0.0, y, 40.0, 24.0);
            let visible = |items: &[(usize, Rect)]| {
                items
                    .iter()
                    .filter_map(|(index, rect)| {
                        let rect = Rect::new(rect.x + origin.x, rect.y + origin.y, rect.w, rect.h);
                        (!super::super::disjoint(rect, clip)).then_some(*index)
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                visible(placements.visible(Some(clip), origin)),
                visible(&placements.children)
            );
        }
        assert!(
            placements
                .visible(Some(Rect::new(0.0, 100.0, 40.0, 24.0)), origin)
                .len()
                < 15
        );
    }
}
