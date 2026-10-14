//! Solid OreUI drawing without any installed artwork: the dimmed backdrop, the raised panel and
//! the menus' raised buttons, shared by every bootstrap window.

use ui::oreui_theme::{self as theme, Appearance};

use crate::canvas::{Canvas, Image, Rect, Text};

/// Dims the panorama under the whole frame and draws the logo, as the menus' loading screen does.
pub fn backdrop(canvas: &mut Canvas, appearance: Appearance, logo: Option<(&Image, Rect)>) {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        w: canvas.width as f32,
        h: canvas.height as f32,
    };
    canvas.fill(full, appearance.backdrop(theme::OVERLAY_SCREEN));
    if let Some((logo, rect)) = logo {
        canvas.image(logo, rect);
    }
}

/// The raised panel: an offset drop shadow, the panel surface and its one-edge border.
pub fn panel_surface(canvas: &mut Canvas, appearance: Appearance, panel: Rect, rem: f32) {
    canvas.fill(
        Rect {
            x: panel.x + theme::BUTTON_DEPTH * rem,
            y: panel.y + theme::BUTTON_DEPTH * rem,
            ..panel
        },
        theme::TEXT_SHADOW,
    );
    canvas.fill(panel, appearance.surface(theme::NEUTRAL80.fill));
    canvas.frame(panel, theme::EDGE * rem, appearance.surface(theme::BORDER));
}

/// Centres open-font text within a shared OreUI semantic type size.
pub fn centred(
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

/// One raised button's interaction state.
#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonState {
    pub hovered: bool,
    pub focused: bool,
    pub pressed: bool,
}

/// Reproduces the menus' solid raised fallback, including active speculars and focus outline,
/// and returns the face the label sits on.
pub fn raised_button(
    canvas: &mut Canvas,
    appearance: Appearance,
    role: theme::Role,
    state: ButtonState,
    rect: Rect,
    rem: f32,
) -> Rect {
    let ButtonState {
        hovered,
        focused,
        pressed,
    } = state;
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
            appearance.surface(theme::BORDER)
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
    face
}

/// Centres a button label on its face in the menus' secondary button type.
pub fn button_label(
    canvas: &mut Canvas,
    text: &mut Text,
    face: Rect,
    rem: f32,
    ink: theme::Rgba,
    label: &str,
) {
    let px = theme::SECONDARY_BUTTON.size * rem;
    let label_y = face.y + (face.h - text.line_height(px)) * 0.5;
    centred(canvas, text, face, label_y, px, ink, label);
}
