use super::*;
use std::sync::Arc;

/// Builds original texels with distinct face, hat and unrelated body regions.
fn skin(side: u32, legacy: bool) -> StandardSkin {
    let height = if legacy { side / 2 } else { side };
    let scale = side / CLASSIC_SKIN_SIDE as u32;
    let mut rgba = vec![0; (side * height * 4) as usize];
    for y in 8 * scale..16 * scale {
        for x in 8 * scale..16 * scale {
            let offset = ((y * side + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[x as u8, y as u8, 40, 255]);
        }
    }
    StandardSkin {
        width: side,
        height,
        rgba8: rgba.into(),
        cape: None,
        geometry: None,
    }
}

/// Adds a named modern model over the original fixture pixels.
fn model(skin: &mut StandardSkin, bones: serde_json::Value) {
    skin.geometry = Some(Arc::new(render_api::SkinGeometrySource {
        resource_patch: r#"{"geometry":{"default":"geometry.portrait"}}"#.into(),
        geometry_data: serde_json::json!({"minecraft:geometry":[{"description":{"identifier":"geometry.portrait","texture_width":64,"texture_height":64},"bones":bones}]}).to_string().into(),
        animations: Arc::from([]),
    }));
}

#[test]
fn stock_and_legacy_heads_preserve_hd_texels_and_composite_hat_alpha() {
    for side in crate::SKIN_CLASS_SIDES.map(|side| side as u32) {
        for legacy in [false, true] {
            let mut skin = skin(side, legacy);
            let mut rgba = skin.rgba8.to_vec();
            let scale = side / CLASSIC_SKIN_SIDE as u32;
            let offset = ((8 * scale * side + 40 * scale) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[200, 100, 0, 128]);
            skin.rgba8 = rgba.into();
            let face = extract(&skin).unwrap();
            assert_eq!(face.dimensions(), [8 * scale; 2]);
            let first = face.pixels();
            assert_eq!(first[3], 255);
            assert_eq!(first[2], 20);
            assert_eq!(
                &first[4..8],
                &[8 * scale as u8 + 1, 8 * scale as u8, 40, 255]
            );
        }
    }
}

#[test]
fn geometry_front_uvs_and_mirroring_choose_the_transmitted_face() {
    let mut skin = skin(64, false);
    model(
        &mut skin,
        serde_json::json!([{"name":"head","mirror":true,"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]),
    );
    assert_eq!(&extract(&skin).unwrap().pixels()[..4], &[15, 8, 40, 255]);
    model(
        &mut skin,
        serde_json::json!([{"name":"head","cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":{"north":{"uv":[8,8],"uv_size":[8,8]}}}]}]),
    );
    assert_eq!(&extract(&skin).unwrap().pixels()[..4], &[8, 8, 40, 255]);
}

#[test]
fn malformed_invisible_rotated_and_out_of_bounds_heads_request_fallback() {
    let mut invisible = skin(64, false);
    invisible.rgba8 = vec![0; 64 * 64 * 4].into();
    assert!(extract(&invisible).is_none());
    let mut invalid = skin(64, false);
    invalid.rgba8 = vec![0; 4].into();
    assert!(extract(&invalid).is_none());
    for bone in [
        serde_json::json!({"name":"body","cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}),
        serde_json::json!({"name":"head","rotation":[0,45,0],"cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}),
        serde_json::json!({"name":"head","cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[128,128]}]}),
    ] {
        let mut skin = skin(64, false);
        model(&mut skin, serde_json::json!([bone]));
        assert!(extract(&skin).is_none());
    }
}

#[test]
fn animated_face_uses_first_frame_and_named_geometry() {
    let mut skin = skin(64, false);
    let mut pixels = vec![0; 64 * 128 * 4];
    pixels[..64 * 64 * 4].copy_from_slice(&skin.rgba8);
    pixels[64 * 64 * 4..].fill(255);
    skin.geometry = Some(Arc::new(render_api::SkinGeometrySource {
        resource_patch: r#"{"geometry":{"default":"geometry.face","animated_face":"geometry.face"}}"#.into(),
        geometry_data: r#"{"minecraft:geometry":[{"description":{"identifier":"geometry.face","texture_width":64,"texture_height":64},"bones":[{"name":"head","cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.into(),
        animations: Arc::from([render_api::SkinAnimation { kind: SkinAnimationKind::Face, width:64, height:128, frames:2, blinking:false, rgba8: pixels.into() }]),
    }));
    assert_eq!(&extract(&skin).unwrap().pixels()[..4], &[8, 8, 40, 255]);
}

#[test]
fn polygon_front_respects_bottom_left_uv_origin() {
    for normalized in [false, true] {
        let mut skin = skin(CLASSIC_SKIN_SIDE as u32, false);
        let divisor = if normalized {
            CLASSIC_SKIN_SIDE as f32
        } else {
            1.0
        };
        let uvs = [[8.0, 56.0], [16.0, 56.0], [16.0, 48.0], [8.0, 48.0]]
            .map(|uv| uv.map(|v| v / divisor));
        model(
            &mut skin,
            serde_json::json!([{"name":"head","poly_mesh":{
                "normalized_uvs":normalized,
                "positions":[[-4,32,-4],[4,32,-4],[4,24,-4],[-4,24,-4]],
                "normals":[[0,0,-1]], "uvs":uvs,
                "polys":[[[0,0,0],[1,0,1],[2,0,2],[3,0,3]]]
            }}]),
        );
        let portrait = extract(&skin).unwrap();
        assert_eq!(&portrait.pixels()[..4], &[8, 8, 40, 255]);
        assert_eq!(
            &portrait.pixels()[portrait.pixels().len() - 4..],
            &[15, 15, 40, 255]
        );
    }
}
