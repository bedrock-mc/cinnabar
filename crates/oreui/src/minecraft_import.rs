//! Import uses the existing OreUI modal, menu rows and primary/secondary buttons.

use super::{
    Look, OreUiScreen,
    modal::{self, Modal},
    paint::Canvas,
    theme::{Appearance, TEXT},
    widgets::{MenuItem, Variant},
};
use launcher::{
    menu::{MenuAction, MenuView},
    minecraft_import::{Action, Phase},
};
use ui::UiRect;
use ui_engine::{
    engine::{
        rect,
        screen::{PaintError, PaintFrame},
    },
    menu_scroll::MenuScrolls,
};

/// Import previews leave space around the panel, even when their contents overflow.
const MAX_HEIGHT_FRACTION: f32 = 0.8;

impl OreUiScreen {
    /// Draws the consent, profile, preview and restore steps with the standard modal controls.
    pub fn paint_minecraft_import(
        &mut self,
        view: &MenuView,
        size: [f32; 2],
        scrolls: &mut MenuScrolls<MenuAction>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<Vec<(MenuAction, UiRect)>, PaintError> {
        let state = &view.minecraft_import;
        let action = |action| MenuAction::MinecraftImport(action);
        let items = if state.phase == Phase::Profiles {
            state
                .profiles
                .iter()
                .enumerate()
                .map(|(index, label)| MenuItem {
                    label,
                    picture_slot: false,
                    picture: None,
                    selected: false,
                    enabled: true,
                    action: Some(action(Action::Select(index))),
                })
                .collect()
        } else {
            Vec::new()
        };
        let buttons = state
            .actions()
            .into_iter()
            .filter(|action| !matches!(action, Action::Select(_)))
            .enumerate()
            .map(|(index, value)| {
                (
                    state.label(value).into(),
                    if index == 0 {
                        Variant::Primary
                    } else {
                        Variant::Secondary
                    },
                    Some(action(value)),
                )
            })
            .collect();
        let close = if state.phase == Phase::Busy {
            None
        } else if state.phase == Phase::Offered {
            Some(action(Action::NotNow))
        } else {
            Some(action(Action::Close))
        };
        let dialog = Modal {
            title: state.title(),
            items,
            body: if state.pack_status.is_empty() {
                state.body().into()
            } else {
                format!("{}\n\n{}", state.body(), state.pack_status).into()
            },
            body_color: TEXT,
            buttons,
            close,
        };
        let originals = self
            .originals
            .clone()
            .filter(|_| self.look == Look::Originals);
        let rollback = (frame.nodes.len(), *frame.next);
        let mut offsets = scrolls.offsets().clone();
        let mut first = true;
        loop {
            frame.nodes.truncate(rollback.0);
            *frame.next = rollback.1;
            let mut canvas = Canvas::new(
                &mut *frame.nodes,
                &mut *frame.next,
                &mut *frame.layouts,
                frame.font,
                frame.metrics,
                frame.solid_page,
                originals.as_deref(),
            );
            canvas.appearance = Appearance::from_dark(view.settings_options.oreui_dark_mode());
            canvas.seconds = frame.now;
            canvas.transitions = Some(&mut self.transitions);
            canvas.offsets = offsets.clone();
            let focused = modal::draw_bounded(
                &mut canvas,
                view,
                size,
                &dialog,
                size[1] * MAX_HEIGHT_FRACTION,
            )?;
            let (hits, areas) = (canvas.hits, canvas.scrolls);
            let used = offsets.get(modal::SCROLL).copied().unwrap_or(0.0);
            if focused.is_none() {
                scrolls.observe_focus(view.focused_action);
            }
            let revealed = focused.and_then(|item| {
                let bounds = |b: super::paint::Bounds| rect(b[0], b[1], b[2], b[3]).ok();
                Some(scrolls.reveal_focus(
                    modal::SCROLL,
                    view.focused_action,
                    bounds(item.bounds),
                    bounds(item.viewport)?,
                    item.max,
                ))
            });
            match revealed {
                Some(offset) if first && (offset - used).abs() > f32::EPSILON => {
                    offsets.insert(modal::SCROLL.to_owned(), offset);
                    first = false;
                }
                _ => {
                    scrolls.set_areas(areas);
                    return Ok(hits);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
