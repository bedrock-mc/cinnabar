//! Preview, primary action and native category rows share an independent viewport.

use super::super::motion::{Kind, opacity};
use super::super::theme::{BODY, BORDER, EDGE, NEUTRAL80, OUTLINE, TEXT, TEXT_DIMMEST};
use super::*;
use crate::ui_runtime::oreui_assets::{
    SETTINGS_ICON_HIGHLIGHT_IMAGE, WORLD_CATEGORY_ICONS, WORLD_PREVIEW,
};

const CATEGORIES: [&str; 7] = [
    "General",
    "Advanced",
    "Multiplayer",
    "Cheats",
    "Resource packs",
    "Behaviour packs",
    "Experiments",
];

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    route: Screen,
    narrow: bool,
) -> Result<(), UiPresentationError> {
    let edge = canvas.r(EDGE);
    let pad = canvas.r(if narrow { 0.8 } else { 1.6 });
    let inner = [bounds[0] + edge + pad, bounds[2] - edge - pad];
    let preview_height = (inner[1] - inner[0]) * 9.0 / 16.0;
    let row_height = canvas.r(4.8);
    let actions_height = canvas.r(if route == Screen::Create { 10.0 } else { 5.6 });
    let content = pad * 2.0
        + preview_height
        + canvas.r(1.6)
        + actions_height
        + row_height * CATEGORIES.len() as f32;
    let panel = [
        bounds[0],
        bounds[1],
        bounds[2],
        (bounds[1] + content + edge * 2.0).min(bounds[3]),
    ];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, EDGE, BORDER)?;
    let scroll = canvas.begin_scroll(
        "world_settings_sidebar",
        [
            panel[0] + edge,
            panel[1] + edge,
            panel[2] - edge,
            panel[3] - edge,
        ],
    )?;
    let top = bounds[1] + edge + pad - scroll.offset;
    let preview = [inner[0], top, inner[1], top + preview_height];
    canvas.fill(preview, NEUTRAL80.fill)?;
    canvas.cover_sprite(WORLD_PREVIEW, preview)?;
    canvas.frame(preview, EDGE, BORDER)?;
    let mut y = preview[3] + canvas.r(0.8);
    let edit = route == Screen::Edit;
    button(
        canvas,
        view,
        [inner[0], y, inner[1], y + canvas.r(4.8)],
        Variant::Hero,
        if edit { "Play" } else { "Create" },
        (!view.local.busy
            && (edit
                || view.local.create.backend == protocol::world_control::Backend::Dragonfly
                || view.local.bds_can_run))
            .then_some(local(if edit { A::PlayFromEdit } else { A::Create })),
    )?;
    y += canvas.r(5.6);
    if !edit {
        button(
            canvas,
            view,
            [inner[0], y, inner[1], y + canvas.r(4.4)],
            Variant::Neutral,
            "Create on Realms",
            None,
        )?;
        y += canvas.r(4.4);
    }
    y += canvas.r(0.8);
    let selected = if edit || view.local.tab == Tab::General {
        0
    } else {
        1
    };
    if let Some(transitions) = canvas.transitions.as_deref_mut() {
        transitions.begin_world(selected as u8 + if edit { 2 } else { 0 });
    }
    let side = super::super::icons::native_side(canvas);
    let mut focus = None;
    for (index, label) in CATEGORIES.iter().enumerate() {
        let row = [panel[0] + edge, y, panel[2] - edge, y + row_height];
        let action = match (edit, index) {
            (_, 0) => Some(local(A::Tab(Tab::General))),
            (false, 1) => Some(local(A::Tab(Tab::Advanced))),
            _ => None,
        };
        let mut state = canvas.interaction(view, action);
        state.pressed &= state.hovered;
        let motion = canvas.feedback(state, action.is_some(), index == selected, Kind::Surface);
        super::super::sidebar::background(canvas, row, motion)?;
        if motion.focus > 0.0 {
            focus = Some((row, motion.focus));
        }
        let icon = [
            row[0] + pad,
            (row[1] + row[3] - side) * 0.5,
            row[0] + pad + side,
            (row[1] + row[3] + side) * 0.5,
        ];
        let color = if action.is_some() {
            [255; 4]
        } else {
            [255, 255, 255, 128]
        };
        canvas.sprite(WORLD_CATEGORY_ICONS[index], icon, color)?;
        if let Some(frame) = canvas.transitions.as_deref().and_then(|transitions| {
            transitions.world_icon_frame(index as u8 + if edit { 2 } else { 0 })
        }) {
            canvas.sprite_frame(
                SETTINGS_ICON_HIGHLIGHT_IMAGE,
                icon,
                [255; 4],
                frame,
                super::super::transitions::ICON_HIGHLIGHT_FRAMES,
            )?;
        }
        canvas.text_line_vertically_centred(
            label,
            [icon[2] + canvas.r(0.8), row[1], row[2] - pad, row[3]],
            BODY,
            if action.is_some() { TEXT } else { TEXT_DIMMEST },
        )?;
        if let Some(action) = action {
            canvas.hit(action, row)?;
        }
        y = row[3];
    }
    if let Some((bounds, alpha)) = focus {
        canvas.frame(bounds, EDGE, opacity(OUTLINE, alpha))?;
    }
    canvas.end_scroll(scroll, y + pad - top + pad)
}
