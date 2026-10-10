//! Shared OreUI colours, control styles, type sizes and spacing. Sizes are in rem.

mod font;
pub use font::OreUiFont;

mod appearance;
mod metrics;
pub use appearance::Appearance;
pub use metrics::{
    BUTTON_DEPTH, BUTTON_HEIGHT, CANCEL_WIDTH, GUI_PIXELS_PER_REM, LOADING_FOOTER_AREA,
    LOADING_PAD, LOADING_PROGRESS_AREA, LOADING_WIDTH, PROGRESS_HEIGHT,
};

pub type Rgba = [u8; 4];

const fn rgb(value: u32) -> Rgba {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}

const fn white(alpha: u8) -> Rgba {
    [255, 255, 255, alpha]
}

const fn black(alpha: u8) -> Rgba {
    [0, 0, 0, alpha]
}

pub const TEXT: Rgba = rgb(0xffffff);
pub const TEXT_DIMMER: Rgba = rgb(0xd0d1d4);
pub const TEXT_DIMMEST: Rgba = rgb(0xb1b2b5);
pub const TEXT_DARK: Rgba = rgb(0x1e1e1f);
pub const BORDER: Rgba = rgb(0x1e1e1f);
pub const OUTLINE: Rgba = rgb(0xffffff);
pub const OVERLAY_SCREEN: Rgba = black(128);
/// The dimming behind a modal.
pub const OVERLAY_MODAL: Rgba = black(179);
pub const TEXT_SHADOW: Rgba = black(77);

/// One semantic role's fills, text and edge colours.
#[derive(Clone, Copy)]
pub struct Role {
    pub fill: Rgba,
    pub hovered: Rgba,
    pub pressed: Rgba,
    pub text: Rgba,
    /// The elevated strip under a raised control.
    pub shadow: Rgba,
    /// The one-texel outline of a raised control.
    pub border: Rgba,
    /// Top-left and bottom-right inner edges.
    pub specular: [Rgba; 2],
    pub specular_hovered: [Rgba; 2],
}

pub const NEUTRAL: Role = Role {
    fill: rgb(0x48494a),
    hovered: rgb(0x58585a),
    pressed: rgb(0x313233),
    text: TEXT,
    shadow: rgb(0x313233),
    border: BORDER,
    specular: [white(51), white(26)],
    specular_hovered: [white(51), white(26)],
};

pub const NEUTRAL20: Role = Role {
    fill: rgb(0xe6e8eb),
    hovered: rgb(0xf4f6f9),
    pressed: rgb(0xd0d1d4),
    text: TEXT_DARK,
    shadow: rgb(0xb1b2b5),
    border: BORDER,
    specular: [white(255), white(51)],
    specular_hovered: [white(255), white(51)],
};

pub const NEUTRAL80: Role = Role {
    fill: rgb(0x313233),
    hovered: rgb(0x48494a),
    pressed: rgb(0x242425),
    text: TEXT,
    shadow: rgb(0x242425),
    border: BORDER,
    specular: [white(26), black(102)],
    specular_hovered: [white(26), black(102)],
};

pub const PRIMARY_ROLE: Role = Role {
    fill: rgb(0x3c8527),
    hovered: rgb(0x2a641c),
    pressed: rgb(0x1d4d13),
    text: TEXT,
    shadow: rgb(0x1d4d13),
    border: BORDER,
    specular: [white(51), white(26)],
    specular_hovered: [white(102), white(77)],
};

pub const SECONDARY: Role = Role {
    fill: rgb(0xd0d1d4),
    hovered: rgb(0xb1b2b5),
    pressed: rgb(0xb1b2b5),
    text: TEXT_DARK,
    shadow: rgb(0x58585a),
    border: BORDER,
    specular: [white(153), white(102)],
    specular_hovered: [white(204), white(153)],
};

