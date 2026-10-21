//! Retains fragment placement so unchanged replays only copy shared node handles.

use std::sync::Arc;

use super::{Canvas, PaintError, UiNode, UiNodeId, intersect, move_clips, replay};

/// One inert fragment's last placement. Callers retain one cache per visible instance.
#[derive(Default)]
pub struct CachedFragment {
    source: Option<Arc<[UiNode]>>,
    placement: Option<Placement>,
    nodes: Vec<UiNode>,
    #[cfg(test)]
    pub(super) builds: usize,
}

#[derive(PartialEq)]
struct Placement {
    size: [f32; 2],
    dest: [f32; 4],
    clip: [f32; 4],
    outer: [f32; 4],
    opacity: f32,
    transform: Option<json_ui::transform::Affine>,
}

impl Canvas<'_> {
    /// Replays shared artwork at `dest`, clipped to `bounds` and the enclosing canvas.
    /// Unchanged source, placement and opacity reuse all prepared geometry and allocations.
    pub fn paint_cached_fragment(
        &mut self,
        cache: &mut CachedFragment,
        source: &Arc<[UiNode]>,
        size: [f32; 2],
        dest: [f32; 4],
        bounds: [f32; 4],
        opacity: f32,
    ) -> Result<(), PaintError> {
        let Some(outer) = self.clip() else {
            return Ok(());
        };
        let clip = if self.residual.is_some() {
            bounds
        } else {
            intersect(bounds, outer)
        };
        let placement = Placement {
            size,
            dest,
            clip,
            outer,
            opacity,
            transform: self.residual,
        };
        if cache.placement.as_ref() != Some(&placement)
            || !cache
                .source
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, source))
        {
            let (mut nodes, _) = replay(source, size, dest, clip, opacity, 0, 0)?;
            if let Some(transform) = self.residual {
                move_clips(&mut nodes, transform, outer)?;
            }
            cache.nodes = nodes;
            #[cfg(test)]
            {
                cache.builds += 1;
            }
            cache.source = Some(Arc::clone(source));
            cache.placement = Some(placement);
        }
        let count = self
            .nodes
            .len()
            .saturating_add(cache.nodes.len())
            .saturating_add(usize::from(!cache.nodes.is_empty()));
        if count > ui::UiLimits::MAX_NODES {
            return Err(PaintError::Tree(ui::UiError::NodeLimitExceeded {
                actual: count,
                limit: ui::UiLimits::MAX_NODES,
            }));
        }
        let after = self
            .next
            .checked_add((cache.nodes.len() + usize::from(!cache.nodes.is_empty())) as u32)
            .ok_or(PaintError::Tree(ui::UiError::DrawIndexOverflow))?;
        for node in &cache.nodes {
            self.nodes.push(node.clone().with_identity(
                UiNodeId::new(*self.next + node.id().get()),
                node.parent().map(|id| UiNodeId::new(*self.next + id.get())),
            ));
        }
        *self.next = after - u32::from(!cache.nodes.is_empty());
        if !cache.nodes.is_empty() {
            self.clip = None;
            self.group(outer)?;
        }
        Ok(())
    }
}
