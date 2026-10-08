//! What the client's controls keep between binds: each property bag, the
//! component state bindings last set, and every binding's schedule memory.
//! Controls are addressed by layout key.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::predicate::Scalar;
use crate::state::LayoutReport;
use crate::tree::ResolvedControl;

/// FNV-1a over a layout key, streamed so a child extends its parent's hash.
pub(super) fn key_hash(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The hash of the empty key, where every layout key starts.
pub(super) const KEY_ROOT: u64 = 0xcbf2_9ce4_8422_2325;

/// Properties that scroll layout publishes back into binding bags.
pub(super) const SCROLL_PROPERTIES: [&str; 3] = [
    "#scrolled_to_end",
    "#scrollbar_hit_bottom",
    "#scroll_bar_visible",
];

/// Hashes a map key that already is a key hash.
#[derive(Default)]
pub(super) struct Prehashed(u64);

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

pub(super) type KeyMap<V> = HashMap<u64, V, BuildHasherDefault<Prehashed>>;

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
    pub(super) incarnation: Option<u64>,
    pub(super) parent_incarnation: Option<u64>,
    pub(super) bag: BTreeMap<String, Scalar>,
    /// Literal properties bindings set on components.
    pub(super) native: BTreeMap<String, Value>,
    /// `once` bindings already applied, by index.
    pub(super) once: BTreeSet<usize>,
    /// Visibility each `visibility_changed` binding last applied at.
    pub(super) seen: BTreeMap<usize, bool>,
    /// Last source value each registered view observed.
    pub(super) views: BTreeMap<usize, Option<Scalar>>,
    /// The parent's key hash.
    pub(super) parent: u64,
    /// The refresh that last built this control.
    pub(super) generation: u64,
    /// Whether its subtree waited hidden then.
    pub(super) deferred: bool,
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
