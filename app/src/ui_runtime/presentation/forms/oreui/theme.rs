//! OreUI's vanilla theme as reference facts: the palette, the semantic roles'
//! fills per state, the type scale and the spacer steps. Sizes are in rem
//! (one rem is five GUI pixels).

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
    pub(super) specular_top: Rgba,
    pub(super) specular_bottom: Rgba,
    pub(super) specular_top_hovered: Rgba,
    pub(super) specular_bottom_hovered: Rgba,
}

pub(super) const NEUTRAL: Role = Role {
    fill: rgb(0x48494a),
    hovered: rgb(0x58585a),
    pressed: rgb(0x313233),
    text: TEXT,
    shadow: rgb(0x313233),
    specular_top: white(51),
    specular_bottom: white(26),
    specular_top_hovered: white(51),
    specular_bottom_hovered: white(26),
};

pub(super) const NEUTRAL20: Role = Role {
    fill: rgb(0xe6e8eb),
    hovered: rgb(0xf4f6f9),
    pressed: rgb(0xd0d1d4),
    text: TEXT_DARK,
    shadow: rgb(0xb1b2b5),
    specular_top: white(255),
    specular_bottom: white(51),
    specular_top_hovered: white(255),
    specular_bottom_hovered: white(51),
};

pub(super) const NEUTRAL80: Role = Role {
    fill: rgb(0x313233),
    hovered: rgb(0x48494a),
    pressed: rgb(0x242425),
    text: TEXT,
    shadow: rgb(0x242425),
    specular_top: white(26),
    specular_bottom: black(102),
    specular_top_hovered: white(26),
    specular_bottom_hovered: black(102),
};

pub(super) const PRIMARY_ROLE: Role = Role {
    fill: rgb(0x3c8527),
    hovered: rgb(0x2a641c),
    pressed: rgb(0x1d4d13),
    text: TEXT,
    shadow: rgb(0x1d4d13),
    specular_top: white(51),
    specular_bottom: white(26),
    specular_top_hovered: white(102),
    specular_bottom_hovered: white(77),
};

pub(super) const SECONDARY: Role = Role {
    fill: rgb(0xd0d1d4),
    hovered: rgb(0xb1b2b5),
    pressed: rgb(0xb1b2b5),
    text: TEXT_DARK,
    shadow: rgb(0x58585a),
    specular_top: white(153),
    specular_bottom: white(102),
    specular_top_hovered: white(204),
    specular_bottom_hovered: white(153),
};

/// `colorsDestructive` (#ca3636); the state fills and edges are unrecovered approximations.
pub(super) const DESTRUCTIVE: Role = Role {
    fill: rgb(0xca3636),
    hovered: rgb(0xb02e2e),
    pressed: rgb(0x8f2424),
    text: TEXT,
    shadow: rgb(0x8f2424),
    specular_top: white(51),
    specular_bottom: white(26),
    specular_top_hovered: white(102),
    specular_bottom_hovered: white(77),
};

/// Text field facts from `baseTextField*`: white text, dimmest placeholder, green caret.
pub(super) const FIELD_PLACEHOLDER: Rgba = rgb(0xb1b2b5);
pub(super) const FIELD_CARET: Rgba = rgb(0x6cc349);

/// Section tints from the vanilla theme (`informativeTint` and `successTint`).
pub(super) const INFORMATIVE_TINT: Rgba = rgb(0x8cb3ff);
pub(super) const SUCCESS_TINT: Rgba = rgb(0xa0e081);

/// Solid surfaces.
pub(super) const NEUTRAL90: Rgba = rgb(0x242425);
pub(super) const NEUTRAL100: Rgba = rgb(0x1e1e1f);
pub(super) const HEADER_STRIP: Rgba = rgb(0xb1b2b5);
pub(super) const BEVEL_LIGHT: Rgba = white(26);
pub(super) const BEVEL_DARK: Rgba = black(77);

/// A type style: size and line height in rem, and whether it is a heading face.
#[derive(Clone, Copy)]
pub(super) struct Type {
    pub(super) size: f32,
    pub(super) line: f32,
}

pub(super) const HEADER3: Type = Type {
    size: 3.2,
    line: 4.0,
};
pub(super) const HEADER5: Type = Type {
    size: 2.0,
    line: 2.4,
};
pub(super) const SECTION_HEADER: Type = Type {
    size: 1.6,
    line: 2.0,
};
pub(super) const BODY: Type = Type {
    size: 1.6,
    line: 2.0,
};
pub(super) const CAPTION: Type = Type {
    size: 1.4,
    line: 2.0,
};
pub(super) const PRIMARY_BUTTON: Type = Type {
    size: 2.0,
    line: 2.4,
};
pub(super) const SECONDARY_BUTTON: Type = Type {
    size: 1.6,
    line: 2.0,
};

/// Spacer steps 1..=8 in rem.
pub(super) const SPACE: [f32; 8] = [0.4, 0.8, 1.2, 1.6, 2.0, 2.4, 3.2, 6.4];
/// The standard one-texel edge.
pub(super) const EDGE: f32 = 0.2;
/// Header bar height including its bottom strip.
pub(super) const HEADER_HEIGHT: f32 = 4.8;
