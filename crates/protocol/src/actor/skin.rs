use super::*;
use sha2::{Digest, Sha256};

pub(crate) fn normalize_player_skin(
    skin: valentine::bedrock::version::v1_26_30::Skin,
    retained_bytes: &mut usize,
) -> PlayerSkin {
    if skin.persona {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona);
    }
    if !skin.animations.is_empty()
        || !skin.personal_pieces.is_empty()
        || !skin.piece_tint_colors.is_empty()
        || !skin.animation_data.is_empty()
    {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedAppearance);
    }
    let geometry = match normalize_player_skin_geometry(
        &skin.arm_size,
        &skin.skin_resource_pack,
        &skin.geometry_data,
    ) {
        Ok(geometry) => geometry,
        Err(unavailable) => return PlayerSkin::Unavailable(unavailable),
    };
    let Ok(width) = u32::try_from(skin.skin_data.width) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    let Ok(height) = u32::try_from(skin.skin_data.height) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    if !matches!(
        (width, height),
        (64, 32) | (64, 64) | (128, 128) | (MAX_STANDARD_SKIN_SIDE, MAX_STANDARD_SKIN_SIDE)
    ) {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    }
    let Some(expected_bytes) = usize::try_from(width)
        .ok()
        .and_then(|width| usize::try_from(height).ok().map(|height| (width, height)))
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    if skin.skin_data.data.len() != expected_bytes {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidByteLength);
    }
    let Some(next_bytes) = retained_bytes.checked_add(expected_bytes) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    };
    if next_bytes > MAX_PLAYER_LIST_SKIN_BYTES {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    }
    *retained_bytes = next_bytes;
    PlayerSkin::Standard(StandardSkin {
        width,
        height,
        rgba8: Arc::from(skin.skin_data.data),
        geometry,
    })
}

pub(super) fn normalize_player_skin_geometry(
    arm_size: &str,
    resource_patch: &str,
    geometry_data: &str,
) -> Result<PlayerSkinGeometry, PlayerSkinUnavailable> {
    let (standard, expected_identifier) = match arm_size {
        "wide" => (PlayerSkinGeometry::Wide, "geometry.humanoid.custom"),
        "slim" => (PlayerSkinGeometry::Slim, "geometry.humanoid.customSlim"),
        _ => return Err(PlayerSkinUnavailable::InvalidArmSize),
    };
    if resource_patch.is_empty() && geometry_data.is_empty() {
        return Ok(standard);
    }
    let patch_identifier = skin_resource_patch_identifier(resource_patch)?;
    if patch_identifier != expected_identifier {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    if geometry_data.is_empty() {
        return Ok(standard);
    }
    if geometry_data.len() > MAX_PLAYER_SKIN_GEOMETRY_BYTES {
        return Err(PlayerSkinUnavailable::GeometryTooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_str(geometry_data).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?;
    validate_skin_geometry_tree(&value, 0, &mut 0)?;
    let geometry = select_skin_geometry(&value, &patch_identifier)?;
    Ok(PlayerSkinGeometry::Custom {
        identifier: Arc::from(patch_identifier),
        data_sha256: Sha256::digest(
            serde_json::to_vec(geometry).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?,
        )
        .into(),
    })
}

fn skin_resource_patch_identifier(patch: &str) -> Result<String, PlayerSkinUnavailable> {
    if patch.is_empty() || patch.len() > 4_096 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    let value: serde_json::Value =
        serde_json::from_str(patch).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?;
    validate_skin_geometry_tree(&value, 0, &mut 0)?;
    let root = value
        .as_object()
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    if root.len() != 1 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    let geometry = root
        .get("geometry")
        .and_then(serde_json::Value::as_object)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    if geometry.len() != 1 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    geometry
        .get("default")
        .and_then(serde_json::Value::as_str)
        .filter(|identifier| identifier.len() <= MAX_ACTOR_IDENTIFIER_BYTES)
        .map(str::to_owned)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)
}

fn validate_skin_geometry_tree(
    value: &serde_json::Value,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), PlayerSkinUnavailable> {
    if depth > MAX_PLAYER_SKIN_GEOMETRY_DEPTH {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    *nodes = nodes
        .checked_add(1)
        .filter(|nodes| *nodes <= MAX_PLAYER_SKIN_GEOMETRY_NODES)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                validate_skin_geometry_tree(value, depth + 1, nodes)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_skin_geometry_tree(value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn select_skin_geometry<'a>(
    value: &'a serde_json::Value,
    selected: &str,
) -> Result<&'a serde_json::Value, PlayerSkinUnavailable> {
    if let Some(geometries) = value.get("minecraft:geometry") {
        let geometries = geometries
            .as_array()
            .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
        let matches = geometries
            .iter()
            .filter(|geometry| {
                geometry
                    .get("description")
                    .and_then(|description| description.get("identifier"))
                    .and_then(serde_json::Value::as_str)
                    == Some(selected)
            })
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [geometry] => Ok(*geometry),
            _ => Err(PlayerSkinUnavailable::InvalidGeometry),
        };
    }
    let object = value
        .as_object()
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    let matches = object
        .iter()
        .filter(|(identifier, _)| identifier.split(':').next() == Some(selected))
        .map(|(_, geometry)| geometry)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [geometry] => Ok(*geometry),
        _ => Err(PlayerSkinUnavailable::InvalidGeometry),
    }
}
