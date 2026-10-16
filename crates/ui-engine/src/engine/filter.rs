//! Element filters: the draws of a filtered surface subtree paint into one filter group, which
//! a closing [`UiBlendMode::Filter`] node composites blurred. Groups follow paint order, so a
//! subtree interleaved with other layers closes and reopens as separate groups.

use std::{cell::RefCell, sync::Arc};

use json_ui::{DrawNode, filter::FilterGroup};
use render_api::UiBackdropPaint;
use ui::{UiBlendMode, UiVisual};

use super::{
    Canvas,
    glass::{cover_quad, paint_bits},
    rounded::MeshCache,
    screen::PaintError,
};

thread_local! {
    /// Whole-content filter composites, keyed by page and paint.
    static COMPOSITES: RefCell<MeshCache<(u16, [u32; 3])>> = RefCell::default();
}

/// The filter group currently receiving draws and the identifier its nodes carry.
pub(super) struct OpenFilter {
    group: Arc<FilterGroup>,
    id: u32,
}

impl Canvas<'_> {
    /// Routes `node`'s draws into its filter group: closes the open group when `node` leaves
    /// it and opens the node's own. Call before painting `node`.
    pub(super) fn enter_filter(&mut self, node: &DrawNode) -> Result<(), PaintError> {
        let group = node.anim.as_ref().and_then(|anim| anim.filter.as_ref());
        if let (Some(open), Some(group)) = (&self.filter, group)
            && (Arc::ptr_eq(&open.group, group) || *open.group == **group)
        {
            return Ok(());
        }
        self.close_filter()?;
        if let Some(group) = group {
            let id = *self.next;
            *self.next = self.next.saturating_add(1);
            self.filter = Some(OpenFilter {
                group: Arc::clone(group),
                id,
            });
        }
        Ok(())
    }

    /// Places nodes painted since `start` in the open filter group.
    pub(super) fn tag_filtered(&mut self, start: usize) {
        let Some(id) = self.filter.as_ref().map(|open| open.id) else {
            return;
        };
        for node in &mut self.nodes[start..] {
            *node = node.clone().with_filter_group(id);
        }
    }

    /// Composites the open filter group, if any, over the whole content area; the renderer
    /// bounds the work to what the group drew.
    pub(super) fn close_filter(&mut self) -> Result<(), PaintError> {
        let Some(open) = self.filter.take() else {
            return Ok(());
        };
        let paint = UiBackdropPaint {
            blur: (open.group.filter.blur as f32) * self.px * self.metrics.dpi_scale.get(),
            ..UiBackdropPaint::IDENTITY
        }
        .bounded();
        let page = self.solid_page;
        let Some(mesh) = COMPOSITES.with(|cache| {
            cache
                .borrow_mut()
                .get_or_build((page, paint_bits(paint)), |(page, _)| {
                    cover_quad(page, paint, UiBlendMode::Filter)
                })
        }) else {
            return Ok(());
        };
        let screen = self.screen;
        self.group(screen)?;
        self.push(UiVisual::Mesh(mesh), screen)
    }
}
