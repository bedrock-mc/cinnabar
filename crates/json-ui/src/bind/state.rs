//! What the client's controls keep between binds: each property bag, the
//! component state bindings last set, and every binding's schedule memory.
//! Controls are addressed by layout key.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::predicate::Scalar;
use crate::state::LayoutReport;
use crate::tree::ResolvedControl;

/// FNV-1a over a layout key, streamed so a child extends its parent's hash.
pub(crate) fn key_hash(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The hash of the empty key, where every layout key starts.
pub(crate) const KEY_ROOT: u64 = 0xcbf2_9ce4_8422_2325;
/// Ordinary virtualized grids start with this many cells before their first layout.
const INITIAL_GRID_WINDOW: usize = 64;

/// Properties that scroll layout publishes back into binding bags.
pub(super) const SCROLL_PROPERTIES: [&str; 3] = [
    "#scrolled_to_end",
    "#scrollbar_hit_bottom",
    "#scroll_bar_visible",
];

/// Hashes a map key that already is a key hash.
#[derive(Default)]
pub(crate) struct Prehashed(u64);

impl Hasher for Prehashed {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = key_hash(self.0 ^ KEY_ROOT, bytes);
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

pub(crate) type KeyMap<V> = HashMap<u64, V, BuildHasherDefault<Prehashed>>;

/// A screen's live binding state, kept by the caller across binds of the same
/// screen and dropped when the screen changes.
#[derive(Clone, Debug, Default)]
pub struct BindState {
    /// By layout-key hash.
    pub(super) controls: KeyMap<Retained>,
    /// Bag writes from widgets and components since the last bind.
    pub(super) published: KeyMap<BTreeMap<String, Scalar>>,
    /// The refresh count, which marks the controls each refresh built.
    pub(super) generation: u64,
    /// Whether any built control reads the layout’s scroll feedback.
    pub(super) scroll_observed: bool,
    /// Whether binding omitted descendants after exhausting the shared node budget.
    pub(crate) node_budget_exceeded: bool,
    /// The last bind's controls, which hold the memory of every live one; `controls`
    /// keeps only those dropped under still-hidden ancestors.
    pub(super) tree: Option<Box<super::Node>>,
    /// The data the last incremental bind read.
    pub(super) data: Option<std::sync::Arc<super::DataSource>>,
    /// Keys of the controls the last bind built rather than reused.
    pub(super) built: Vec<u64>,
    /// Whether the last bind's views reached a fixed point, so a view whose control and
    /// source both stand unchanged would observe what it last did.
    pub(super) views_settled: bool,
    /// View notifications the last bind ran.
    pub(super) views_run: usize,
    /// Collections whose large grids build only the cells around those layout shows.
    pub(super) virtual_grids: BTreeMap<String, GridPolicy>,
    /// The cells each virtualized grid builds, by its key; the rest bind as placeholders.
    pub(super) grid_windows: KeyMap<Range<usize>>,
    /// Grids whose built cells moved since the last bind, which it rebuilds.
    pub(super) moved_windows: Vec<u64>,
    /// Each grid's cell template with its placeholder, so standing placeholders are reused.
    pub(super) placeholders: Vec<(u64, Arc<ResolvedControl>, Arc<ResolvedControl>)>,
}

/// An opted grid's first window and the host's guarantee about cell paint bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GridPolicy {
    initial: usize,
    clipped: bool,
}

/// Allocate an opaque lifetime identity only when a custom control is created.
pub(super) fn new_custom_instance() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// One control's memory.
#[derive(Clone, Debug, Default)]
pub(super) struct Retained {
    /// Survives hidden subtrees and refreshes, but never a destroyed control.
    pub(super) custom_instance: Option<u64>,
    /// Binding, component, or visibility state owns the bag instead of fresh literals.
    pub(super) keep_bag: bool,
    pub(super) incarnation: Option<u64>,
    pub(super) parent_incarnation: Option<u64>,
    pub(super) bag: BTreeMap<String, Scalar>,
    /// Inherited creation inputs before view bindings transformed the bag.
    pub(super) creation_inputs: Arc<super::bag::Bag>,
    /// Changed creation inputs that view observers must consume before normal settling.
    pub(super) creation_writes: BTreeSet<String>,
    /// Each view writer invalidated by a changed creation target.
    pub(super) creation_views: BTreeSet<usize>,
    /// Literal properties bindings set on components.
    pub(super) native: BTreeMap<String, Value>,
    /// `once` bindings already applied, by index.
    pub(super) once: BTreeSet<usize>,
    /// Visibility each `visibility_changed` binding last applied at.
    pub(super) seen: BTreeMap<usize, bool>,
    /// Last source value each registered view observed.
    pub(super) views: BTreeMap<usize, Option<Scalar>>,
    /// The source control each view last ran against, by key; `None` when it did not resolve.
    pub(super) view_sources: BTreeMap<usize, Option<u64>>,
    /// The parent's key hash.
    pub(super) parent: u64,
    /// The refresh that last built this control.
    pub(super) generation: u64,
    /// Whether its subtree waited hidden then.
    pub(super) deferred: bool,
}

impl Retained {
    /// Refreshes changed inputs while preserving standing view outputs. Changed
    /// targets notify their readers before ordinary view settling can overwrite them.
    pub(super) fn refresh_creation_values(
        &mut self,
        own: &mut super::bag::Bag,
        inputs: &super::bag::Bag,
        bindings: &[super::spec::Binding],
    ) {
        for (name, value) in inputs {
            let changed = self.creation_inputs.get(name) != Some(value);
            let mut written = false;
            for (index, binding) in bindings.iter().enumerate() {
                if matches!(&binding.kind, super::spec::Kind::View { target, .. } if target == name)
                    && self.views.get(&index).is_some_and(Option::is_some)
                {
                    written = true;
                    if changed {
                        self.creation_views.insert(index);
                    }
                }
            }
            if written && changed {
                self.creation_writes.insert(name.clone());
            }
            if !written || changed {
                own.insert(name.clone(), value.clone());
            }
        }
    }
}

impl BindState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write `name` into the bag of the control at layout `key`, as a widget or
    /// component publishes state; views reading it update on the next bind.
    pub fn publish(&mut self, key: &str, name: &str, value: Scalar) {
        self.published
            .entry(key_hash(KEY_ROOT, key.as_bytes()))
            .or_default()
            .insert(name.to_owned(), value);
    }

    /// The bag value `name` of the control at layout `key` after the last bind.
    pub fn value(&self, key: &str, name: &str) -> Option<&Scalar> {
        let key = key_hash(KEY_ROOT, key.as_bytes());
        let live = || {
            let node = super::reuse::find(self.tree.as_deref()?, key)?;
            node.retained.then_some(&node.own)
        };
        self.published
            .get(&key)
            .and_then(|values| values.get(name))
            .or_else(|| live()?.get(name))
            .or_else(|| self.controls.get(&key)?.bag.get(name))
    }

    /// Publish each scroll view's end and scrollbar state the layout computed;
    /// `true` when a published value changed.
    pub fn publish_scrolls(&mut self, report: &LayoutReport) -> bool {
        let mut changed = false;
        for (key, metrics) in &report.scrolls {
            let published = [
                Some(metrics.scrolled_to_end),
                Some(metrics.hit_bottom),
                metrics.bar_visible,
            ];
            for (name, value) in SCROLL_PROPERTIES.into_iter().zip(published) {
                let Some(value) = value else { continue };
                if self.value(key, name) != Some(&Scalar::Bool(value)) {
                    self.publish(key, name, Scalar::Bool(value));
                    changed = true;
                }
            }
        }
        changed
    }

    /// Lets a large grid listing `collection` build only the cells around those in view, binding
    /// the rest as empty placeholders of its cell template's size. After each layout, pass its
    /// report to [`Self::show_grid_cells`] and bind and lay out again while that returns `true`.
    ///
    /// Only for a grid whose cells have a literal pixel size, draw less than a view's extent past
    /// their rect, and read through `once` bindings only what stays fixed per index: a cell built
    /// late must match one built with the grid.
    pub fn virtualize_grid(&mut self, collection: &str) {
        self.virtualize_grid_with_window(collection, INITIAL_GRID_WINDOW);
    }

    /// Selects a smaller first window for fixed native viewports with large tile templates.
    pub fn virtualize_grid_with_window(&mut self, collection: &str, initial: usize) {
        self.set_grid_policy(collection, initial, false);
    }

    /// Opts in host-admitted tiles whose descendants cannot paint outside their cell rectangle.
    pub fn virtualize_clipped_grid(&mut self, collection: &str, initial: usize) {
        self.set_grid_policy(collection, initial, true);
    }

    /// Invalidates placeholder geometry when the owning host changes its grid guarantee.
    fn set_grid_policy(&mut self, collection: &str, initial: usize, clipped: bool) {
        let policy = GridPolicy {
            initial: initial.clamp(1, INITIAL_GRID_WINDOW),
            clipped,
        };
        if self.virtual_grids.get(collection) == Some(&policy) {
            return;
        }
        self.virtual_grids.insert(collection.to_owned(), policy);
        self.grid_windows.clear();
        self.placeholders.clear();
    }

    /// Whether the host guarantees this collection's cells cannot overflow their rectangles.
    pub(super) fn clips_grid_cells(&self, collection: Option<&str>) -> bool {
        collection
            .and_then(|name| self.virtual_grids.get(name))
            .is_some_and(|policy| policy.clipped)
    }

    /// Whether grids listing `collection` build only the cells around those in view.
    pub(super) fn virtualizes(&self, collection: Option<&str>) -> bool {
        collection.is_some_and(|name| self.virtual_grids.contains_key(name))
    }

    /// The first unmeasured window for the opted collection, retaining the ordinary default.
    pub(super) fn initial_grid_window(&self, collection: Option<&str>) -> usize {
        collection
            .and_then(|name| self.virtual_grids.get(name))
            .map(|policy| policy.initial)
            .unwrap_or(INITIAL_GRID_WINDOW)
    }

    /// Moves each virtualized grid's built cells over the cells `report` laid out in view, with
    /// a quarter view of margin on either side; `true` when a grid must bind and lay out again.
    pub fn show_grid_cells(&mut self, report: &LayoutReport) -> bool {
        let mut moved = false;
        for (grid, view) in &report.grid_cells {
            if view.is_empty() {
                continue;
            }
            // A range left far wider than the view, as after a longer tab, shrinks back.
            if self.grid_windows.get(grid).is_some_and(|built| {
                built.start <= view.start && view.end <= built.end && built.len() <= 4 * view.len()
            }) {
                continue;
            }
            let margin = Self::grid_window_margin(view.len());
            self.grid_windows
                .insert(*grid, view.start.saturating_sub(margin)..view.end + margin);
            self.moved_windows.push(*grid);
            moved = true;
        }
        moved
    }

    /// Shares the retained-cell margin with hosts that admit a fixed viewport's node budget.
    pub fn grid_window_margin(visible: usize) -> usize {
        visible / 4 + 1
    }

    /// Whether scroll feedback can affect a bound property or view.
    pub fn observes_scroll(&self) -> bool {
        self.scroll_observed
    }

    /// Whether view notifications left a visibility-scheduled binding for the next refresh.
    pub fn has_pending_visibility(&self) -> bool {
        self.tree
            .as_ref()
            .is_some_and(|node| node.track.visibility_pending)
    }

    /// Whether a bind has run over this state yet.
    pub fn is_empty(&self) -> bool {
        fn retains(node: &super::Node) -> bool {
            node.retained || node.children.iter().any(retains)
        }
        self.tree.as_deref().is_none_or(|tree| !retains(tree)) && self.controls.is_empty()
    }

    /// Controls the last bind built rather than carried over unchanged.
    pub fn rebuilt(&self) -> usize {
        self.built.len()
    }

    /// View bindings the last bind ran rather than left standing.
    pub fn views_run(&self) -> usize {
        self.views_run
    }

    /// The `/`-joined name paths of the controls the last bind built.
    pub fn rebuilt_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(tree) = &self.tree {
            super::reuse::rebuilt_paths(tree, &self.built, "", &mut paths);
        }
        paths
    }

    /// The whole tree the last bind produced.
    pub(super) fn bake(&mut self, components: &crate::component::Components) -> ResolvedControl {
        let tree = self.tree.as_deref_mut().expect("a bind keeps its root");
        super::reuse::bake_full(tree, components)
    }
}
