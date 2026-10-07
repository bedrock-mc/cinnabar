//! Retained JSON-UI artwork and its rendering identity for the F3 overlay.

use std::sync::Arc;

use ui::{SafeArea, UiNode, UiNodeId};

/// Rendering inputs whose changes require repainting the retained overlay.
#[derive(Clone, Copy, PartialEq)]
pub(in super::super) struct PaintKey {
    pub(in super::super) content: [f32; 2],
    pub(in super::super) scale: [f32; 2],
    pub(in super::super) line: [u32; 2],
    pub(in super::super) solid_page: u16,
    pub(in super::super) safe_area: SafeArea,
}

/// Painted JSON-UI nodes retain their shared glyph runs between publications.
#[derive(Default)]
pub(in super::super) struct PaintedOverlay {
    pub(in super::super) key: Option<PaintKey>,
    pub(in super::super) nodes: Vec<UiNode>,
    pub(in super::super) count: u32,
    pub(in super::super) catalog: Option<Arc<json_ui::Catalog>>,
}

impl PaintedOverlay {
    /// Reparents cached artwork into this frame without copying glyph or mesh storage.
    pub(in super::super) fn append(&self, nodes: &mut Vec<UiNode>, next: &mut u32) {
        let offset = next.saturating_sub(1);
        nodes.extend(self.nodes.iter().map(|node| {
            let id = UiNodeId::new(node.id().get().saturating_add(offset));
            let parent = node
                .parent()
                .map(|id| UiNodeId::new(id.get().saturating_add(offset)));
            node.clone().with_identity(id, parent)
        }));
        *next = next.saturating_add(self.count);
    }
}
