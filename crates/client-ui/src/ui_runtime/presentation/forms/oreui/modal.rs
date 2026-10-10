//! OreUI's modal: a bordered panel over a dimmed screen, with a neutral header (centred
//! title, X close), a scrolling content area (menu items on the darkest surface, then
//! text) and a bevelled button tray stacking full-width buttons. The local-world dialogs
//! and the accounts picker are built on it.

use std::borrow::Cow;

use super::super::super::UiPresentationError;
use super::focus;
use super::icons::{self, Icon};
use super::motion::{Kind, Surface, opacity};
use super::paint::{Bounds, Canvas};
use super::theme::{
    BEVEL_DARK, BEVEL_LIGHT, BODY, BORDER, CAPTION, EDGE, MENU_ITEM, NEUTRAL, NEUTRAL80,
    NEUTRAL100, OUTLINE, OVERLAY_MODAL, Rgba, TEXT,
};
use super::widgets::{MenuItem, Variant, button, menu_item};
use launcher::local_worlds::{PromptButton, Screen, WorldsView};
use launcher::menu::view::SettingsFocusAxis;
use launcher::menu::{LocalWorldAction, MenuAction, MenuView};

/// One modal; a button without an action draws disabled.
pub(super) struct Modal<'a> {
    pub(super) title: &'a str,
    pub(super) items: Vec<MenuItem<'a>>,
    pub(super) body: Cow<'a, str>,
    pub(super) body_color: Rgba,
    pub(super) buttons: Vec<(Cow<'a, str>, Variant, Option<MenuAction>)>,
    pub(super) close: Option<MenuAction>,
}

impl<'a> Modal<'a> {
    fn text(title: &'a str, body: impl Into<Cow<'a, str>>) -> Self {
        Self {
            title,
            items: Vec::new(),
            body: body.into(),
            body_color: TEXT,
            buttons: Vec::new(),
            close: None,
        }
    }
}

const MAX_WIDTH: f32 = 47.6;
const HEADER: f32 = 4.8;
const ITEM: f32 = 4.8;
const BUTTON: f32 = 4.4;
/// Text and button tray padding.
const PAD: f32 = 1.6;
/// The overlay's padding above and below the panel.
const MARGIN: f32 = 1.2;

#[cfg(test)]
mod tests;

/// Where the keyboard-focused menu item lies unscrolled, for scrolling it into view.
pub(super) struct FocusedItem {
    pub(super) bounds: Bounds,
    pub(super) viewport: Bounds,
    pub(super) max: f32,
}

/// The modal's scroll view key.
pub(super) const SCROLL: &str = "oreui_modal";

/// Draws `modal` centred over the screen; only its own controls take presses.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    modal: &Modal<'_>,
) -> Result<Option<FocusedItem>, UiPresentationError> {
    draw_inner(canvas, view, size, modal, false)
}

/// Pickers dismiss when the player presses outside the panel.
pub(super) fn draw_picker(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    modal: &Modal<'_>,
) -> Result<Option<FocusedItem>, UiPresentationError> {
    draw_inner(canvas, view, size, modal, true)
}

