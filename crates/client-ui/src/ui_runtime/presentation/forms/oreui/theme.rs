//! OreUI's vanilla theme as reference facts: the palette, the semantic roles'
//! fills per state, the type scale and the spacer steps. Sizes are in rem
//! (one rem is five GUI pixels).

use crate::ui_runtime::oreui_fonts::OreUiFont;

mod appearance;
pub(super) use appearance::Appearance;

pub(super) type Rgba = [u8; 4];

const fn rgb(value: u32) -> Rgba {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}

const fn white(alpha: u8) -> Rgba {
    [255, 255, 255, alpha]
}

const fn black(alpha: u8) -> Rgba {
    [0, 0, 0, alpha]
}

pub(super) const TEXT: Rgba = rgb(0xffffff);
pub(super) const TEXT_DIMMER: Rgba = rgb(0xd0d1d4);
pub(super) const TEXT_DIMMEST: Rgba = rgb(0xb1b2b5);
pub(super) const TEXT_DARK: Rgba = rgb(0x1e1e1f);
pub(super) const BORDER: Rgba = rgb(0x1e1e1f);
pub(super) const OUTLINE: Rgba = rgb(0xffffff);
pub(super) const OVERLAY_SCREEN: Rgba = black(128);
/// The dimming behind a modal.
pub(super) const OVERLAY_MODAL: Rgba = black(179);
pub(super) const TEXT_SHADOW: Rgba = black(77);

/// One semantic role's fills, text and edge colours.
#[derive(Clone, Copy)]
pub(super) struct Role {
    pub(super) fill: Rgba,
    pub(super) hovered: Rgba,
    pub(super) pressed: Rgba,
    pub(super) text: Rgba,
    /// The elevated strip under a raised control.
    pub(super) shadow: Rgba,
    /// The one-texel outline of a raised control.
    pub(super) border: Rgba,
    /// Top-left and bottom-right inner edges.
    pub(super) specular: [Rgba; 2],
    pub(super) specular_hovered: [Rgba; 2],
}

pub(super) const NEUTRAL: Role = Role {
    fill: rgb(0x48494a),
    hovered: rgb(0x58585a),
    pressed: rgb(0x313233),
    text: TEXT,
    shadow: rgb(0x313233),
    border: BORDER,
    specular: [white(51), white(26)],
    specular_hovered: [white(51), white(26)],
};

pub(super) const NEUTRAL20: Role = Role {
    fill: rgb(0xe6e8eb),
    hovered: rgb(0xf4f6f9),
    pressed: rgb(0xd0d1d4),
    text: TEXT_DARK,
    shadow: rgb(0xb1b2b5),
    border: BORDER,
    specular: [white(255), white(51)],
    specular_hovered: [white(255), white(51)],
};

pub(super) const NEUTRAL80: Role = Role {
    fill: rgb(0x313233),
    hovered: rgb(0x48494a),
    pressed: rgb(0x242425),
    text: TEXT,
    shadow: rgb(0x242425),
    border: BORDER,
    specular: [white(26), black(102)],
    specular_hovered: [white(26), black(102)],
};

pub(super) const PRIMARY_ROLE: Role = Role {
    fill: rgb(0x3c8527),
    hovered: rgb(0x2a641c),
    pressed: rgb(0x1d4d13),
    text: TEXT,
    shadow: rgb(0x1d4d13),
    border: BORDER,
    specular: [white(51), white(26)],
    specular_hovered: [white(102), white(77)],
};

pub(super) const SECONDARY: Role = Role {
    fill: rgb(0xd0d1d4),
    hovered: rgb(0xb1b2b5),
    pressed: rgb(0xb1b2b5),
    text: TEXT_DARK,
    shadow: rgb(0x58585a),
    border: BORDER,
    specular: [white(153), white(102)],
    specular_hovered: [white(204), white(153)],
};

pub(super) const DESTRUCTIVE: Role = Role {
    fill: rgb(0xca3636),
    hovered: rgb(0xc02d2d),
    pressed: rgb(0xad1d1d),
    text: TEXT,
    shadow: rgb(0xad1d1d),
    border: BORDER,
    specular: [white(51), white(26)],
    specular_hovered: [white(128), white(102)],
};

/// A disabled raised control in every role: no speculars.
pub(super) const DISABLED: Role = Role {
    fill: rgb(0xb1b2b5),
    hovered: rgb(0xb1b2b5),
    pressed: rgb(0xb1b2b5),
    text: rgb(0x58585a),
    shadow: rgb(0x8c8d90),
    border: rgb(0x58585a),
    specular: [black(0), black(0)],
    specular_hovered: [black(0), black(0)],
};

