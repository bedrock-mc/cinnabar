//! The programmatic dialog used when the JSON-UI carrier is absent or a form's
//! template cannot resolve: a clipped, scrollable text list on menu surfaces.
use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime, rect};
use super::FormPresentation;
use crate::ui_runtime::{LocalFormAction, UiRuntime};
use protocol::{MenuElement, ServerFormModel};
use std::sync::Arc;
use ui::{SafeArea, TextLayout, UiNode, UiNodeId, UiRect, UiVisual};

impl UiPresentationRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_fallback_form(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<(), UiPresentationError> {
        let Some(entry) = runtime.server_forms().active() else {
            return Ok(());
        };
        // Tiny viewports fail closed to no pointer actions; Escape still works.
        if width < 96.0 || height < 180.0 {
            return Ok(());
        }
        let panel_width = (width - 32.0).min(640.0);
        let panel_height = (height - 48.0).min(640.0);
        let left = (width - panel_width) * 0.5;
        let top = (height - panel_height) * 0.5;
        let text_width = panel_width - 32.0;
        let line_height = metrics.line_height_64 as f32 / 64.0 * metrics.scale.get();
        let base_row = (line_height + 20.0).max(44.0);
        let list_top = top + 64.0;
        let list_height = (panel_height - 80.0 - base_row).max(1.0);
        let list_bottom = list_top + list_height;
        let resolve = |text: &protocol::FormText| runtime.resolve_form_text(text);
        let (title, body_text, labels): (String, String, Vec<Arc<str>>) = match &entry.model {
            ServerFormModel::TextMenu(menu) => (
                resolve(&menu.title),
                resolve(&menu.content),
                menu.buttons.iter().map(|text| Arc::from(resolve(text))).collect(),
            ),
            ServerFormModel::ElementMenu(menu) => (
                resolve(&menu.title),
                resolve(&menu.content),
                menu.elements.iter().filter_map(|element| match element {
                    MenuElement::Button { text, .. } => Some(Arc::from(resolve(text))),
                    _ => None,
                }).collect(),
            ),
            ServerFormModel::NpcDialogue(npc) => (
                npc.npc_name.to_string(),
                npc.dialogue.to_string(),
                npc.buttons.iter().map(|button| Arc::clone(&button.text)).collect(),
            ),
            ServerFormModel::Modal(modal) => (
                resolve(&modal.title),
                resolve(&modal.content),
                vec![Arc::from(resolve(&modal.button1)), Arc::from(resolve(&modal.button2))],
            ),
            ServerFormModel::Custom(_) | ServerFormModel::Unsupported(_) => (
                "Unsupported server form".into(),
                "This form uses controls that are not supported yet. You can close it without submitting an answer.".into(), vec![],
            ),
        };
        let mut body = body_text.as_str();
        let mut buttons = labels.as_slice();
        // Rows grow to the tallest multi-line label; empty lines drop, as a
        // vanilla label discards them.
        let label_lines = |label: &str| {
            label
                .split('\n')
                .filter(|line| !line.is_empty())
                .count()
                .clamp(1, MAX_LABEL_LINES)
        };
        let lines = buttons
            .iter()
            .map(|label| label_lines(label))
            .max()
            .unwrap_or(1);
        let row_height = (line_height * lines as f32 + 20.0).max(44.0);
        let notice = match &entry.model {
            ServerFormModel::TextMenu(menu) if menu.omitted_images > 0 => Some(format!(
                "{} button images omitted. Text-only buttons.",
                menu.omitted_images
            )),
            _ => None,
        };
        // Controlled diagnostic text has its own layout budget, never reducing
        // the valid server-authored body's 16 KiB text budget.
        let notice_layout = notice
            .as_deref()
            .map(|notice| {
                self.layouts
                    .layout(metrics.request(notice, (text_width * 64.0) as u32, &self.font))
                    .map_err(UiPresentationError::Text)
            })
            .transpose()?;
        let notice_height = notice_layout
            .as_ref()
            .map_or(0.0, |layout| layout.size_64()[1] as f32 / 64.0 + 16.0);
        let title_layout = fit_line(self, metrics, &title, text_width)?;
        let body_layout =
            match self
                .layouts
                .layout(metrics.request(body, (text_width * 64.0) as u32, &self.font))
            {
                Ok(layout) => layout,
                Err(_) => {
                    // Server-authored formatting can exhaust the renderer's own
                    // span budget. Remain nonfatal and offer cancel, not fake controls.
                    body = "The server's text could not be displayed. Close this form.";
                    buttons = &[];
                    self.layouts
                        .layout(metrics.request(body, (text_width * 64.0) as u32, &self.font))
                        .map_err(UiPresentationError::Text)?
                }
            };
        let body_height = if body.is_empty() {
            0.0
        } else {
            body_layout.size_64()[1] as f32 / 64.0 + 16.0
        };
        let text_height = notice_height + body_height;
        let content_height = text_height + buttons.len() as f32 * row_height;
        let maximum = (content_height - list_height).max(0.0) as usize;
        let scroll = runtime.server_forms().scroll().min(maximum);
        let mut state = FormPresentation {
            identity: Some(entry.identity),
            scroll,
            height: list_height as usize,
            maximum,
            row_height: row_height as usize,
            ..FormPresentation::default()
        };
        solid(
            nodes,
            next,
            self.solid_texture_page,
            rect(0.0, 0.0, width, height)?,
            [4, 6, 10, 214],
        );
        solid(
            nodes,
            next,
            self.solid_texture_page,
            rect(left, top, left + panel_width, top + panel_height)?,
            [22, 29, 39, 255],
        );
        let title_clip = clip(
            nodes,
            next,
            rect(
                left + 16.0,
                top + 16.0,
                left + panel_width - 16.0,
                top + 56.0,
            )?,
        );
        text(
            nodes,
            next,
            title_clip,
            title_layout,
            metrics,
            rect(0.0, 0.0, text_width, 40.0)?,
            [239, 243, 247, 255],
        );
        let list_clip = clip(
            nodes,
            next,
            rect(
                left + 16.0,
                list_top,
                left + panel_width - 16.0,
                list_bottom,
            )?,
        );
        if let Some(notice_layout) = notice_layout {
            text(
                nodes,
                next,
                list_clip,
                notice_layout,
                metrics,
                rect(
                    0.0,
                    -(scroll as f32),
                    text_width,
                    notice_height - scroll as f32,
                )?,
                [166, 178, 193, 255],
            );
        }
        if !body.is_empty() {
            text(
                nodes,
                next,
                list_clip,
                body_layout,
                metrics,
                rect(
                    0.0,
                    notice_height - scroll as f32,
                    text_width,
                    text_height - scroll as f32,
                )?,
                [166, 178, 193, 255],
            );
        }
        for (index, label) in buttons.iter().enumerate() {
            let offset = text_height + index as f32 * row_height;
            state.offsets.push(offset as usize);
            let y = offset - scroll as f32;
            if y + row_height <= 0.0 || y >= list_height {
                continue;
            }
            let bounds = rect(0.0, y, text_width, y + row_height - 6.0)?;
            nodes.push(
                UiNode::new(UiNodeId::new(*next), Some(list_clip), bounds).with_visual(
                    UiVisual::Solid {
                        texture_page: self.solid_texture_page,
                        color: if runtime.server_forms().focus() == index {
                            [55, 112, 151, 255]
                        } else {
                            [43, 54, 70, 255]
                        },
                    },
                ),
            );
            *next = next.saturating_add(1);
            let mut carry = String::new();
            let label_lines = label.split('\n').filter(|line| !line.is_empty());
            for (row, line) in label_lines.take(MAX_LABEL_LINES).enumerate() {
                let source = format!("{carry}{line}");
                carry = super::engine::active_codes(&source);
                let layout = fit_line(self, metrics, &source, (text_width - 24.0).max(1.0))?;
                let top = y + 10.0 + row as f32 * line_height;
                text(
                    nodes,
                    next,
                    list_clip,
                    layout,
                    metrics,
                    rect(12.0, top, text_width - 12.0, top + line_height)?,
                    [239, 243, 247, 255],
                );
            }
            // Only whole visible controls are actionable, never cropped rows.
            if y >= 0.0 && y + row_height - 6.0 <= list_height {
                state.hits.push((
                    LocalFormAction::SubmitButton(index as u32),
                    window_rect(
                        rect(
                            left + 16.0,
                            list_top + y,
                            left + panel_width - 16.0,
                            list_top + y + row_height - 6.0,
                        )?,
                        self.safe_area,
                    )?,
                ));
            }
        }
        state.offsets.push(scroll); // fixed cancel never needs list movement
        let cancel_top = top + panel_height - base_row - 12.0;
        let cancel = rect(
            left + 16.0,
            cancel_top,
            left + panel_width - 16.0,
            cancel_top + base_row - 6.0,
        )?;
        solid(
            nodes,
            next,
            self.solid_texture_page,
            cancel,
            if runtime.server_forms().focus() == buttons.len() {
                [55, 112, 151, 255]
            } else {
                [43, 54, 70, 255]
            },
        );
        let cancel_clip = clip(nodes, next, cancel);
        let cancel_layout = fit_line(self, metrics, "Close", text_width - 24.0)?;
        text(
            nodes,
            next,
            cancel_clip,
            cancel_layout,
            metrics,
            rect(12.0, 10.0, text_width - 12.0, base_row - 8.0)?,
            [239, 243, 247, 255],
        );
        state.hits.push((
            LocalFormAction::Dismiss,
            window_rect(cancel, self.safe_area)?,
        ));
        let retained = &mut self.form_presentation;
        retained.identity = state.identity;
        retained.scroll = state.scroll;
        retained.height = state.height;
        retained.maximum = state.maximum;
        retained.row_height = state.row_height;
        retained.offsets = state.offsets;
        retained.hits = state.hits;
        retained.frame = None;
        Ok(())
    }
}

