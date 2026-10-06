//! Incremental refreshes: a control whose source, scope and data reads match
//! the last bind, and whose last bind left its whole subtree at a fixed point,
//! is moved over as it stood instead of rebuilt. Bindings re-run only where an
//! input changed, so the result equals rebuilding every control.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use super::bag::Bag;
use super::data::DataSource;
use super::declarations::Declaration;
use super::native::Native;
use super::source::Src;
use super::spec::{Kind, ViewScope};
use super::state::{KEY_ROOT, KeyMap, Retained, key_hash};
use super::{Binder, Node, Scope, output};
use crate::component::Components;
use crate::tree::{Properties, ResolvedControl};

/// A bloom set of data keys a control or subtree reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Reads([u64; 8]);

/// The keys a refresh changed, as hashes.
#[derive(Clone, Debug, Default)]
pub(super) struct Changes(Vec<u64>);

/// The data a key names.
#[derive(Clone, Copy)]
pub(super) enum Data {
    Global = 1,
    Item,
    Length,
    Roles,
    Defaults,
    Control,
    Feed,
}

fn data_hash(kind: Data, name: &str, index: Option<usize>) -> u64 {
    let hash = key_hash(KEY_ROOT ^ kind as u64, name.as_bytes());
    match index {
        Some(index) => key_hash(hash, &index.to_le_bytes()),
        None => hash,
    }
}

/// The two filter bits a key hash sets.
fn bits(hash: u64) -> [(usize, u64); 2] {
    let bit = |value: u64| ((value as usize / 64) % 8, 1 << (value % 64));
    [bit(hash % 512), bit((hash >> 32) % 512)]
}

impl Reads {
    pub(super) fn add(&mut self, kind: Data, name: &str, index: Option<usize>) {
        for (word, bit) in bits(data_hash(kind, name, index)) {
            self.0[word] |= bit;
        }
    }

    fn union(&mut self, other: &Reads) {
        for (word, other) in self.0.iter_mut().zip(other.0) {
            *word |= other;
        }
    }

    /// Whether any changed key may be among these reads.
    fn meets(&self, changes: &Changes) -> bool {
        changes.0.iter().any(|hash| {
            bits(*hash)
                .iter()
                .all(|(word, bit)| self.0[*word] & bit != 0)
        })
    }
}

impl Changes {
    fn add(&mut self, kind: Data, name: &str, index: Option<usize>) {
        self.0.push(data_hash(kind, name, index));
    }
}

/// Per-control reuse bookkeeping, carried with the retained tree.
#[derive(Clone, Default)]
pub(super) struct Track {
    /// What the control's own creation reads from the data source.
    reads: Reads,
    /// [`Self::reads`] across the subtree.
    below: Reads,
    /// Controls in the subtree, counted against the creation budget.
    count: usize,
    observes_scroll: bool,
    /// Whether any control in the subtree reads scroll feedback.
    scroll: bool,
    /// The last bind left this control's state as it found it.
    settled: bool,
    /// Every control in the subtree is settled.
    stable: bool,
    /// Built this bind.
    pub(super) fresh: bool,
    /// Changed after building this bind (a view write or a deferred expansion).
    pub(super) touched: bool,
    /// Something in the subtree was built or changed this bind.
    pub(super) dirty: bool,
    /// The subtree holds a control whose children wait to be built.
    pub(super) deferred_below: bool,
    /// The subtree holds a view binding.
    pub(super) views_below: bool,
    /// Properties of the last bake.
    baked: Option<Properties>,
    /// The source before a bound grid patched it, which a reuse compares.
    origin: Option<Src>,
    /// The replaced control's state, compared once the bind settles.
    prior: Option<Box<Prior>>,
    /// Child keys before a touch expanded a reused control.
    pub(super) touched_children: Option<Vec<u64>>,
    /// A `factory` control whose creations its parent holds.
    pub(super) hoisted: bool,
}