/// Which bundle's art a screen draws with: menus screens draw raised controls from
/// nine-slice art, gameplay screens (death, bed) from the role table above.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Bundle {
    #[default]
    Menus,
    Gameplay,
}

/// The menus art where it departs from the role table; primary matches it.
pub(super) const MENU_NEUTRAL: Role = Role {
    specular_hovered: [white(102), white(77)],
    ..NEUTRAL
};
pub(super) const MENU_SECONDARY: Role = Role {
    border: black(255),
    specular: [white(102), white(102)],
    specular_hovered: [white(153), white(153)],
    ..SECONDARY
};
pub(super) const MENU_DESTRUCTIVE: Role = Role {
    specular_hovered: [white(102), white(77)],
    ..DESTRUCTIVE
};
/// Menus art for every pressed or focused face: a `BORDER` outline over these speculars.
pub(super) const MENU_SPECULAR_ACTIVE: [Rgba; 2] = [white(51), white(26)];

/// The modal menu item surface (`neutral60`): fills per state and its borders.
pub(super) const MENU_ITEM: Role = Role {
    fill: rgb(0x58585a),
    hovered: rgb(0x48494a),
    pressed: rgb(0x313233),
    text: TEXT,
    shadow: rgb(0x313233),
    border: DISABLED.shadow,
    specular: [black(0), black(0)],
    specular_hovered: [black(0), black(0)],
};
pub(super) const MENU_ITEM_DISABLED: Role = Role {
    fill: rgb(0x48494a),
    hovered: rgb(0x48494a),
    pressed: rgb(0x48494a),
    text: TEXT_DIMMEST,
    border: rgb(0x8c8d90),
    ..MENU_ITEM
};

/// Text field facts from `baseTextField*`: white text, dimmest placeholder, green caret.
pub(super) const FIELD_PLACEHOLDER: Rgba = rgb(0xb1b2b5);
pub(super) const FIELD_CARET: Rgba = rgb(0x6cc349);

/// Section tints from the vanilla theme (`informativeTint`, `successTint`, `destructiveTint`).
pub(super) const INFORMATIVE_TINT: Rgba = rgb(0x8cb3ff);
pub(super) const SUCCESS_TINT: Rgba = rgb(0xa0e081);
pub(super) const DESTRUCTIVE_TINT: Rgba = rgb(0xff8080);

/// Solid surfaces.
pub(super) const NEUTRAL90: Rgba = rgb(0x242425);
pub(super) const NEUTRAL100: Rgba = rgb(0x1e1e1f);
pub(super) const HEADER_STRIP: Rgba = rgb(0xb1b2b5);
/// The neutral bevel: a light top edge over a dark bottom one.
pub(super) const BEVEL_LIGHT: Rgba = white(26);
pub(super) const BEVEL_DARK: Rgba = black(77);

/// A semantic face with its CSS em size and line height in rem.
#[derive(Clone, Copy)]
pub(super) struct Type {
    pub(super) size: f32,
    pub(super) line: f32,
    pub(super) face: OreUiFont,
}

pub(super) const HEADER3: Type = Type {
    size: 3.2,
    line: 4.0,
    face: OreUiFont::Ten,
};
pub(super) const HEADER5: Type = Type {
    size: 2.0,
    line: 2.4,
    face: OreUiFont::Ten,
};
pub(super) const SECTION_HEADER: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Ten,
};
pub(super) const BODY: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Seven,
};
pub(super) const CAPTION: Type = Type {
    size: 1.4,
    line: 2.0,
    face: OreUiFont::Seven,
};
pub(super) const PRIMARY_BUTTON: Type = Type {
    size: 2.0,
    line: 2.4,
    face: OreUiFont::Ten,
};
pub(super) const SECONDARY_BUTTON: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Seven,
};

pub(super) const LETTER_SPACING: f32 = 0.04;

/// Spacer steps 1..=8 in rem.
pub(super) const SPACE: [f32; 8] = [0.4, 0.8, 1.2, 1.6, 2.0, 2.4, 3.2, 6.4];
/// The standard one-texel edge.
pub(super) const EDGE: f32 = 0.2;
/// Header bar height including its bottom strip.
pub(super) const HEADER_HEIGHT: f32 = 4.8;