/// Most lines a fallback button label shows.
const MAX_LABEL_LINES: usize = 4;

pub(super) fn fit_line(
    presentation: &mut UiPresentationRuntime,
    metrics: TextMetrics,
    value: &str,
    width: f32,
) -> Result<Arc<TextLayout>, UiPresentationError> {
    let ends = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect::<Vec<_>>();
    let wrap = (width.max(1.0) * 64.0) as u32;
    let mut low = 0;
    let mut high = ends.len();
    let mut best = presentation
        .layouts
        .layout(metrics.request("", wrap, &presentation.font))
        .map_err(UiPresentationError::Text)?;
    while low < high {
        let middle = low + (high - low) / 2;
        let end = ends[middle];
        let text = if end < value.len() {
            format!("{}…", &value[..end])
        } else {
            value.to_owned()
        };
        let candidate =
            presentation
                .layouts
                .layout(metrics.request(&text, wrap, &presentation.font));
        let Ok(candidate) = candidate else {
            high = middle;
            continue;
        };
        let shadow = match metrics.shadow() {
            ui::TextShadow::None => 0.0,
            ui::TextShadow::Offset64(offset) => offset as f32 / 64.0,
        };
        if candidate.line_count() <= 1 && candidate.size_64()[0] as f32 / 64.0 + shadow <= width {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Ok(best)
}
pub(super) fn solid(
    nodes: &mut Vec<UiNode>,
    next: &mut u32,
    page: u16,
    bounds: UiRect,
    color: [u8; 4],
) {
    nodes.push(
        UiNode::new(UiNodeId::new(*next), None, bounds).with_visual(UiVisual::Solid {
            texture_page: page,
            color,
        }),
    );
    *next = next.saturating_add(1);
}
pub(super) fn clip(nodes: &mut Vec<UiNode>, next: &mut u32, bounds: UiRect) -> UiNodeId {
    let id = UiNodeId::new(*next);
    nodes.push(UiNode::new(id, None, bounds).with_clip_children(true));
    *next = next.saturating_add(1);
    id
}
pub(super) fn text(
    nodes: &mut Vec<UiNode>,
    next: &mut u32,
    parent: UiNodeId,
    layout: Arc<TextLayout>,
    metrics: TextMetrics,
    bounds: UiRect,
    color: [u8; 4],
) {
    nodes.push(
        UiNode::new(UiNodeId::new(*next), Some(parent), bounds).with_visual(UiVisual::Text {
            layout,
            color,
            shadow: metrics.shadow(),
        }),
    );
    *next = next.saturating_add(1);
}
pub(super) fn window_rect(bounds: UiRect, safe: SafeArea) -> Result<UiRect, UiPresentationError> {
    rect(
        bounds.min().x() + safe.left(),
        bounds.min().y() + safe.top(),
        bounds.max().x() + safe.left(),
        bounds.max().y() + safe.top(),
    )
}
