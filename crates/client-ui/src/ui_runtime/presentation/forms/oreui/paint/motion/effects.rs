use super::super::super::motion::{Surface, Tween};
use super::{UiPresentationError, transform_nodes};
use crate::ui_runtime::presentation::rect;
use ui::{UiNode, UiNodeId, UiVisual};

const OVERLAY_SECONDS: f64 = 0.100;
const EXIT_SECONDS: f64 = 0.080;

#[cfg(test)]
mod tests;

struct Overlay {
    alpha: Tween,
    page: u16,
}

struct Dialog {
    surface: Surface,
    nodes: Vec<UiNode>,
    rem: f32,
    size: [f32; 2],
    touched: bool,
    exit: Option<f64>,
}

pub(in super::super::super) struct Effects {
    enabled: bool,
    primed: bool,
    overlays: Vec<Overlay>,
    overlay_ids: Vec<UiNodeId>,
    dialogs: Vec<Dialog>,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            enabled: true,
            primed: false,
            overlays: Vec::new(),
            overlay_ids: Vec::new(),
            dialogs: Vec::new(),
        }
    }
}

impl Effects {
    /// A fading overlay or departing dialog still changes the output with time.
    pub(in crate::ui_runtime::presentation) fn active(&self, seconds: f64) -> bool {
        self.overlays.iter().any(|o| o.alpha.active(seconds))
            || self
                .dialogs
                .iter()
                .any(|d| d.exit.is_some_and(|start| seconds - start < EXIT_SECONDS))
    }

    pub(in super::super::super) fn configure(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.overlays.clear();
            self.overlay_ids.clear();
            self.dialogs.clear();
            self.primed = false;
        }
        self.enabled = enabled;
    }

    pub(in super::super::super) fn mark_overlay(&mut self, id: UiNodeId) {
        if self.enabled && !self.overlay_ids.contains(&id) {
            self.overlay_ids.push(id);
        }
    }

    pub(in super::super::super) fn capture(
        &mut self,
        surface: Surface,
        nodes: &[UiNode],
        rem: f32,
        size: [f32; 2],
    ) {
        if !self.enabled || nodes.is_empty() {
            return;
        }
        let index = self
            .dialogs
            .iter()
            .position(|dialog| dialog.surface == surface)
            .unwrap_or_else(|| {
                self.dialogs.push(Dialog {
                    surface,
                    nodes: Vec::new(),
                    rem,
                    size,
                    touched: false,
                    exit: None,
                });
                self.dialogs.len() - 1
            });
        let dialog = &mut self.dialogs[index];
        if dialog.nodes != nodes {
            dialog.nodes.clear();
            dialog.nodes.extend_from_slice(nodes);
        }
        dialog.rem = rem;
        dialog.size = size;
        dialog.touched = true;
        dialog.exit = None;
    }

    pub(in super::super::super) fn finish(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        seconds: f64,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        if !self.enabled {
            return Ok(());
        }
        self.overlays(nodes, next, seconds, size)?;
        self.overlay_ids.clear();
        self.dialogs.retain(|dialog| {
            dialog.size == size
                && dialog
                    .exit
                    .is_none_or(|start| seconds - start < EXIT_SECONDS)
        });
        for dialog in &mut self.dialogs {
            if std::mem::take(&mut dialog.touched) {
                continue;
            }
            let start = *dialog.exit.get_or_insert(seconds);
            let t = ((seconds - start) / EXIT_SECONDS).clamp(0.0, 1.0) as f32;
            let ease = 1.0 - (1.0 - t).powi(3);
            let first = nodes.len();
            let old_first = dialog.nodes[0].id().get();
            let old_last = dialog.nodes.last().unwrap().id().get();
            let base = allocate(next, old_last - old_first + 1)?;
            nodes.extend(dialog.nodes.iter().map(|node| {
                let parent = node
                    .parent()
                    .filter(|parent| parent.get() >= old_first)
                    .map(|parent| UiNodeId::new(base + parent.get() - old_first));
                node.clone()
                    .with_identity(UiNodeId::new(base + node.id().get() - old_first), parent)
                    .with_focusable(false)
            }));
            transform_nodes(nodes, first, 1.0 - ease, dialog.rem * 0.4 * ease, size)?;
        }
        self.primed = true;
        Ok(())
    }

    fn overlays(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        seconds: f64,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let mut count = 0;
        for node in nodes.iter_mut() {
            if !self.overlay_ids.contains(&node.id()) {
                continue;
            }
            let bounds = node.bounds();
            let UiVisual::Solid {
                texture_page,
                color,
            } = node.visual()
            else {
                continue;
            };
            if node.parent().is_some()
                || color[..3] != [0; 3]
                || color[3] == 255
                || bounds.min().x() != 0.0
                || bounds.min().y() != 0.0
                || bounds.max().x() != size[0]
                || bounds.max().y() != size[1]
            {
                continue;
            }
            let target = f32::from(color[3]);
            if count == self.overlays.len() {
                self.overlays.push(Overlay {
                    alpha: Tween::at(if self.primed { 0.0 } else { target }),
                    page: *texture_page,
                });
            }
            let overlay = &mut self.overlays[count];
            overlay.page = *texture_page;
            let alpha = overlay.alpha.retarget(target, OVERLAY_SECONDS, seconds);
            *node = node.clone().with_visual(UiVisual::Solid {
                texture_page: overlay.page,
                color: [0, 0, 0, alpha.round() as u8],
            });
            count += 1;
        }
        for (index, overlay) in self.overlays.iter_mut().enumerate().skip(count) {
            let alpha = overlay.alpha.retarget(0.0, EXIT_SECONDS, seconds);
            if alpha > 0.0 {
                let node = UiNode::new(
                    UiNodeId::new(allocate(next, 1)?),
                    None,
                    rect(0.0, 0.0, size[0], size[1])?,
                )
                .with_visual(UiVisual::Solid {
                    texture_page: overlay.page,
                    color: [0, 0, 0, alpha.round() as u8],
                });
                if index == 0 {
                    nodes.insert(0, node);
                } else {
                    nodes.push(node);
                }
            }
        }
        while self.overlays.len() > count
            && self
                .overlays
                .last()
                .is_some_and(|overlay| overlay.alpha.sample(seconds) == 0.0)
        {
            self.overlays.pop();
        }
        Ok(())
    }
}

fn allocate(next: &mut u32, count: u32) -> Result<u32, UiPresentationError> {
    let first = *next;
    *next = next
        .checked_add(count)
        .ok_or(UiPresentationError::Tree(ui::UiError::DrawIndexOverflow))?;
    Ok(first)
}