fn draw_inner(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    modal: &Modal<'_>,
    dismiss_overlay: bool,
) -> Result<Option<FocusedItem>, UiPresentationError> {
    canvas.hits.clear();
    canvas.overlay(size, OVERLAY_MODAL)?;
    let edge = canvas.r(EDGE);
    let width = canvas.r(MAX_WIDTH).min(size[0]);
    let pad = canvas.r(PAD);
    let inner = width - edge * 2.0;
    // A menu list sits over the header's shadow strip; text sits below it.
    let list = if modal.items.is_empty() {
        0.0
    } else {
        modal.items.len() as f32 * canvas.r(ITEM) + edge * 3.0
    };
    let text = if modal.body.is_empty() {
        0.0
    } else {
        canvas.measure_height(&modal.body, inner - pad * 2.0, CAPTION)? + pad * 2.0
    };
    let header = canvas.r(HEADER) + if list > 0.0 { 0.0 } else { edge };
    let buttons = modal.buttons.len() as f32;
    let tray = if buttons > 0.0 {
        buttons * canvas.r(BUTTON) + (buttons - 1.0) * canvas.r(0.4) + pad * 2.0
    } else {
        0.0
    };
    let overlap = if dismiss_overlay && list > 0.0 {
        edge
    } else {
        0.0
    };
    let room = size[1] - canvas.r(MARGIN) * 2.0 - edge * 2.0 - header - tray + overlap;
    let room = room.max(0.0);
    let item = canvas.r(ITEM);
    // An overflowing menu shows whole items less half of one, so a cut row signals scrolling.
    let content = if list > 0.0 && list + text > room {
        ((room / item).floor() * item - item * 0.5).max(item * 0.5)
    } else {
        (list + text).min(room)
    };
    let height = edge * 2.0 + header + content + tray - overlap;
    let left = (size[0] - width) * 0.5;
    let top = ((size[1] - height) * 0.5).max(0.0);
    let panel = [left, top, left + width, top + height];
    let native_picker = dismiss_overlay && canvas.capture_focus;
    let picker_focus = if native_picker {
        canvas.clear_focus_geometry();
        let parent = canvas.begin_focus_region(focus::PICKER, panel, None, true)?;
        canvas.focus_trap();
        canvas.focus_delegate(
            modal.items.iter().find_map(|item| {
                (item.selected && item.enabled)
                    .then_some(item.action)
                    .flatten()
            }),
            None,
        );
        Some(parent)
    } else {
        None
    };
    if dismiss_overlay && let Some(close) = modal.close {
        let capture = canvas.capture_focus;
        canvas.capture_focus = false;
        for bounds in [
            [0.0, 0.0, size[0], panel[1]],
            [0.0, panel[3], size[0], size[1]],
            [0.0, panel[1], panel[0], panel[3]],
            [panel[2], panel[1], size[0], panel[3]],
        ] {
            if bounds[2] > bounds[0] && bounds[3] > bounds[1] {
                canvas.hit(close, bounds)?;
            }
        }
        canvas.capture_focus = capture;
    }
    let entrance = canvas.begin_entrance(Surface::Dialog(dialog_id(modal.title)));
    canvas.fill(panel, BORDER)?;
    let x = [left + edge, left + width - edge];
    let header_focus = if native_picker {
        Some(canvas.begin_focus_region(
            focus::PICKER_HEADER,
            [x[0], top + edge, x[1], top + edge + header],
            None,
            false,
        )?)
    } else {
        None
    };
    let header_bottom = title_bar(
        canvas,
        view,
        [x[0], top + edge, x[1]],
        modal,
        !dismiss_overlay || !view.gamepad_input,
    )?;
    if let Some(parent) = header_focus {
        canvas.end_focus_region(parent);
    }
    let content_bottom = header_bottom + content;
    if tray > 0.0 {
        let b = [x[0], content_bottom, x[1], content_bottom + tray];
        canvas.fill(b, NEUTRAL.fill)?;
        canvas.bevel(b, BEVEL_LIGHT, BEVEL_DARK)?;
        let mut y = b[1] + pad;
        for (label, variant, action) in &modal.buttons {
            let button_bounds = [b[0] + pad, y, b[2] - pad, y + canvas.r(BUTTON)];
            button(canvas, view, button_bounds, *variant, label, *action)?;
            y = button_bounds[3] + canvas.r(0.4);
        }
    }
    if content <= 0.0 {
        if let Some(parent) = picker_focus {
            canvas.end_focus_region(parent);
        }
        canvas.end_entrance(entrance, size)?;
        return Ok(None);
    }
    let viewport = [x[0], header_bottom, x[1], content_bottom];
    let list_focus = if native_picker {
        let parent = canvas.begin_focus_region(focus::PICKER_BODY, viewport, None, false)?;
        canvas.disable_focus_delegation();
        Some(parent)
    } else {
        None
    };
    let scroll_focus = if native_picker {
        let parent = canvas.begin_focus_region(
            focus::PICKER_SCROLL,
            viewport,
            Some(SettingsFocusAxis::Vertical),
            false,
        )?;
        canvas.disable_focus_delegation();
        Some(parent)
    } else {
        None
    };
    let focused = modal
        .items
        .iter()
        .position(|entry| entry.action.is_some() && entry.action == view.focused_action)
        .map(|index| {
            let top = header_bottom + edge + index as f32 * item;
            FocusedItem {
                bounds: [x[0], top, x[1], top + item],
                viewport,
                max: (list + text - content).max(0.0),
            }
        });
    let scroll = canvas.begin_scroll(SCROLL, viewport)?;
    let mut y = header_bottom - scroll.offset;
    if list > 0.0 {
        canvas.fill([x[0], y, x[1], y + list], NEUTRAL100)?;
        let mut row = y + edge;
        for entry in &modal.items {
            menu_item(canvas, view, [x[0], row, x[1], row + item], entry)?;
            row += item;
        }
        canvas.fill([x[0], row, x[1], row + edge], MENU_ITEM.border)?;
        y += list;
    }
    if text > 0.0 {
        canvas.fill([x[0], y, x[1], y + text], NEUTRAL80.fill)?;
        canvas.text(
            &modal.body,
            [x[0] + pad, y + pad],
            inner - pad * 2.0,
            CAPTION,
            modal.body_color,
            false,
        )?;
    }
    canvas.end_scroll(scroll, list + text)?;
    if let Some(parent) = scroll_focus {
        canvas.end_focus_region(parent);
    }
    if let Some(parent) = list_focus {
        canvas.end_focus_region(parent);
    }
    if overlap > 0.0 {
        canvas.fill(
            [x[0], content_bottom - overlap, x[1], content_bottom],
            BORDER,
        )?;
    }
    if let Some(parent) = picker_focus {
        canvas.end_focus_region(parent);
    }
    canvas.end_entrance(entrance, size)?;
    Ok(focused)
}