/// What a rebuilt control was before the bind.
#[derive(Clone)]
pub(super) struct Prior {
    src: Src,
    retained: bool,
    own: Bag,
    native: Native,
    memory: Retained,
    deferred: bool,
    children: Vec<u64>,
    baked: Option<Properties>,
}

impl Track {
    /// Whether the subtree stands as the last bind left it.
    pub(super) fn quiet(&self) -> bool {
        !self.fresh && !self.touched && !self.dirty
    }

    /// A control built this bind from `prior`, its replaced self.
    pub(super) fn built(
        reads: Reads,
        observes_scroll: bool,
        origin: Option<Src>,
        prior: Option<Box<Prior>>,
    ) -> Self {
        Self {
            reads,
            observes_scroll,
            fresh: true,
            origin,
            prior,
            ..Self::default()
        }
    }
}

/// Changed data keys between two data sources; `None` when a change reaches
/// every control (creation values, strictness, the screen factory id or
/// component writes on either side).
pub(super) fn changes(old: &DataSource, new: &DataSource) -> Option<Changes> {
    if old.creation_values != new.creation_values
        || old.strict != new.strict
        || old.factory_id != new.factory_id
        || !old.components.is_empty()
        || !new.components.is_empty()
    {
        return None;
    }
    let mut reads = Changes::default();
    diff(&old.globals, &new.globals, |name| {
        reads.add(Data::Global, name, None);
    });
    diff(&old.indexed_globals, &new.indexed_globals, |index| {
        let empty = BTreeMap::new();
        let old = old.indexed_globals.get(index).unwrap_or(&empty);
        let new = new.indexed_globals.get(index).unwrap_or(&empty);
        diff(old, new, |name| reads.add(Data::Global, name, None));
    });
    diff(&old.collections, &new.collections, |key| {
        let (old, new) = (old.collections.get(key), new.collections.get(key));
        // An absent list and an empty one bind differently (a factory's literal count).
        if old.is_some() != new.is_some() {
            reads.add(Data::Length, key, None);
            reads.add(Data::Roles, key, None);
        }
        let old: &[_] = old.map_or(&[], |items| items);
        let new: &[_] = new.map_or(&[], |items| items);
        if old.len() != new.len() {
            reads.add(Data::Length, key, None);
        }
        for index in 0..old.len().max(new.len()) {
            let (before, after) = (old.get(index), new.get(index));
            if before != after {
                reads.add(Data::Item, key, Some(index));
                if before.map(|item| &item.role) != after.map(|item| &item.role) {
                    reads.add(Data::Roles, key, None);
                }
            }
        }
    });
    diff(&old.collection_defaults, &new.collection_defaults, |key| {
        reads.add(Data::Defaults, key, None);
    });
    diff(&old.controls, &new.controls, |name| {
        reads.add(Data::Control, name, None);
    });
    diff(&old.factories, &new.factories, |name| {
        reads.add(Data::Feed, name, None);
    });
    Some(reads)
}

/// Call `changed` for each key whose value differs or exists on one side only.
fn diff<K: Ord, V: PartialEq>(
    old: &BTreeMap<K, V>,
    new: &BTreeMap<K, V>,
    mut changed: impl FnMut(&K),
) {
    let (mut old, mut new) = (old.iter().peekable(), new.iter().peekable());
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => return,
            (Some((key, _)), None) => {
                changed(key);
                old.next();
            }
            (None, Some((key, _))) => {
                changed(key);
                new.next();
            }
            (Some((a, x)), Some((b, y))) => match a.cmp(b) {
                std::cmp::Ordering::Less => {
                    changed(a);
                    old.next();
                }
                std::cmp::Ordering::Greater => {
                    changed(b);
                    new.next();
                }
                std::cmp::Ordering::Equal => {
                    if x != y {
                        changed(a);
                    }
                    old.next();
                    new.next();
                }
            },
        }
    }
}

