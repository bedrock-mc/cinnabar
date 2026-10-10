//! Optional dark roles preserve geometry, accent colors and full-color artwork.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Default,
    Dark,
}

const DARK_HEADER: Role = Role {
    fill: rgb(0x22262c),
    hovered: rgb(0x343b44),
    pressed: rgb(0x1b2026),
    text: TEXT,
    shadow: rgb(0x181c22),
    border: rgb(0x111419),
    specular: [white(26), black(77)],
    specular_hovered: [white(38), black(77)],
};
const DARK_SECONDARY: Role = Role {
    fill: rgb(0x3a424c),
    hovered: rgb(0x4a5563),
    pressed: rgb(0x2d343e),
    text: TEXT,
    shadow: rgb(0x191e25),
    border: rgb(0x111419),
    specular: [white(51), white(26)],
    specular_hovered: [white(64), white(38)],
};
const DARK_NEUTRAL: Role = Role {
    fill: rgb(0x2b3139),
    hovered: rgb(0x3a444f),
    pressed: rgb(0x20262e),
    text: TEXT,
    shadow: rgb(0x171c22),
    border: rgb(0x111419),
    specular: [white(26), black(77)],
    specular_hovered: [white(38), black(77)],
};
const DARK_PANEL: Role = Role {
    fill: rgb(0x20262d),
    hovered: rgb(0x343e49),
    pressed: rgb(0x171d24),
    text: TEXT,
    shadow: rgb(0x161b22),
    border: rgb(0x111419),
    specular: [white(26), black(102)],
    specular_hovered: [white(38), black(102)],
};
const DARK_DISABLED: Role = Role {
    fill: rgb(0x292f37),
    hovered: rgb(0x292f37),
    pressed: rgb(0x292f37),
    text: rgb(0x969faa),
    shadow: rgb(0x1c222a),
    border: rgb(0x39424d),
    specular: [black(0); 2],
    specular_hovered: [black(0); 2],
};
const DARK_ITEM: Role = Role {
    fill: rgb(0x303943),
    hovered: rgb(0x3e4b58),
    pressed: rgb(0x242d37),
    text: TEXT,
    shadow: rgb(0x1c222a),
    border: rgb(0x434e5c),
    specular: [black(0); 2],
    specular_hovered: [black(0); 2],
};

const DARK_BACKDROP_VISIBILITY_PERCENT: u16 = 40;

impl Appearance {
    /// Selects the saved menu appearance without loading a resource pack.
    pub fn from_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Default }
    }

    /// Maps a semantic control role to the selected appearance.
    pub fn role(self, role: Role) -> Role {
        if self == Self::Default {
            return role;
        }
        match role.fill {
            color if color == NEUTRAL20.fill => DARK_HEADER,
            color if color == SECONDARY.fill => DARK_SECONDARY,
            color if color == DISABLED.fill => DARK_DISABLED,
            color if color == NEUTRAL80.fill => DARK_PANEL,
            color if color == NEUTRAL.fill => DARK_NEUTRAL,
            color if color == MENU_ITEM.fill => DARK_ITEM,
            _ => role,
        }
    }

    /// Dims scenery while preserving the overlay tint.
    pub fn backdrop(self, mut color: Rgba) -> Rgba {
        if self == Self::Dark && color[3] > 0 {
            let visible = u16::from(255 - color[3]) * DARK_BACKDROP_VISIBILITY_PERCENT / 100;
            color[3] = 255 - visible as u8;
        }
        color
    }

    /// Maps solid surfaces while retaining their original opacity.
    pub fn surface(self, color: Rgba) -> Rgba {
        if self == Self::Default {
            return color;
        }
        let alpha = color[3];
        let rgb = [color[0], color[1], color[2], 255];
        let target = match rgb {
            c if c == NEUTRAL20.fill => DARK_HEADER.fill,
            c if c == NEUTRAL20.hovered => DARK_HEADER.hovered,
            c if c == SECONDARY.fill => DARK_SECONDARY.fill,
            c if c == DISABLED.fill => DARK_DISABLED.fill,
            c if c == DISABLED.shadow => DARK_DISABLED.shadow,
            c if c == NEUTRAL80.fill => DARK_PANEL.fill,
            c if c == NEUTRAL.fill => DARK_NEUTRAL.fill,
            c if c == NEUTRAL.hovered => DARK_NEUTRAL.hovered,
            c if c == NEUTRAL90 => DARK_PANEL.shadow,
            c if c == BORDER => DARK_PANEL.border,
            _ => rgb,
        };
        [target[0], target[1], target[2], alpha]
    }

    /// Keeps foreground text readable on the selected appearance.
    pub fn ink(self, color: Rgba) -> Rgba {
        if self == Self::Default {
            return color;
        }
        let target = match [color[0], color[1], color[2], 255] {
            c if c == TEXT_DARK || c == NEUTRAL90 => TEXT,
            c if c == DISABLED.text => DARK_DISABLED.text,
            _ => color,
        };
        [target[0], target[1], target[2], color[3]]
    }
}

#[cfg(test)]
mod tests;
