//! Solid OreUI control drawing without any installed artwork.

use super::*;

/// Centres open-font text within a shared OreUI semantic type size.
pub(super) fn centred(
    canvas: &mut Canvas,
    text: &mut Text,
    rect: Rect,
    y: f32,
    px: f32,
    ink: theme::Rgba,
    label: &str,
) {
    let x = rect.x + (rect.w - text.width(label, px)) * 0.5;
    text.draw(canvas, x, y, px, ink, label);
}

/// Reproduces the menus' solid raised fallback, including active speculars and focus outline.
pub(super) fn button(
    canvas: &mut Canvas,
    text: &mut Text,
    screen: &Screen,
    style: &Style<'_>,
    action: Action,
    rect: Rect,
    rem: f32,
) {
    let role = style.appearance.role(if screen.primary() == Some(action) {
        theme::PRIMARY_ROLE
    } else {
        theme::MENU_NEUTRAL
    });
    let pressed = style.pressed == Some(action);
    let focused = style.focused == Some(action);
    let hovered = style.hovered == Some(action);
    let active = pressed || focused && !hovered;
    let edge = theme::EDGE * rem;
    let depth = theme::BUTTON_DEPTH * rem;
    let outer = if pressed {
        Rect {
            y: rect.y + depth,
            h: rect.h - depth,
            ..rect
        }
    } else {
        rect
    };
    canvas.fill(
        outer,
        if active {
            style.appearance.surface(theme::BORDER)
        } else {
            role.border
        },
    );
    let mut face = outer.inset(edge);
    if !pressed {
        canvas.fill(
            Rect {
                y: face.y + face.h - depth,
                h: depth,
                ..face
            },
            role.shadow,
        );
        face.h -= depth;
    }
    canvas.fill(
        face,
        if pressed {
            role.pressed
        } else if hovered {
            role.hovered
        } else {
            role.fill
        },
    );
    let specular = if active {
        theme::MENU_SPECULAR_ACTIVE
    } else if hovered {
        role.specular_hovered
    } else {
        role.specular
    };
    canvas.fill(
        Rect {
            y: face.y + face.h - edge,
            h: edge,
            ..face
        },
        specular[1],
    );
    canvas.fill(
        Rect {
            x: face.x + face.w - edge,
            w: edge,
            h: face.h - edge,
            ..face
        },
        specular[1],
    );
    canvas.fill(Rect { h: edge, ..face }, specular[0]);
    canvas.fill(
        Rect {
            y: face.y + edge,
            w: edge,
            h: face.h - edge,
            ..face
        },
        specular[0],
    );
    if focused {
        canvas.frame(outer.inset(-edge), edge, theme::OUTLINE);
    }
    let px = theme::SECONDARY_BUTTON.size * rem;
    let label_y = face.y + (face.h - text.line_height(px)) * 0.5;
    centred(
        canvas,
        text,
        face,
        label_y,
        px,
        role.text,
        screen.button_label(action),
    );
}