impl Scope {
    /// Whether a control created under `self` and `other` sees the same scope.
    fn same(&self, other: &Scope) -> bool {
        (Arc::ptr_eq(&self.cursor, &other.cursor) || self.cursor == other.cursor)
            && self.parent_collection == other.parent_collection
            && (Arc::ptr_eq(&self.values, &other.values) || self.values == other.values)
            && (Arc::ptr_eq(&self.for_children, &other.for_children)
                || self.for_children == other.for_children)
            && self.parent_key == other.parent_key
            && self.retained_parent == other.retained_parent
            && self.layout_key == other.layout_key
            && (Arc::ptr_eq(&self.expansions, &other.expansions)
                || self.expansions == other.expansions)
    }
}

impl Binder<'_> {
    /// Whether `previous`, the last bind's control at this key, stands for
    /// rebuilding `src` under `scope`.
    pub(super) fn reusable(&self, previous: &Node, src: &Src, scope: &Scope) -> bool {
        let Some(changes) = &self.changes else {
            return false;
        };
        let track = &previous.track;
        track.stable
            && !track.below.meets(changes)
            && track.origin.as_ref().unwrap_or(&previous.src).same(src)
            && previous.scope.same(scope)
            // A factory's creations are hoisted to its parent, which rebuilds them.
            && !track.hoisted
            && self.created + track.count <= crate::resolve::MAX_NODES
    }

    /// Move `previous` over unchanged, as rebuilding it would leave it.
    pub(super) fn reuse(&mut self, previous: Node) -> Node {
        self.created += previous.track.count;
        self.state.scroll_observed |= previous.track.scroll;
        previous
    }

    /// The memory a rebuilt control continues from, and what it was; its
    /// children wait in the pool for their own rebuild or reuse.
    pub(super) fn replace(&mut self, previous: Node) -> (Option<Retained>, Box<Prior>) {
        let Node {
            src,
            own,
            native,
            mut memory,
            children,
            deferred,
            retained,
            track,
            ..
        } = previous;
        let keys = children.iter().map(|child| child.key).collect();
        for child in children {
            self.pool.insert(child.key, child);
        }
        let prior = Box::new(Prior {
            src,
            retained,
            own: if retained { own.clone() } else { Bag::new() },
            native: if retained {
                native.clone()
            } else {
                Native::default()
            },
            memory: if retained {
                memory.clone()
            } else {
                Retained::default()
            },
            deferred: deferred.is_some(),
            children: keys,
            baked: track.baked,
        });
        let continued = retained.then(|| {
            memory.bag = own;
            memory.native = native.props;
            memory.deferred = deferred.is_some();
            memory
        });
        (continued, prior)
    }

    /// The data keys a control's creation reads, beyond its scope.
    pub(super) fn own_reads(
        &self,
        declaration: &Declaration,
        control: &ResolvedControl,
        name: &str,
        scope: &Scope,
    ) -> Reads {
        let mut reads = Reads::default();
        reads.add(Data::Control, name, None);
        for binding in declaration.bindings.iter() {
            match &binding.kind {
                Kind::Global { source, .. }
                | Kind::View {
                    source,
                    scope: ViewScope::Own,
                    ..
                } => {
                    for property in source.properties() {
                        reads.add(Data::Global, property, None);
                    }
                }
                Kind::Collection {
                    source, collection, ..
                } => {
                    let key = scope
                        .cursor
                        .keys
                        .get(collection)
                        .map_or(collection.as_str(), String::as_str);
                    let index = scope.cursor.indices.get(collection).copied().unwrap_or(0);
                    if let Ok(index) = usize::try_from(index) {
                        reads.add(Data::Item, key, Some(index));
                    }
                    reads.add(Data::Defaults, key, None);
                    if source
                        .properties()
                        .any(|name| name == "#collection_total_items")
                    {
                        reads.add(Data::Length, key, None);
                    }
                }
                Kind::Details { .. } | Kind::View { .. } => {}
            }
        }
        if declaration.radio_group
            && let Some(name) = control
                .properties
                .get("toggle_name")
                .and_then(Value::as_str)
        {
            reads.add(Data::Global, &format!("#radio:{name}"), None);
        }
        if let Some(feed) = control.factory.as_ref().and_then(|f| f.name.as_deref()) {
            reads.add(Data::Feed, feed, None);
        }
        if let Some(collection) = super::feed::collection_name(control) {
            // The list an item-scoped key would name, whether or not it exists yet.
            let scoped = scope
                .cursor
                .path
                .last()
                .map(|(parent, index)| super::scoped_key(parent, *index, collection));
            for key in scoped.as_deref().into_iter().chain([collection]) {
                reads.add(Data::Length, key, None);
                reads.add(Data::Roles, key, None);
            }
        }
        reads
    }
}

