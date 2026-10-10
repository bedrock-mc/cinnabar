//! Settings controls share vanilla's rail, thumb, bevel and focus geometry.

use super::super::super::super::UiPresentationError;
use super::super::motion::{Kind, mix, opacity};
use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, EDGE, Rgba};
use super::super::widgets::Interaction;
use crate::menu::{MenuAction, MenuView};
use crate::ui_runtime::oreui_assets::{SWITCH_OFF_IMAGE, SWITCH_ON_IMAGE};

const RAIL_OFF: Rgba = theme::DISABLED.shadow;

fn thumb(
    canvas: &mut Canvas<'_>,
    b: Bounds,
    state: Interaction,
    enabled: bool,
    disabled_specular: bool,
) -> Result<(), UiPresentationError> {
    let motion = canvas.feedback(state, enabled, false, Kind::Thumb);
    let role = if enabled {
        theme::SECONDARY
    } else {
        theme::DISABLED
    };
    let role = canvas.role(role);
    canvas.fill(b, role.border)?;
    let edge = canvas.r(EDGE);
    let face = [
        b[0] + edge,
        b[1] + edge,
        b[2] - edge,
        b[3] - edge - canvas.r(0.4),
    ];
    canvas.fill([face[0], face[3], face[2], b[3] - edge], role.shadow)?;
    canvas.fill(face, motion.color(role.fill, role.hovered, role.pressed))?;
    let specular = if !enabled && disabled_specular {
        theme::SECONDARY.specular
    } else {
        std::array::from_fn(|index| {
            mix(
                role.specular[index],
                role.specular_hovered[index],
                motion.hover,
            )
        })
    };
    canvas.specular(face, specular[0], specular[1])?;
    if motion.focus > 0.0 {
        let outset = canvas.r(0.4);
        canvas.frame(
            [b[0] - outset, b[1] - outset, b[2] + outset, b[3] + outset],
            EDGE,
            opacity(theme::OUTLINE, motion.focus),
        )?;
    }
    Ok(())
}

pub(super) fn toggle_control(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    at: [f32; 2],
    on: bool,
    action: MenuAction,
    enabled: bool,
) -> Result<(), UiPresentationError> {
    let knob = canvas.transitions.as_deref_mut().map_or(
        super::super::transitions::switch_target(on),
        |transitions| {
            transitions.switch(action, on, view.settings_control_activation, canvas.seconds)
        },
    );
    let state = canvas.switch_interaction(view, Some(action));
    toggle_at(canvas, at, knob, state, enabled)
}

fn toggle_at(
    canvas: &mut Canvas<'_>,
    at: [f32; 2],
    knob: f32,
    state: Interaction,
    enabled: bool,
) -> Result<(), UiPresentationError> {
    let [x, y] = at;
    let rail = [
        x + canvas.r(0.2),
        y + canvas.r(0.2),
        x + canvas.r(6.2),
        y + canvas.r(3.0),
    ];
    canvas.fill(rail, if enabled { theme::BORDER } else { RAIL_OFF })?;
    let edge = canvas.r(EDGE);
    let face = [
        rail[0] + edge,
        rail[1] + edge,
        rail[2] - edge,
        rail[3] - edge,
    ];
    let middle = (face[0] + face[2]) * 0.5;
    canvas.fill(
        [face[0], face[1], middle, face[3]],
        if enabled {
            theme::PRIMARY_ROLE.fill
        } else {
            theme::DISABLED.fill
        },
    )?;
    canvas.fill(
        [middle, face[1], face[2], face[3]],
        if enabled {
            RAIL_OFF
        } else {
            theme::DISABLED.fill
        },
    )?;
    if enabled {
        canvas.specular(face, [255, 255, 255, 51], [255, 255, 255, 26])?;
    }
    for symbol_on in [true, false] {
        let symbol = [
            x + canvas.r(if symbol_on { 1.6 } else { 4.0 }),
            y + canvas.r(1.0),
        ];
        let glyph = [
            symbol[0],
            symbol[1],
            symbol[0] + canvas.r(if symbol_on { EDGE } else { 1.2 }),
            symbol[1] + canvas.r(1.2),
        ];
        let key = if symbol_on {
            SWITCH_ON_IMAGE
        } else {
            SWITCH_OFF_IMAGE
        };
        let symbol_color = if enabled {
            if symbol_on {
                theme::TEXT
            } else {
                [36, 36, 37, 255]
            }
        } else if symbol_on {
            [102, 102, 102, 255]
        } else {
            [109, 109, 109, 255]
        };
        let symbol_color = canvas.appearance.ink(symbol_color);
        if !canvas.masked_sprite(key, glyph, symbol_color)? {
            if symbol_on {
                canvas.fill(glyph, symbol_color)?;
            } else {
                canvas.frame(glyph, EDGE, symbol_color)?;
            }
        }
    }
    let knob_x = x + canvas.r(knob);
    thumb(
        canvas,
        [knob_x, y, knob_x + canvas.r(3.2), y + canvas.r(3.2)],
        state,
        enabled,
        false,
    )
}

