//! Layout retains full-size children while a clipped, fading region changes height.

use super::clip::ClipScope;
use super::{Bounds, Canvas, UiPresentationError, rect};

pub(in super::super) struct RevealScope {
    clip: ClipScope,
    first_node: usize,
    hits: usize,
    focus_hits: usize,
    targets: usize,
    alpha: f32,
    viewport: Bounds,
    fraction: f32,
}

impl Canvas<'_> {
    pub(in super::super) fn begin_reveal(
        &mut self,
        span: [f32; 2],
        top: f32,
        fraction: f32,
    ) -> Result<RevealScope, UiPresentationError> {
        let bottom = self
            .clip
            .map_or(top + 65536.0, |(_, bounds)| bounds[3].max(top));
        let viewport = [span[0], top, span[1], bottom];
        let first_node = self.nodes.len();
        let clip = self.begin_clip(viewport)?;
        let scope = RevealScope {
            clip,
            first_node,
            hits: self.hits.len(),
            focus_hits: self.focus_hits.len(),
            targets: self.focus_targets.len(),
            alpha: self.alpha,
            viewport,
            fraction,
        };
        self.alpha *= fraction;
        Ok(scope)
    }

    pub(in super::super) fn end_reveal(
        &mut self,
        scope: RevealScope,
        bottom: f32,
        interactive: bool,
    ) -> Result<f32, UiPresentationError> {
        let visible_bottom = scope.viewport[1] + (bottom - scope.viewport[1]) * scope.fraction;
        let root = &mut self.nodes[scope.first_node];
        let bounds = root.bounds();
        *root = root.clone().with_bounds(rect(
            bounds.min().x(),
            bounds.min().y(),
            bounds.max().x(),
            bounds.min().y() + (visible_bottom - scope.viewport[1]).max(0.0),
        )?);
        let viewport = [
            scope.viewport[0],
            scope.viewport[1],
            scope.viewport[2],
            visible_bottom.min(scope.viewport[3]),
        ];
        if interactive {
            let clip = |bounds: ui::UiRect| {
                let b = [
                    bounds.min().x().max(viewport[0]),
                    bounds.min().y().max(viewport[1]),
                    bounds.max().x().min(viewport[2]),
                    bounds.max().y().min(viewport[3]),
                ];
                (b[2] > b[0] && b[3] > b[1]).then(|| rect(b[0], b[1], b[2], b[3]).unwrap())
            };
            let mut index = 0;
            self.hits.retain_mut(|(_, bounds)| {
                let keep = index < scope.hits
                    || clip(*bounds).is_some_and(|b| {
                        *bounds = b;
                        true
                    });
                index += 1;
                keep
            });
            index = 0;
            self.focus_hits.retain_mut(|(_, bounds)| {
                let keep = index < scope.focus_hits
                    || clip(*bounds).is_some_and(|b| {
                        *bounds = b;
                        true
                    });
                index += 1;
                keep
            });
            index = 0;
            self.focus_targets.retain_mut(|target| {
                let keep = index < scope.targets
                    || clip(target.bounds).is_some_and(|b| {
                        target.bounds = b;
                        true
                    });
                index += 1;
                keep
            });
        } else {
            self.hits.truncate(scope.hits);
            self.focus_hits.truncate(scope.focus_hits);
            self.focus_targets.truncate(scope.targets);
        }
        self.alpha = scope.alpha;
        self.end_clip(scope.clip);
        Ok(visible_bottom)
    }
}