/// The neutral header across `[left, top, right]` with its shadow strip; returns its bottom.
fn title_bar(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    [left, top, right]: [f32; 3],
    modal: &Modal<'_>,
    show_close: bool,
) -> Result<f32, UiPresentationError> {
    let bar = [left, top, right, top + canvas.r(HEADER)];
    canvas.fill(bar, NEUTRAL.fill)?;
    canvas.specular(bar, NEUTRAL.specular[0], NEUTRAL.specular[1])?;
    canvas.fill(
        [left, bar[3], right, bar[3] + canvas.r(EDGE)],
        NEUTRAL.shadow,
    )?;
    let cell = canvas.r(4.0);
    let inset = canvas.r(0.4);
    canvas.text_centred(
        modal.title,
        [left + inset + cell, top, right - inset - cell, bar[3]],
        BODY,
        NEUTRAL.text,
        false,
    )?;
    if show_close && let Some(close) = modal.close {
        let b = [
            right - inset - cell,
            top + inset,
            right - inset,
            top + inset + cell,
        ];
        close_button(canvas, view, b, close)?;
    }
    Ok(bar[3])
}

/// The header's X: a neutral cell that lightens on hover and rings when focused.
pub(super) fn close_button(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, false, Kind::Surface);
    let role = canvas.role(NEUTRAL);
    canvas.fill(b, motion.color([0; 4], role.hovered, role.pressed))?;
    if motion.focus > 0.0 {
        canvas.frame(b, EDGE, opacity(OUTLINE, motion.focus))?;
    }
    let [w, h] = Icon::Cross.texels();
    let texel = canvas.r(EDGE);
    let at = [
        (b[0] + b[2] - w as f32 * texel) * 0.5,
        (b[1] + b[3] - h as f32 * texel) * 0.5,
    ];
    icons::draw(canvas, Icon::Cross, at, NEUTRAL.text)?;
    canvas.hit(action, b)
}

fn local(action: LocalWorldAction) -> Option<MenuAction> {
    Some(MenuAction::LocalWorld(action))
}

pub(super) fn dialog_id(title: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut hash);
    hash.finish()
}

/// The modal a local-world state shows, if any.
pub(super) fn local_world_modal(view: &WorldsView) -> Option<Modal<'_>> {
    let back = local(LocalWorldAction::Back);
    Some(match view.screen {
        Screen::ConfirmDelete => Modal {
            buttons: vec![
                ("Continue editing".into(), Variant::Secondary, back),
                (
                    "Delete world".into(),
                    Variant::Destructive,
                    local(LocalWorldAction::ConfirmDelete),
                ),
            ],
            close: back,
            ..Modal::text(
                "Are you sure?",
                "If you delete this world it will be gone forever.",
            )
        },
        Screen::ConfirmLeaveEdit => Modal {
            buttons: vec![
                (
                    "Save changes".into(),
                    Variant::Primary,
                    local(LocalWorldAction::Save),
                ),
                (
                    "Discard changes".into(),
                    Variant::Secondary,
                    local(LocalWorldAction::Discard),
                ),
            ],
            close: back,
            ..Modal::text(
                "Do you want to save your changes?",
                "You have unsaved changes. Make sure to save or discard your changes.",
            )
        },
        Screen::BackendPrompt => {
            let prompt = view.prompt?;
            Modal {
                buttons: prompt
                    .buttons()
                    .iter()
                    .map(|button| {
                        let variant = match button {
                            PromptButton::UseDragonfly | PromptButton::Retry => Variant::Primary,
                            _ => Variant::Secondary,
                        };
                        (
                            button.label().into(),
                            variant,
                            local(LocalWorldAction::Prompt(*button)),
                        )
                    })
                    .collect(),
                close: back,
                ..Modal::text(prompt.title(), prompt.text())
            }
        }
        Screen::Eula => Modal {
            buttons: vec![
                (
                    "Accept".into(),
                    Variant::Primary,
                    local(LocalWorldAction::AcceptEula),
                ),
                (
                    "View EULA".into(),
                    Variant::Secondary,
                    local(LocalWorldAction::ViewEula),
                ),
                ("Cancel".into(), Variant::Secondary, back),
            ],
            close: back,
            ..Modal::text(
                "Minecraft End User License Agreement",
                "Default worlds run on Mojang's official Bedrock Dedicated Server, downloaded \
                 from minecraft.net the first time you play. Accept the Minecraft EULA and \
                 Privacy Policy to continue.",
            )
        },
        Screen::Error => Modal {
            buttons: vec![("OK".into(), Variant::Primary, back)],
            close: back,
            ..Modal::text(
                "Something went wrong",
                view.error.as_deref().unwrap_or_default(),
            )
        },
        Screen::Create if view.busy => Modal::text("Creating new world...", ""),
        _ => return None,
    })
}
