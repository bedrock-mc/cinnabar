//! The binder's view of a control: a node of a shared resolved tree, read in
//! place, plus the name and properties the binder gives a created instance.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::tree::ResolvedControl;

/// A control the binder reads in place: a node of a shared tree, plus the
/// name and properties the binder gives a created instance.
#[derive(Clone)]
pub(super) struct Src {
    tree: Arc<ResolvedControl>,
    path: Vec<u32>,
    pub(super) patch: Option<Arc<Patch>>,
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Patch {
    pub(super) name: Option<String>,
    pub(super) properties: BTreeMap<String, Value>,
}

impl Src {
    /// Whether literal-child expansion will hoist this factory's creations.
    pub(super) fn is_authored_child(&self) -> bool {
        !self.path.is_empty()
    }

    /// The immutable tree that owns this control's template.
    pub(super) fn owner(&self) -> &Arc<ResolvedControl> {
        &self.tree
    }

    pub(super) fn root(tree: Arc<ResolvedControl>) -> Self {
        Self {
            tree,
            path: Vec::new(),
            patch: None,
        }
    }

    pub(super) fn get(&self) -> &ResolvedControl {
        self.path
            .iter()
            .fold(&*self.tree, |node, &index| &node.children[index as usize])
    }

    /// Extend the path with one allocation, reserving space for the child index.
    pub(super) fn child(&self, index: usize) -> Self {
        let mut path = Vec::with_capacity(self.path.len() + 1);
        path.extend_from_slice(&self.path);
        path.push(index as u32);
        Self {
            tree: Arc::clone(&self.tree),
            path,
            patch: None,
        }
    }

    pub(super) fn name(&self) -> &str {
        match self.patch.as_ref().and_then(|patch| patch.name.as_deref()) {
            Some(name) => name,
            None => &self.get().name,
        }
    }

    pub(super) fn prop(&self, key: &str) -> Option<&Value> {
        self.patch
            .as_ref()
            .and_then(|patch| patch.properties.get(key))
            .or_else(|| self.get().properties.get(key))
    }

    /// This source with `edit` applied to its patch.
    pub(super) fn patched(mut self, edit: impl FnOnce(&mut Patch)) -> Self {
        let mut patch = self.patch.as_deref().cloned().unwrap_or_default();
        edit(&mut patch);
        self.patch = Some(Arc::new(patch));
        self
    }
}
