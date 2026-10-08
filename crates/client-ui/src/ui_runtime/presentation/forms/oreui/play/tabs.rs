use super::super::{
    icons,
    motion::{Feedback, Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{BODY, BORDER, EDGE, NEUTRAL, NEUTRAL80, OUTLINE, TEXT},
};
use super::{MenuAction, MenuScreen, MenuView, UiPresentationError};
use crate::ui_runtime::oreui_assets::{PLAY_TAB_ICONS, SETTINGS_ICON_HIGHLIGHT_IMAGE};

const ART: [&str; 5] = [
    "assets/tabBar_neutral_default-40e26ac9318c12909f42.png",
    "assets/tabBar_neutral_hovered-b73d7029874500714b0f.png",
    "assets/tabBar_neutral_pressed-9d14d8d1343a1fc3836d.png",
    "assets/tabBar_neutral_default_focused-812a9fcda5a4e49d93d5.png",
    "assets/tabBar_neutral_pressed_focused-c05012af3863c527b0a2.png",
];

#[cfg(test)]
mod tests;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    selected: usize,
) -> Result<(), UiPresentationError> {
    if let Some(transitions) = canvas.transitions.as_deref_mut() {
        transitions.begin_play(selected as u8);
    }
    let worlds = format!("Worlds ({})", view.friends.len() + view.local_worlds.len());
    let labels = [worlds.as_str(), "Realms", "Servers"];
    let screens = [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers];
    let role = canvas.role(NEUTRAL);
    let panel = canvas.role(NEUTRAL80);
    let edge = canvas.r(EDGE);
    let width = (b[2] - b[0] + edge * (labels.len() - 1) as f32) / labels.len() as f32;
    let feedback: [Feedback; 3] = std::array::from_fn(|i| {
        let state = canvas.interaction(view, Some(MenuAction::Navigate(screens[i])));
        canvas.feedback(state, true, i == selected, Kind::Tab)
    });
    // Raised focus artwork overlaps the shared border and belongs above its neighbors.
    let order = (0..labels.len())
        .filter(|&i| feedback[i].focus == 0.0)
        .chain((0..labels.len()).filter(|&i| feedback[i].focus > 0.0));
    for i in order {
        let label = labels[i];
        let left = b[0] + (width - edge) * i as f32;
        let cell = [left, b[1], left + width, b[3]];
        let action = MenuAction::Navigate(screens[i]);
        let motion = feedback[i];
        let down = motion.depression();
        let face = [cell[0], cell[1] + canvas.r(0.4) * down, cell[2], cell[3]];
        let strip = canvas.r(0.4) * (1.0 - down);
        if !artwork(canvas, face, motion, down)? {
            canvas.fill(face, BORDER)?;
            let border = canvas.r(0.4);
            let inner = [
                face[0] + border,
                face[1] + border,
                face[2] - border,
                face[3] - border,
            ];
            canvas.fill(inner, NEUTRAL80.fill)?;
            let front = [inner[0], inner[1], inner[2], inner[3] - strip];
            canvas.fill(
                front,
                mix(mix(role.fill, role.hovered, motion.hover), panel.fill, down),
            )?;
            canvas.specular(front, role.specular[0], role.specular[1])?;
            if motion.focus > 0.0 {
                canvas.frame(
                    [
                        face[0] - edge,
                        face[1] - edge,
                        face[2] + edge,
                        face[3] + edge,
                    ],
                    EDGE,
                    opacity(OUTLINE, motion.focus),
                )?;
            }
        }
        let side = icons::native_side(canvas);
        let has_icon = canvas
            .originals
            .is_some_and(|art| art.sprites.contains_key(PLAY_TAB_ICONS[i]));
        let label_width = canvas.measure(label, BODY)?;
        let gap = if has_icon { canvas.r(0.8) } else { 0.0 };
        let icon_width = if has_icon { side } else { 0.0 };
        let x = (face[0] + face[2] - label_width - icon_width - gap) * 0.5;
        let middle = (face[1] + face[3] - strip) * 0.5;
        if has_icon {
            let icon = [x, middle - side * 0.5, x + side, middle + side * 0.5];
            canvas.sprite(PLAY_TAB_ICONS[i], icon, [255; 4])?;
            if let Some(frame) = canvas
                .transitions
                .as_deref()
                .and_then(|t| t.play_icon_frame(i as u8))
            {
                canvas.sprite_frame(
                    SETTINGS_ICON_HIGHLIGHT_IMAGE,
                    icon,
                    [255; 4],
                    frame,
                    super::super::transitions::ICON_HIGHLIGHT_FRAMES,
                )?;
            }
        }
        canvas.text_line(
            label,
            [x + icon_width + gap, middle - canvas.r(BODY.line) * 0.5],
            label_width + 1.0,
            BODY,
            TEXT,
        )?;
        let indicator = canvas.r(4.8).min(width);
        let center = (face[0] + face[2]) * 0.5;
        canvas.fill(
            [
                center - indicator * 0.5,
                face[3],
                center + indicator * 0.5,
                face[3] + edge,
            ],
            opacity(OUTLINE, motion.selected),
        )?;
        if i != selected {
            canvas.hit(action, cell)?;
        }
    }
    Ok(())
}

fn artwork(
    canvas: &mut Canvas<'_>,
    face: Bounds,
    motion: Feedback,
    down: f32,
) -> Result<bool, UiPresentationError> {
    if canvas.appearance == super::super::theme::Appearance::Dark {
        return Ok(false);
    }
    if canvas
        .originals
        .is_none_or(|art| ART.iter().any(|key| !art.sprites.contains_key(*key)))
    {
        return Ok(false);
    }
    let widths = [0.4, 0.4, 0.8 - 0.4 * down, 0.4];
    canvas.nine_slice(ART[0], face, [2, 2, 4, 2], widths, true, [255; 4])?;
    canvas.nine_slice(
        ART[1],
        face,
        [2, 2, 4, 2],
        widths,
        true,
        opacity([255; 4], motion.hover * (1.0 - down)),
    )?;
    canvas.nine_slice(
        ART[2],
        face,
        [2; 4],
        [0.4; 4],
        true,
        opacity([255; 4], down),
    )?;
    if motion.focus > 0.0 {
        let outset = canvas.r(EDGE) * motion.focus;
        let focused = [
            face[0] - outset,
            face[1] - outset,
            face[2] + outset,
            face[3] + outset,
        ];
        canvas.nine_slice(
            ART[3],
            focused,
            [3, 3, 5, 3],
            [0.6, 0.6, 1.0 - 0.4 * down, 0.6],
            true,
            opacity([255; 4], motion.focus * (1.0 - down)),
        )?;
        canvas.nine_slice(
            ART[4],
            focused,
            [3; 4],
            [0.6; 4],
            true,
            opacity([255; 4], motion.focus * down),
        )?;
    }
    Ok(true)
}
