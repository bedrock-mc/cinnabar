//! Local clipping reveals expanding content without creating nested scroll areas.

use super::{Bounds, Canvas, UiNode, UiNodeId, UiPresentationError};

pub(in super::super) struct ClipScope(Option<(UiNodeId, Bounds)>);

impl Canvas<'_> {
    pub(in super::super) fn begin_clip(
        &mut self,
        viewport: Bounds,
    ) -> Result<ClipScope, UiPresentationError> {
        let area = self.local(viewport)?;
        let id = UiNodeId::new(*self.next);
        let parent = self.clip.map(|(id, _)| id);
        self.nodes
            .push(UiNode::new(id, parent, area).with_clip_children(true));
        *self.next = self.next.saturating_add(1);
        Ok(ClipScope(self.clip.replace((id, viewport))))
    }

    pub(in super::super) fn end_clip(&mut self, scope: ClipScope) {
        self.clip = scope.0;
    }
}
