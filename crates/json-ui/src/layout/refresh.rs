//! Preserve control addresses while invalidating changed measurements.

use crate::ResolvedControl;
use crate::bind::{Children, Patch};

use super::measure::Dirty;

/// Apply a fresh binding, marking changed controls and their ancestors.
pub(super) fn update(
    tree: &mut ResolvedControl,
    mut next: ResolvedControl,
    dirty: &mut Dirty,
) -> bool {
    let children = std::mem::take(&mut next.children);
    let changed = update_children(tree, children, dirty);
    finish(tree, next, changed, dirty)
}

/// Apply a binder patch: [`update`] for the controls it carries, nothing for
/// those it reports unchanged.
pub(super) fn apply(tree: &mut ResolvedControl, patch: Patch, dirty: &mut Dirty) -> bool {
    let (next, children) = match patch {
        Patch::Same => return false,
        Patch::Full(next) => return update(tree, *next, dirty),
        Patch::Update(next, children) => (next, children),
    };
    let changed = match children {
        Children::Each(patches) => {
            assert_eq!(
                patches.len(),
                tree.children.len(),
                "a patch must address the tree its bind produced"
            );
            let mut changed = false;
            for (child, patch) in tree.children.iter_mut().zip(patches) {
                changed |= apply(child, patch, dirty);
            }
            changed
        }
        Children::Replace(children) => update_children(tree, children, dirty),
    };
    match next {
        Some(next) => finish(tree, *next, changed, dirty),
        None => {
            if changed {
                dirty.changed(std::ptr::from_ref(tree).addr());
            }
            changed
        }
    }
}

/// Take `next`'s own fields, marking the control when they or its children changed.
fn finish(
    tree: &mut ResolvedControl,
    next: ResolvedControl,
    children_changed: bool,
    dirty: &mut Dirty,
) -> bool {
    let changed = children_changed
        || tree.name != next.name
        || tree.control_type != next.control_type
        || tree.properties != next.properties
        || tree.base != next.base
        || tree.unresolved_base != next.unresolved_base
        || tree.factory != next.factory;
    tree.name = next.name;
    tree.control_type = next.control_type;
    tree.properties = next.properties;
    tree.base = next.base;
    tree.unresolved_base = next.unresolved_base;
    tree.factory = next.factory;
    if changed {
        dirty.changed(std::ptr::from_ref(tree).addr());
    }
    changed
}

/// Update children in place when names and indices line up, else replace them.
fn update_children(
    tree: &mut ResolvedControl,
    next: Vec<ResolvedControl>,
    dirty: &mut Dirty,
) -> bool {
    let same_children = tree.children.len() == next.len()
        && tree.children.iter().zip(&next).all(|(a, b)| {
            a.name == b.name
                && a.properties.get("collection_index") == b.properties.get("collection_index")
        });
    if !same_children {
        for child in &tree.children {
            mark_subtree(child, dirty);
        }
        tree.children = next;
        return true;
    }
    let mut changed = false;
    for (child, next) in tree.children.iter_mut().zip(next) {
        changed |= update(child, next, dirty);
    }
    changed
}

/// Remove every address before replacing a child allocation.
fn mark_subtree(tree: &ResolvedControl, dirty: &mut Dirty) {
    dirty.removed(std::ptr::from_ref(tree).addr());
    for child in &tree.children {
        mark_subtree(child, dirty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutEnv, MeasureCache, TextMeasure, TextureMeta, TextureSource, ViewState};
    use serde_json::{Value, json};
    use std::cell::Cell;

    struct Metrics(Cell<usize>);
    impl TextMeasure for Metrics {
        fn extent(&self, _: &str) -> [f64; 2] {
            self.0.set(self.0.get() + 1);
            [20.0, 9.0]
        }
    }
    impl TextureSource for Metrics {
        fn texture(&self, _: &str) -> Option<TextureMeta> {
            None
        }
    }

    /// Build a small control without resolving a pack.
    fn node(
        name: &str,
        kind: &str,
        properties: Value,
        children: Vec<ResolvedControl>,
    ) -> ResolvedControl {
        ResolvedControl {
            name: name.into(),
            control_type: Some(kind.into()),
            properties: properties
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            children,
            base: None,
            unresolved_base: None,
            factory: None,
        }
    }

    #[test]
    fn moving_the_root_keeps_fitting_scrollbar_measurements() {
        let label = node("content", "label", json!({"text":"short"}), vec![]);
        let viewport = node("viewport", "panel", json!({"size":[100,100]}), vec![label]);
        let track = node(
            "track",
            "panel",
            json!({"size":[4,100]}),
            vec![node(
                "box",
                "scrollbar_box",
                json!({"size":[4,10],"draggable":"vertical"}),
                vec![],
            )],
        );
        let view = node(
            "view",
            "scroll_view",
            json!({
                "size":[100,100], "scroll_content":"content", "scroll_view_port":"viewport",
                "scrollbar_track":"track", "scrollbar_box":"box", "scroll_box_and_track_panel":"bars"
            }),
            vec![
                viewport,
                node("bars", "panel", json!({"size":[4,100]}), vec![track]),
            ],
        );
        let boxed = Box::new(node("root", "panel", json!({"size":[100,100]}), vec![view]));
        let metrics = Metrics(Cell::new(0));
        let env = LayoutEnv {
            text: &metrics,
            textures: &metrics,
        };
        let mut cache = MeasureCache::default();
        let state = ViewState::default();
        let (_, first) =
            super::super::layout_reusing(&boxed, [100.0, 100.0], &env, &state, &mut cache);
        assert_eq!(first.scrolls["/root/view"].bar_visible, Some(false));
        assert!(metrics.0.get() > 0);
        let moved = *boxed;
        metrics.0.set(0);
        let (_, next) =
            super::super::layout_reusing(&moved, [100.0, 100.0], &env, &state, &mut cache);
        assert_eq!(first, next);
        assert_eq!(
            metrics.0.get(),
            0,
            "moving the root must not flush descendant measurements"
        );
    }
}
