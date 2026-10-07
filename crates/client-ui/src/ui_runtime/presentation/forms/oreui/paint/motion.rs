use super::super::{
    motion::{Feedback, Kind, Surface, opacity},
    widgets::Interaction,
};
use super::{Canvas, UiPresentationError};
use crate::ui_runtime::presentation::rect;
use ui::{UiNode, UiVisual};

mod effects;
pub(in super::super) use effects::Effects;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(in super::super) struct EntranceScope {
    first: usize,
    previous: Surface,
    progress: f32,
    previous_active: bool,
}

impl Canvas<'_> {
    pub(in super::super) fn overlay(
        &mut self,
        size: [f32; 2],
        color: super::super::theme::Rgba,
    ) -> Result<(), UiPresentationError> {
        let first = self.nodes.len();
        self.fill(
            [0.0, 0.0, size[0], size[1]],
            self.appearance.backdrop(color),
        )?;
        if let Some(node) = self.nodes.get(first)
            && let Some(transitions) = self.transitions.as_deref_mut()
        {
            transitions.effects.mark_overlay(node.id());
        }
        Ok(())
    }

    pub(in super::super) fn feedback(
        &mut self,
        state: Interaction,
        enabled: bool,
        selected: bool,
        kind: Kind,
    ) -> Feedback {
        let target = Feedback::immediate(state, enabled, selected);
        if !enabled {
            return target;
        }
        self.transitions
            .as_deref_mut()
            .map_or(target, |transitions| {
                transitions
                    .motion
                    .feedback(self.surface, state.action, kind, target, self.seconds)
            })
    }

    pub(in super::super) fn begin_entrance(&mut self, surface: Surface) -> EntranceScope {
        let mut progress = self.transitions.as_deref_mut().map_or(1.0, |transitions| {
            transitions.motion.entrance(surface, self.seconds)
        });
        let previous_active = self.entrance_active;
        if previous_active {
            progress = 1.0;
        }
        self.entrance_active |= progress < 1.0;
        let previous = std::mem::replace(&mut self.surface, surface);
        EntranceScope {
            first: self.nodes.len(),
            previous,
            progress,
            previous_active,
        }
    }

    pub(in super::super) fn end_entrance(
        &mut self,
        scope: EntranceScope,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let surface = self.surface;
        self.surface = scope.previous;
        self.entrance_active = scope.previous_active;
        apply_entrance(self.nodes, scope, self.rem, size)?;
        if matches!(surface, Surface::Dialog(_))
            && let Some(transitions) = self.transitions.as_deref_mut()
        {
            transitions
                .effects
                .capture(surface, &self.nodes[scope.first..], self.rem, size);
        }
        Ok(())
    }
}

pub(in super::super) fn apply_entrance(
    nodes: &mut [UiNode],
    scope: EntranceScope,
    rem: f32,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    if scope.progress >= 1.0 || scope.first >= nodes.len() {
        return Ok(());
    }
    let alpha = 0.45 + 0.55 * scope.progress;
    let offset = rem * 0.6 * (1.0 - scope.progress);
    transform_nodes(nodes, scope.first, alpha, offset, size)
}

fn transform_nodes(
    nodes: &mut [UiNode],
    first: usize,
    alpha: f32,
    offset: f32,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    if first >= nodes.len() {
        return Ok(());
    }
    let first_id = nodes[first].id();
    for node in &mut nodes[first..] {
        let bounds = node.bounds();
        if bounds.min().x() == 0.0
            && bounds.min().y() == 0.0
            && bounds.max().x() == size[0]
            && bounds.max().y() == size[1]
            && matches!(
                node.visual(),
                UiVisual::Solid { .. } | UiVisual::Sprite { .. } | UiVisual::Gradient { .. }
            )
        {
            continue;
        }
        let visual = match node.visual().clone() {
            UiVisual::Solid {
                texture_page,
                color,
            } => UiVisual::Solid {
                texture_page,
                color: opacity(color, alpha),
            },
            UiVisual::Sprite {
                texture_page,
                uv,
                color,
            } => UiVisual::Sprite {
                texture_page,
                uv,
                color: opacity(color, alpha),
            },
            UiVisual::RotatedSprite {
                texture_page,
                uv,
                color,
                angle_radians,
            } => UiVisual::RotatedSprite {
                texture_page,
                uv,
                color: opacity(color, alpha),
                angle_radians,
            },
            UiVisual::Text {
                layout,
                color,
                shadow,
            } => UiVisual::Text {
                layout,
                color: opacity(color, alpha),
                shadow,
            },
            UiVisual::RotatedText {
                layout,
                color,
                shadow,
                angle_radians,
            } => UiVisual::RotatedText {
                layout,
                color: opacity(color, alpha),
                shadow,
                angle_radians,
            },
            UiVisual::Gradient {
                texture_page,
                colors,
                horizontal,
            } => UiVisual::Gradient {
                texture_page,
                colors: colors.map(|color| opacity(color, alpha)),
                horizontal,
            },
            UiVisual::Mesh(mesh) => {
                UiVisual::Mesh(std::sync::Arc::new((*mesh).clone().with_opacity(alpha)))
            }
            UiVisual::GlintSprite {
                texture_page,
                uv,
                color,
            } => UiVisual::GlintSprite {
                texture_page,
                uv,
                color: opacity(color, alpha),
            },
            UiVisual::StyledSprite {
                texture_page,
                uv,
                color,
                style,
            } => UiVisual::StyledSprite {
                texture_page,
                uv,
                color: opacity(color, alpha),
                style,
            },
            visual => visual,
        };
        let moved = if node
            .parent()
            .is_none_or(|parent| parent.get() < first_id.get())
        {
            rect(
                bounds.min().x(),
                bounds.min().y() + offset,
                bounds.max().x(),
                bounds.max().y() + offset,
            )?
        } else {
            bounds
        };
        *node = node.clone().with_visual(visual).with_bounds(moved);
    }
    Ok(())
}
