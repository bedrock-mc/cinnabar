/// Colour format used by the ordinary scene and its built-in pipeline variants.
pub const SCENE_COLOR_FORMAT: bevy::render::render_resource::TextureFormat =
    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;

/// Colour format used by the enhanced scene and its built-in pipeline variants.
pub const SCENE_HDR_FORMAT: bevy::render::render_resource::TextureFormat =
    bevy::render::render_resource::TextureFormat::Rgba16Float;
