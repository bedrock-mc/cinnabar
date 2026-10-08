use super::*;

fn background() -> assets::EquipmentTexture {
    assets::EquipmentTexture {
        identifier: assets::MAP_BACKGROUND_TEXTURE_IDENTIFIER.into(),
        width: 1,
        height: 1,
        rgba8: Arc::from([83, 59, 37, 255]),
    }
}

#[test]
fn map_pixels_keep_paper_border_and_transparent_unexplored_pixels() {
    let paper = background();
    let mut image = client_world::MapImage {
        pixels: vec![0; IMAGE_SIDE * IMAGE_SIDE],
        revision: 1,
    };
    image.pixels[0] = 0xff33_2211;
    image.pixels[IMAGE_SIDE * IMAGE_SIDE - 1] = 0xffcc_bbaa;
    let atlas = paper_atlas(&paper, Some(&image)).unwrap();
    let pixel = |x, y| &atlas.rgba8[(y * PAPER_SIDE + x) * 4..(y * PAPER_SIDE + x + 1) * 4];
    assert_eq!(pixel(PAPER_BORDER, PAPER_BORDER), [0x11, 0x22, 0x33, 255]);
    assert_eq!(
        pixel(PAPER_BORDER + IMAGE_SIDE - 1, PAPER_BORDER + IMAGE_SIDE - 1),
        [0xaa, 0xbb, 0xcc, 255]
    );
    assert_eq!(pixel(PAPER_BORDER + 1, PAPER_BORDER), &paper.rgba8[..]);
    for corner in [
        (0, 0),
        (PAPER_SIDE - 1, 0),
        (0, PAPER_SIDE - 1),
        (PAPER_SIDE - 1, PAPER_SIDE - 1),
    ] {
        assert_eq!(pixel(corner.0, corner.1), &paper.rgba8[..]);
    }
    image.pixels[0] = 0xff66_5544;
    image.revision += 1;
    let updated = paper_atlas(&paper, Some(&image)).unwrap();
    assert_ne!(atlas.rgba8, updated.rgba8);
    let transparent = assets::EquipmentTexture {
        rgba8: Arc::from([0, 0, 0, 0]),
        ..paper
    };
    let atlas = paper_atlas(&transparent, Some(&image)).unwrap();
    let offset = (PAPER_BORDER * PAPER_SIDE + PAPER_BORDER) * 4;
    assert_eq!(&atlas.rgba8[offset..offset + 4], &[0x44, 0x55, 0x66, 255]);
}

#[test]
fn maps_use_native_two_hand_size_and_independent_one_hand_offsets() {
    let hand = FirstPersonHand {
        swing: 0.0,
        equip: 1.0,
        consume: None,
    };
    let center = Vec3::new(IMAGE_SIDE as f32 / 2.0, IMAGE_SIDE as f32 / 2.0, 0.0);
    let two = map_pose(hand, 90.0, false, true).unwrap();
    assert!((two.transform_point3(center) - Vec3::new(0.0, 0.04, -0.72)).length() < 0.0001);
    let right = map_pose(hand, 0.0, false, false)
        .unwrap()
        .transform_point3(center);
    let left = map_pose(hand, 0.0, true, false)
        .unwrap()
        .transform_point3(center);
    assert!((right.x - 0.513).abs() < 0.0001);
    assert!((left.x + 0.513).abs() < 0.0001);
    assert!((right.y - left.y).abs() < 0.0001);
    let hidden = map_pose(FirstPersonHand { equip: 0.0, ..hand }, 90.0, false, true).unwrap();
    assert!(
        (hidden.transform_point3(center).y - two.transform_point3(center).y + 1.2).abs() < 0.0001
    );
    assert!(map_pose(hand, f32::NAN, false, true).is_none());
}