/// Settle the bind's bookkeeping bottom-up: counts, reads, fixed points and
/// what changed. Returns whether the subtree changed.
pub(super) fn finish(node: &mut Node) -> bool {
    let mut dirty = node.track.fresh || node.track.touched;
    for child in &mut node.children {
        dirty |= finish(child);
    }
    node.track.dirty = dirty;
    if !dirty {
        return false;
    }
    if node.track.fresh {
        node.track.settled = settled(node);
    } else if node.track.touched {
        node.track.settled = false;
    }
    let track = &mut node.track;
    track.count = 1;
    track.below = track.reads;
    track.scroll = track.observes_scroll;
    // A grid whose cell count a view sets expands only after views settle, so
    // it rebuilds every bind rather than keep cells for a stale count.
    track.stable = track.settled && !super::grid::grid_awaits_views(node.src.get(), &node.bindings);
    track.deferred_below = node.deferred.is_some();
    track.views_below = node
        .bindings
        .iter()
        .any(|binding| matches!(binding.kind, Kind::View { .. }));
    for child in &node.children {
        track.count += child.track.count;
        track.below.union(&child.track.below);
        track.scroll |= child.track.scroll;
        track.stable &= child.track.stable;
        track.deferred_below |= child.track.deferred_below;
        track.views_below |= child.track.views_below;
    }
    true
}

/// Whether a rebuilt control's state equals the state it was rebuilt from.
fn settled(node: &Node) -> bool {
    if !node.retained {
        return true;
    }
    let Some(prior) = node.track.prior.as_deref() else {
        return false;
    };
    prior.retained
        && node.own == prior.own
        && node.native == prior.native
        && node.memory.once == prior.memory.once
        && node.memory.seen == prior.memory.seen
        && node.memory.views == prior.memory.views
        && node.deferred.is_some() == prior.deferred
}

/// How a refreshed tree differs from the one the last bind produced.
pub(crate) enum Patch {
    Same,
    /// The control's new fields (childless) when they changed, and its children's changes.
    Update(Option<Box<ResolvedControl>>, Children),
    /// A whole new tree.
    Full(Box<ResolvedControl>),
}

pub(crate) enum Children {
    /// The same children in the same order, each patched.
    Each(Vec<Patch>),
    /// A new child list.
    Replace(Vec<ResolvedControl>),
}

/// The tree's changes since the last bind, settling its bake memory.
pub(super) fn patch(node: &mut Node, components: &Components) -> Patch {
    if !node.track.fresh && node.track.prior.is_none() && node.track.baked.is_none() {
        return Patch::Full(Box::new(bake_full(node, components)));
    }
    patch_node(node, components)
}

fn patch_node(node: &mut Node, components: &Components) -> Patch {
    if !node.track.dirty {
        return Patch::Same;
    }
    let (previous, same_source, keys) = match node.track.prior.take() {
        Some(prior) => (
            prior.baked,
            same_template(&prior.src, &node.src),
            prior.children,
        ),
        // A control new to the tree replaces nothing a patch can address.
        None if node.track.fresh => return Patch::Full(Box::new(bake_full(node, components))),
        None => {
            let keys = node
                .track
                .touched_children
                .take()
                .unwrap_or_else(|| node.children.iter().map(|child| child.key).collect());
            (node.track.baked.clone(), true, keys)
        }
    };
    let properties = properties(node, components);
    let own = (!same_source || previous.as_ref() != Some(&properties))
        .then(|| Box::new(shell(node, properties.clone())));
    let same_keys = node.children.iter().map(|child| child.key).eq(keys);
    let patch = if same_keys {
        let children: Vec<Patch> = node
            .children
            .iter_mut()
            .map(|child| patch_node(child, components))
            .collect();
        if own.is_none() && children.iter().all(|child| matches!(child, Patch::Same)) {
            Patch::Same
        } else {
            Patch::Update(own, Children::Each(children))
        }
    } else {
        let children = node
            .children
            .iter_mut()
            .map(|child| bake_full(child, components))
            .collect();
        Patch::Update(own, Children::Replace(children))
    };
    node.track.baked = Some(properties);
    clear(node);
    patch
}

