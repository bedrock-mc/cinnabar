//! Alpha-only inner outlines of resource-pack sprites.

/// UV samples the sprite; overlay RGBA holds its full source texel bounds, before atlas relocation.
/// Neighbors outside that rectangle have zero coverage, regardless of adjacent atlas artwork.
/// RGB comes only from vertex tint; texture RGB never changes an outline or its inversion.
pub const UI_STYLE_SPRITE_OUTLINE: u16 = 1 << 15;

const _: () = assert!(UI_STYLE_SPRITE_OUTLINE & super::ui_repeat::UI_STYLE_REPEAT == 0);
const _: () = assert!(UI_STYLE_SPRITE_OUTLINE & super::ui_repeat::UI_STYLE_REPEAT_PAGE == 0);
