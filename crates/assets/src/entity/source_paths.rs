use super::{AssetError, EntityAssetKind, MAX_ENTITY_ASSET_PATH_BYTES, invalid};

pub const ACTOR_GLINT_TEXTURE_IDENTIFIER: &str = "textures/misc/enchanted_actor_glint";
/// Shared item raster used by held and dropped items and Bedrock 1.12 worn-armour glint.
pub const ITEM_GLINT_TEXTURE_IDENTIFIER: &str = "textures/misc/enchanted_item_glint";
/// Every shared glint raster the entity carrier keeps beside equipment art.
pub const GLINT_TEXTURE_IDENTIFIERS: [&str; 2] = [
    ACTOR_GLINT_TEXTURE_IDENTIFIER,
    ITEM_GLINT_TEXTURE_IDENTIFIER,
];
/// Paper artwork sampled behind a filled map's server image.
pub const MAP_BACKGROUND_TEXTURE_IDENTIFIER: &str = "textures/map/map_background";

/// Shared equipment images retained alongside authored attachable textures.
pub fn shared_equipment_textures() -> impl Iterator<Item = &'static str> {
    GLINT_TEXTURE_IDENTIFIERS
        .into_iter()
        .chain([MAP_BACKGROUND_TEXTURE_IDENTIFIER])
}

pub const BED_GEOMETRY_IDENTIFIER: &str = "geometry.bed";
pub const CAPE_GEOMETRY_IDENTIFIER: &str = "geometry.cape";
pub const ELYTRA_GEOMETRY_IDENTIFIER: &str = "geometry.elytra";
pub const LEGACY_ENTITY_GEOMETRY_PATH: &str = "models/mobs.json";

pub(super) fn validate_symbol_source(kind: EntityAssetKind, path: &str) -> Result<(), AssetError> {
    let matches = match kind {
        EntityAssetKind::Entity => path.starts_with("entity/") && path.ends_with(".json"),
        EntityAssetKind::Attachable => path.starts_with("attachables/") && path.ends_with(".json"),
        EntityAssetKind::Geometry => {
            (path.starts_with("models/entity/") && path.ends_with(".json"))
                || path == LEGACY_ENTITY_GEOMETRY_PATH
        }
        EntityAssetKind::Animation => path.starts_with("animations/") && path.ends_with(".json"),
        EntityAssetKind::AnimationController => {
            path.starts_with("animation_controllers/") && path.ends_with(".json")
        }
        EntityAssetKind::RenderController => {
            path.starts_with("render_controllers/") && path.ends_with(".json")
        }
        EntityAssetKind::Texture => {
            path.starts_with("textures/") && (path.ends_with(".png") || path.ends_with(".tga"))
        }
    };
    if matches {
        Ok(())
    } else {
        Err(invalid("entity symbol kind does not match its source path"))
    }
}

pub(super) fn validate_relative_path(path: &str) -> Result<(), AssetError> {
    if path.is_empty()
        || path.len() > MAX_ENTITY_ASSET_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("entity source path is unsafe or exceeds its bound"));
    }
    Ok(())
}