/// The whole baked tree, reusing unchanged controls' last properties.
pub(super) fn bake_full(node: &mut Node, components: &Components) -> ResolvedControl {
    let properties = properties(node, components);
    node.track.baked = Some(properties.clone());
    node.track.prior = None;
    node.track.touched_children = None;
    clear(node);
    let mut control = shell(node, properties);
    control.children = node
        .children
        .iter_mut()
        .map(|child| bake_full(child, components))
        .collect();
    control
}

fn clear(node: &mut Node) {
    let track = &mut node.track;
    track.fresh = false;
    track.touched = false;
    track.dirty = false;
}

/// Bag values and bound component state baked into the control's properties.
fn properties(node: &Node, components: &Components) -> Properties {
    let changed = node.track.fresh || node.track.touched;
    if !changed && let Some(baked) = &node.track.baked {
        return baked.clone();
    }
    if components.is_empty()
        && let Some(properties) = output::get(node)
    {
        return properties;
    }
    let properties = super::bake_output(node, components);
    if components.is_empty() {
        output::put(node, properties.clone());
    }
    properties
}

/// The control without children.
fn shell(node: &Node, properties: Properties) -> ResolvedControl {
    let control = node.src.get();
    ResolvedControl {
        name: node.src.name().to_owned(),
        control_type: control.control_type.clone(),
        base: control.base.clone(),
        unresolved_base: control.unresolved_base.clone(),
        properties,
        children: Vec::new(),
        factory: control.factory.clone(),
    }
}

/// Whether two sources bake the same name, type, bases and factory.
fn same_template(old: &Src, new: &Src) -> bool {
    if old.same(new) {
        return true;
    }
    let (a, b) = (old.get(), new.get());
    old.name() == new.name()
        && a.control_type == b.control_type
        && a.base == b.base
        && a.unresolved_base == b.unresolved_base
        && a.factory == b.factory
}

/// The retained memory of `node`'s subtree, for controls dropped from the tree.
pub(super) fn dormant(node: Node, generation: u64, out: &mut KeyMap<Retained>) {
    let Node {
        key,
        own,
        native,
        mut memory,
        children,
        deferred,
        retained,
        ..
    } = node;
    for child in children {
        dormant(child, generation, out);
    }
    if retained {
        memory.bag = own;
        memory.native = native.props;
        memory.generation = generation;
        memory.deferred = deferred.is_some();
        out.insert(key, memory);
    }
}

/// Every retained live control's key with whether its subtree waits hidden.
pub(super) fn live(node: &Node, out: &mut KeyMap<bool>) {
    if node.retained {
        out.insert(node.key, node.deferred.is_some());
    }
    for child in &node.children {
        live(child, out);
    }
}

/// Find the live control at `key`.
pub(super) fn find(node: &Node, key: u64) -> Option<&Node> {
    if node.key == key {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, key))
}

/// The name paths of the controls the last bind built.
pub(super) fn rebuilt_paths(node: &Node, built: &[u64], path: &str, out: &mut Vec<String>) {
    let path = format!("{path}/{}", node.src.name());
    if built.contains(&node.key) {
        out.push(path.clone());
    }
    for child in &node.children {
        rebuilt_paths(child, built, &path, out);
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("name", &self.src.name())
            .field("children", &self.children.len())
            .finish()
    }
}
