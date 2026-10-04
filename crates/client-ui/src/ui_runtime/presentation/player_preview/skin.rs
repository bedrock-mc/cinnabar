//! Validate UI skin texels without resampling them into the world skin array.

use std::sync::Arc;

use render::ActorSkinPixels;

/// The local player's validated skin; off-world the launcher's paper doll wears the menu skin.
pub fn local_preview_skin(
    stream: Option<&chunk_pipeline::WorldStream>,
    menu_skin: &render::ActorSkinPixels,
) -> Option<Arc<[u8]>> {
    let pixels = match stream {
        Some(stream) => match &stream
            .authority()
            .actor_player_profile(stream.local_player_runtime_id())?
            .skin
        {
            protocol::PlayerSkin::Standard(skin) => ActorSkinPixels {
                width: skin.width,
                height: skin.height,
                rgba8: skin.rgba8.clone(),
            },
            _ => return None,
        },
        None => ActorSkinPixels {
            width: menu_skin.width,
            height: menu_skin.height,
            rgba8: menu_skin.rgba8.clone(),
        },
    };
    validated_ui_skin(&pixels)
}

/// Native model UVs address the supplied skin's texels directly. Square HD
/// skins retain their dimensions and allocation; legacy half-height skins use
/// the shared Bedrock limb expansion, still at the original texel density.
pub fn validated_ui_skin(skin: &render::ActorSkinPixels) -> Option<Arc<[u8]>> {
    let width = skin.width;
    if !width.is_power_of_two()
        || width < render_api::CLASSIC_SKIN_SIDE as u32
        || width > render_api::MAX_STANDARD_SKIN_SIDE
        || (skin.height != width && skin.height.checked_mul(2) != Some(width))
    {
        return None;
    }
    let side = usize::try_from(width).ok()?;
    let height = usize::try_from(skin.height).ok()?;
    let bytes = side.checked_mul(height)?.checked_mul(4)?;
    if skin.rgba8.len() != bytes {
        return None;
    }
    Some(if side == height {
        Arc::clone(skin.rgba8.pixels())
    } else {
        render_api::expand_legacy_skin_rgba8(&skin.rgba8, side).into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_skin_keeps_odd_fine_texels_and_original_allocation() {
        let side = render_api::CLASSIC_SKIN_SIDE * 2;
        let mut rgba = vec![0; side * side * 4];
        let fine = (11 * side + 13) * 4;
        rgba[fine..fine + 4].copy_from_slice(&[17, 43, 199, 255]);
        let skin = render::ActorSkinPixels {
            width: side as u32,
            height: side as u32,
            rgba8: rgba.into(),
        };
        let validated = validated_ui_skin(&skin).expect("supported HD skin");
        assert!(Arc::ptr_eq(skin.rgba8.pixels(), &validated));
        assert_eq!(validated.len(), side * side * 4);
        assert_eq!(&validated[fine..fine + 4], &[17, 43, 199, 255]);
    }

    #[test]
    fn legacy_skin_expands_without_changing_texel_density() {
        let side = render_api::CLASSIC_SKIN_SIDE * 2;
        let source = render::ActorSkinPixels {
            width: side as u32,
            height: side as u32 / 2,
            rgba8: vec![255; side * side / 2 * 4].into(),
        };
        let validated = validated_ui_skin(&source).expect("supported legacy HD skin");
        assert_eq!(validated.len(), side * side * 4);
        assert_eq!(
            validated.as_ref(),
            render_api::expand_legacy_skin_rgba8(&source.rgba8, side)
        );
    }

    #[test]
    fn malformed_or_unsupported_skin_is_not_resampled() {
        let valid_side = render_api::CLASSIC_SKIN_SIDE as u32;
        for (width, height, bytes) in [
            (valid_side, valid_side, 1),
            (valid_side + 1, valid_side + 1, 0),
            (render_api::MAX_STANDARD_SKIN_SIDE * 2, valid_side, 0),
            (valid_side, valid_side / 4, 0),
        ] {
            let skin = render::ActorSkinPixels {
                width,
                height,
                rgba8: vec![0; bytes].into(),
            };
            assert!(validated_ui_skin(&skin).is_none());
        }
    }
}