pub const DESTRUCTIVE: Role = Role {
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
pub const DISABLED: Role = Role {
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
pub enum Bundle {
    #[default]
    Menus,
    Gameplay,
}

/// The menus art where it departs from the role table; primary matches it.
pub const MENU_NEUTRAL: Role = Role {
    specular_hovered: [white(102), white(77)],
    ..NEUTRAL
};
pub const MENU_SECONDARY: Role = Role {
    border: black(255),
    specular: [white(102), white(102)],
    specular_hovered: [white(153), white(153)],
    ..SECONDARY
};
pub const MENU_DESTRUCTIVE: Role = Role {
    specular_hovered: [white(102), white(77)],
    ..DESTRUCTIVE
};
/// Menus art for every pressed or focused face: a `BORDER` outline over these speculars.
pub const MENU_SPECULAR_ACTIVE: [Rgba; 2] = [white(51), white(26)];

/// The modal menu item surface (`neutral60`): fills per state and its borders.
pub const MENU_ITEM: Role = Role {
    fill: rgb(0x58585a),
    hovered: rgb(0x48494a),
    pressed: rgb(0x313233),
    text: TEXT,
    shadow: rgb(0x313233),
    border: DISABLED.shadow,
    specular: [black(0), black(0)],
    specular_hovered: [black(0), black(0)],
};
pub const MENU_ITEM_DISABLED: Role = Role {
    fill: rgb(0x48494a),
    hovered: rgb(0x48494a),
    pressed: rgb(0x48494a),
    text: TEXT_DIMMEST,
    border: rgb(0x8c8d90),
    ..MENU_ITEM
};

/// Text field facts from `baseTextField*`: white text, dimmest placeholder, green caret.
pub const FIELD_PLACEHOLDER: Rgba = rgb(0xb1b2b5);
pub const FIELD_CARET: Rgba = rgb(0x6cc349);

/// Section tints from the vanilla theme (`informativeTint`, `successTint`, `destructiveTint`).
pub const INFORMATIVE_TINT: Rgba = rgb(0x8cb3ff);
pub const SUCCESS_TINT: Rgba = rgb(0xa0e081);
pub const DESTRUCTIVE_TINT: Rgba = rgb(0xff8080);

/// Solid surfaces.
pub const NEUTRAL90: Rgba = rgb(0x242425);
pub const NEUTRAL100: Rgba = rgb(0x1e1e1f);
pub const HEADER_STRIP: Rgba = rgb(0xb1b2b5);
/// The neutral bevel: a light top edge over a dark bottom one.
pub const BEVEL_LIGHT: Rgba = white(26);
pub const BEVEL_DARK: Rgba = black(77);

/// A semantic face with its CSS em size and line height in rem.
#[derive(Clone, Copy)]
pub struct Type {
    pub size: f32,
    pub line: f32,
    pub face: OreUiFont,
}

pub const HEADER3: Type = Type {
    size: 3.2,
    line: 4.0,
    face: OreUiFont::Ten,
};
pub const HEADER5: Type = Type {
    size: 2.0,
    line: 2.4,
    face: OreUiFont::Ten,
};
pub const SECTION_HEADER: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Ten,
};
pub const BODY: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Seven,
};
pub const CAPTION: Type = Type {
    size: 1.4,
    line: 2.0,
    face: OreUiFont::Seven,
};
pub const PRIMARY_BUTTON: Type = Type {
    size: 2.0,
    line: 2.4,
    face: OreUiFont::Ten,
};
pub const SECONDARY_BUTTON: Type = Type {
    size: 1.6,
    line: 2.0,
    face: OreUiFont::Seven,
};

pub const LETTER_SPACING: f32 = 0.04;

/// Spacer steps 1..=8 in rem.
pub const SPACE: [f32; 8] = [0.4, 0.8, 1.2, 1.6, 2.0, 2.4, 3.2, 6.4];
/// The standard one-texel edge.
pub const EDGE: f32 = 0.2;
/// Header bar height including its bottom strip.
pub const HEADER_HEIGHT: f32 = 4.8;
