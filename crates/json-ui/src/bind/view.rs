//! `view` bindings as the client's property-bag observers: a view registers
//! on its source control's bag once that control resolves, writes when a
//! property it reads is present, and writes again whenever the value it reads
//! changes.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::Value;

use super::apply::BagScope;
use super::native;
use super::spec::{Kind, Source, ViewScope};
use super::{Binder, Node};
use crate::predicate::{self, Scalar};

/// Changed-value notifications followed in one refresh, as the client's bag
/// setter bounds its nesting.
const VIEW_PASSES: usize = 20;
/// Rounds of settling views then building the subtrees they revealed.
const EXPANSION_ROUNDS: usize = 8;

type Path = Vec<usize>;

fn node_at<'n>(root: &'n Node, path: &[usize]) -> &'n Node {
    path.iter().fold(root, |node, &index| &node.children[index])
}

fn node_at_mut<'n>(root: &'n mut Node, path: &[usize]) -> &'n mut Node {
    path.iter()
        .fold(root, |node, &index| &mut node.children[index])
}

/// Every view binding, as `(control path, binding index)`, in tree order.
fn views(node: &Node, path: &mut Path, out: &mut Vec<(Path, usize)>) {
    if node.track.quiet() && !node.track.views_below {
        return;
    }
    for (index, binding) in node.bindings.iter().enumerate() {
        if matches!(binding.kind, Kind::View { .. }) {
            out.push((path.clone(), index));
        }
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        views(child, path, out);
        path.pop();
    }
}

/// The first control breadth-first from `root` for each of `wanted`.
fn first_by_name(root: &Node, wanted: &HashSet<&str>) -> HashMap<String, Path> {
    let mut names = HashMap::new();
    if wanted.is_empty() {
        return names;
    }
    // Breadth-first over (node, parent slot, child index), paths rebuilt only for hits.
    let mut slots: Vec<(usize, usize)> = vec![(usize::MAX, 0)];
    let mut queue = VecDeque::from([(root, 0usize)]);
    while let Some((node, slot)) = queue.pop_front() {
        let name = node.src.name();
        if wanted.contains(name) && !names.contains_key(name) {
            let mut path = Vec::new();
            let mut at = slot;
            while slots[at].0 != usize::MAX {
                path.push(slots[at].1);
                at = slots[at].0;
            }
            path.reverse();
            names.insert(name.to_owned(), path);
            if names.len() == wanted.len() {
                break;
            }
        }
        for (index, child) in node.children.iter().enumerate() {
            slots.push((slot, index));
            queue.push_back((child, slots.len() - 1));
        }
    }
    names
}

/// The first control named `name` breadth-first from the control at `start`.
fn breadth_first(root: &Node, start: &[usize], name: &str) -> Option<Path> {
    let mut queue = VecDeque::from([start.to_vec()]);
    while let Some(path) = queue.pop_front() {
        let node = node_at(root, &path);
        if node.src.name() == name {
            return Some(path);
        }
        for index in 0..node.children.len() {
            let mut child = path.clone();
            child.push(index);
            queue.push_back(child);
        }
    }
    None
}

/// Whether `control`'s authored subtree holds a control named `name`.
fn holds(control: &crate::tree::ResolvedControl, name: &str) -> bool {
    control
        .children
        .iter()
        .any(|child| child.name == name || holds(child, name))
}

