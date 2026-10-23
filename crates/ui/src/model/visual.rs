//! Paint primitives owned by retained UI nodes.

use super::UiMesh;
use crate::TextLayout;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UiVisual {
    #[default]
    None,
    /// Direct geometry at this control's authored draw position; depth-using materials get private depth.
    Mesh(Arc<UiMesh>),
    /// Shared geometry with a uniform replacement color, without copying the retained vertices.
    TintedMesh {
        mesh: Arc<UiMesh>,
        color: [u8; 4],
    },
    Solid {
        texture_page: u16,
        color: [u8; 4],
    },
    Sprite {
        texture_page: u16,
        uv: [u16; 4],
        color: [u8; 4],
    },
    /// A dyed sprite whose sampled alpha weights color rather than opacity.
    ColorMaskSprite {
        texture_page: u16,
        uv: [u16; 4],
        color: [u8; 4],
        tint: [u8; 3],
        glint: bool,
    },
    /// A nearest-neighbour sprite rotated around the centre of its node.
    ///
    /// This is used by first-person viewmodel surfaces whose 2-D fallback
    /// still needs the small inward tilt of the native hand/item path.
    RotatedSprite {
        texture_page: u16,
        uv: [u16; 4],
        color: [u8; 4],
        angle_radians: f32,
    },
    /// A sprite with the animated enchantment glint over its opaque texels.
    GlintSprite {
        texture_page: u16,
        uv: [u16; 4],
        color: [u8; 4],
    },
    /// A sprite retaining fractional texel edges, optionally carrying
    /// [`super::UI_STYLE_GRAYSCALE`]/[`super::UI_STYLE_BILINEAR`].
    StyledSprite {
        texture_page: u16,
        uv: [f32; 4],
        color: [u8; 4],
        style: u8,
    },
    /// A source region addressed by tile coordinates, with no geometry per repeat.
    RepeatedSprite {
        texture_page: u16,
        tile_uv: [f32; 4],
        source: [f32; 4],
        color: [u8; 4],
        style: u16,
    },
    /// A solid rect blending `colors[0]` into `colors[1]` top to bottom (or left
    /// to right when `horizontal`).
    Gradient {
        texture_page: u16,
        colors: [[u8; 4]; 2],
        horizontal: bool,
    },
    /// A sprite drawn with the invert blend instead of alpha compositing.
    InvertedSprite {
        texture_page: u16,
        uv: [u16; 4],
    },
    Text {
        layout: Arc<TextLayout>,
        color: [u8; 4],
        /// Draws the whole run once offset down-right in a darkened copy of
        /// each glyph's colour before drawing the run itself, the way Mojang's
        /// client shadows HUD and chat text. This is a property of the draw
        /// rather than of a span: a `§` colour code changes the hue, never
        /// whether the run is shadowed.
        shadow: TextShadow,
    },
    /// A text run rotated around the centre of its node (the title splash).
    RotatedText {
        layout: Arc<TextLayout>,
        color: [u8; 4],
        shadow: TextShadow,
        angle_radians: f32,
    },
}

/// A Java-style 1-design-pixel drop shadow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextShadow {
    #[default]
    None,
    /// Offset in unscaled 1/64 pixels, applied on both axes.
    Offset64(u32),
}