pub(super) fn slider(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    actions: &[MenuAction],
    selected: usize,
    show_steps: bool,
) -> Result<(), UiPresentationError> {
    if actions.is_empty() {
        return Ok(());
    }
    let half = canvas.r(1.6).min((b[2] - b[0]) * 0.5);
    let travel = (b[2] - b[0] - half * 2.0).max(0.0);
    let selected_fraction = if actions.len() > 1 {
        selected.min(actions.len() - 1) as f32 / (actions.len() - 1) as f32
    } else {
        0.0
    };
    let index = match actions[0] {
        MenuAction::SettingsOption(index, _) => Some(index),
        _ => None,
    };
    let pointer = view
        .settings_slider_pointer
        .filter(|pointer| Some(pointer.option) == index && pointer.fraction.is_finite());
    let target = pointer.map_or(selected_fraction, |pointer| {
        pointer.fraction.clamp(0.0, 1.0)
    });
    let fraction = match (canvas.transitions.as_deref_mut(), index) {
        (Some(transitions), Some(index)) => transitions.slider(
            index,
            target,
            pointer.is_some_and(|pointer| pointer.mouse_input),
            canvas.seconds,
        ),
        _ => target,
    };
    let centre = b[0] + half + fraction * travel;
    let middle = (b[1] + b[3]) * 0.5;
    let rail = [b[0], middle - canvas.r(0.6), b[2], middle + canvas.r(0.6)];
    let enabled = actions.len() > 1;
    canvas.fill(rail, if enabled { theme::BORDER } else { RAIL_OFF })?;
    let edge = canvas.r(EDGE);
    let inner = [
        rail[0] + edge,
        rail[1] + edge,
        rail[2] - edge,
        rail[3] - edge,
    ];
    canvas.fill(
        inner,
        if enabled {
            RAIL_OFF
        } else {
            theme::DISABLED.fill
        },
    )?;
    canvas.specular(inner, [255, 255, 255, 51], [255, 255, 255, 26])?;
    let end = (rail[0] + (rail[2] - rail[0]) * fraction).min(inner[2]);
    if end > inner[0] {
        canvas.fill(
            [inner[0], inner[1], end, inner[3]],
            if enabled {
                theme::PRIMARY_ROLE.fill
            } else {
                theme::DISABLED.fill
            },
        )?;
        canvas.fill(
            [inner[0], inner[1], end, inner[1] + edge],
            [255, 255, 255, 51],
        )?;
        canvas.fill(
            [inner[0], inner[3] - edge, end, inner[3]],
            [255, 255, 255, 26],
        )?;
        canvas.fill(
            [
                inner[0],
                inner[1] + edge,
                (inner[0] + edge).min(end),
                inner[3],
            ],
            [255, 255, 255, 51],
        )?;
        if end == inner[2] {
            canvas.fill(
                [end - edge, inner[1], end, inner[3] - edge],
                [255, 255, 255, 26],
            )?;
        }
    }
    let same =
        |candidate: Option<MenuAction>| actions.iter().any(|action| Some(*action) == candidate);
    if show_steps && actions.len() > 2 {
        for index in 1..actions.len() - 1 {
            let x = b[0] + half + travel * index as f32 / (actions.len() - 1) as f32;
            canvas.fill(
                [x, rail[1], x + edge, rail[3]],
                if enabled { theme::BORDER } else { RAIL_OFF },
            )?;
        }
    }
    let thumb_bounds = [centre - half, b[1], centre + half, b[3]];
    thumb(
        canvas,
        thumb_bounds,
        Interaction {
            action: actions.get(selected).copied(),
            hovered: (view.settings_slider_pointer.is_none()
                && index.is_some_and(|index| view.settings_slider_hovered == Some(index)))
                || pointer.is_some()
                || index.is_some_and(|index| view.settings_slider_selected == Some(index)),
            pressed: false,
            focused: view.navigation_focus_visible && same(view.focused_action),
        },
        enabled,
        true,
    )?;
    if let Some(index) = index.filter(|_| enabled) {
        let track = [b[0] + half, b[1], b[2] - half, b[3]];
        canvas.slider_track(index, track, thumb_bounds)?;
        canvas.focus_target(actions[selected.min(actions.len() - 1)], track)?;
    }
    if actions.len() <= 1 {
        return Ok(());
    }
    let capture_focus = canvas.capture_focus;
    canvas.capture_focus = false;
    let result = actions.iter().enumerate().try_for_each(|(index, action)| {
        let steps = actions.len().saturating_sub(1) as f32;
        let left = if index == 0 {
            b[0]
        } else {
            b[0] + half + travel * (index as f32 - 0.5) / steps
        };
        let right = if index + 1 == actions.len() {
            b[2]
        } else {
            b[0] + half + travel * (index as f32 + 0.5) / steps
        };
        canvas.hit(*action, [left, b[1], right, b[3]])
    });
    canvas.capture_focus = capture_focus;
    result
}

#[cfg(test)]
mod tests;