impl Binder<'_> {
    /// Settle every view until no observed value changes, building subtrees
    /// views reveal and hidden ones holding a control a view names.
    pub(super) fn settle_views(&mut self, root: &mut Node) {
        let mut tried = HashSet::new();
        let mut settled = false;
        for _ in 0..EXPANSION_ROUNDS {
            let (missed, converged) = self.settle_round(root);
            let mut expanded = false;
            for name in missed {
                if tried.insert(name.clone()) {
                    expanded |= self.expand_named(root, &name);
                }
            }
            expanded |= self.expand_deferred(root, false);
            if !expanded {
                settled = converged;
                break;
            }
        }
        self.state.views_settled = settled;
    }

    /// Build the hidden subtrees whose templates hold a control named `name`,
    /// as the client resolves names through hidden controls.
    fn expand_named(&mut self, node: &mut Node, name: &str) -> bool {
        let mut expanded = false;
        if node.deferred.is_some() && holds(node.src.get(), name) {
            let scope = node.deferred.take().unwrap_or_default();
            self.build_deferred(node, &scope);
            expanded = true;
        }
        for child in &mut node.children {
            expanded |= self.expand_named(child, name);
        }
        node.track.dirty |= expanded;
        expanded
    }

    /// Passes of view notifications; returns the source names that missed and whether no
    /// value changed in the last pass.
    fn settle_round(&mut self, root: &mut Node) -> (Vec<String>, bool) {
        let mut list = Vec::new();
        views(root, &mut Path::new(), &mut list);
        let mut missed = Vec::new();
        if list.is_empty() {
            return (missed, true);
        }
        let wanted: HashSet<&str> = list
            .iter()
            .filter_map(
                |(path, index)| match &node_at(root, path).bindings[*index].kind {
                    Kind::View {
                        control,
                        scope: ViewScope::Global,
                        ..
                    } => Some(control.as_str()),
                    _ => None,
                },
            )
            .collect();
        let names = first_by_name(root, &wanted);
        drop(wanted);
        let sources: Vec<_> = list
            .iter()
            .map(|(path, index)| {
                let source = self.source_of(root, &names, path, *index);
                if let Err(name) = &source {
                    missed.push(name.clone());
                }
                source.ok()
            })
            .collect();
        // Without a fixed point last bind, every view runs as if new.
        let unchanged = self.state.views_settled;
        for _ in 0..VIEW_PASSES {
            let mut changed = false;
            for ((path, index), source) in list.iter().zip(&sources) {
                if unchanged && self.stands(root, path, *index, source.as_deref()) {
                    continue;
                }
                changed |= self.notify(root, path, *index, source.as_deref());
            }
            if !changed {
                return (missed, true);
            }
        }
        (missed, false)
    }

    /// Skips a settled view only when its control, source identity and controller reads stand.
    /// Such a view would observe and write the same values as its last run.
    fn stands(&self, root: &Node, path: &Path, index: usize, source: Option<&[usize]>) -> bool {
        let Some(changes) = &self.changes else {
            return false;
        };
        let node = node_at(root, path);
        let quiet = |node: &Node| !node.track.fresh && !node.track.touched;
        let source_node = source.map(|source| node_at(root, source));
        if !quiet(node)
            || source_node.is_some_and(|source| !quiet(source))
            || node.memory.view_sources.get(&index) != Some(&source_node.map(|source| source.key))
        {
            return false;
        }
        let Kind::View {
            source: expression,
            scope,
            ..
        } = &node.bindings[index].kind
        else {
            return false;
        };
        *scope != ViewScope::Own || !changes.globals(expression.properties())
    }

    /// The control a view reads, or the name that did not resolve.
    fn source_of(
        &self,
        root: &Node,
        names: &HashMap<String, Path>,
        path: &Path,
        index: usize,
    ) -> Result<Path, String> {
        let node = node_at(root, path);
        let Kind::View { control, scope, .. } = &node.bindings[index].kind else {
            return Err(String::new());
        };
        let found = match scope {
            ViewScope::Own => Some(path.clone()),
            ViewScope::Global => names.get(control).cloned(),
            ViewScope::Sibling => path
                .split_last()
                .and_then(|(_, parent)| breadth_first(root, parent, control)),
            ViewScope::Ancestor => (0..path.len())
                .rev()
                .map(|depth| path[..depth].to_vec())
                .find(|ancestor| node_at(root, ancestor).src.name() == control),
        };
        found.ok_or_else(|| control.clone())
    }

    /// Run one view: its first bind applies the target, registration writes a
    /// present value, and a changed value writes again. `true` on a write.
    fn notify(
        &mut self,
        root: &mut Node,
        path: &Path,
        index: usize,
        source: Option<&[usize]>,
    ) -> bool {
        let observed = source.and_then(|source| {
            let node = node_at(root, path);
            let Kind::View {
                source: expression,
                scope,
                ..
            } = &node.bindings[index].kind
            else {
                return None;
            };
            let bag = &node_at(root, source).own;
            observe(expression, bag, &self.env).or_else(|| {
                // A view on its own control reads a property its bag lacks from the
                // screen controller, as server packs that pick layouts by
                // `#title_text` rely on.
                if *scope != ViewScope::Own {
                    return None;
                }
                let answered = Answered {
                    bag,
                    data: self.data,
                };
                observe_in(expression, &answered, &self.env)
            })
        });
        let source_key = source.map(|source| node_at(root, source).key);
        let node = node_at_mut(root, path);
        let Kind::View { target, .. } = &node.bindings[index].kind else {
            return false;
        };
        node.memory.view_sources.insert(index, source_key);
        self.state.views_run += 1;
        let first = node.memory.once.insert(index);
        let registered = node.memory.views.contains_key(&index);
        let mut wrote = false;
        node.track.touched |= first;
        if source.is_some() {
            let fire = observed.is_some() && node.memory.views.get(&index) != Some(&observed);
            if !registered || node.memory.views.get(&index) != Some(&observed) {
                node.memory.views.insert(index, observed.clone());
                node.track.touched = true;
            }
            if let (true, Some(value)) = (fire, observed) {
                wrote = node.own.get(target) != Some(&value);
                let applied = value.to_json();
                node.own.insert(target.clone(), value);
                native::apply(
                    target,
                    &applied,
                    node.src.get(),
                    &node.own,
                    &mut node.native,
                );
            }
        }
        if first {
            let applied = node.own.get(target).map_or(Value::Null, Scalar::to_json);
            native::apply(
                target,
                &applied,
                node.src.get(),
                &node.own,
                &mut node.native,
            );
        }
        wrote
    }
}

/// The value a view reads from `bag`: its property, or its expression when a
/// property it reads is present; nothing when none is.
fn observe(source: &Source, bag: &super::bag::Bag, env: &crate::env::Env) -> Option<Scalar> {
    observe_in(source, &BagScope(bag), env)
}

/// [`observe`] over any property scope.
fn observe_in(
    source: &Source,
    scope: &dyn predicate::Bindings,
    env: &crate::env::Env,
) -> Option<Scalar> {
    let read = |name: &str| {
        scope
            .get(name)
            .filter(|value| value != &Scalar::Json(Value::Null))
    };
    match source {
        Source::Simple(name) => read(name),
        Source::Expression { text, properties } => {
            if !properties.iter().any(|name| read(name).is_some()) {
                return None;
            }
            predicate::eval_scalar(text, env, scope)
        }
    }
}

/// A bag whose missing properties the screen controller answers.
struct Answered<'a> {
    bag: &'a super::bag::Bag,
    data: &'a super::DataSource,
}

impl predicate::Bindings for Answered<'_> {
    fn get(&self, name: &str) -> Option<Scalar> {
        self.bag
            .get(name)
            .or_else(|| {
                self.data
                    .global(super::apply::controller_index(self.bag), name)
            })
            .cloned()
    }
}
