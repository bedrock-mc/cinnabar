use super::native_tests::{fixture, hand_source_for_mode};
use super::{maps::*, *};

/// Publishes an original one-row pixel update through the ordered world ingress.
fn update(stream: &mut WorldStream, id: i64, x: u32, pixels: &[u32]) {
    stream
        .submit(
            stream.committed_sequence() + 1,
            client_world::ingestion::WorldEvent::MapData(protocol::MapDataEvent {
                map_id: id,
                start_x: x,
                start_y: 0,
                width: pixels.len() as u32,
                height: 1,
                pixels: Arc::from(pixels),
            }),
        )
        .unwrap();
}

#[test]
fn held_map_images_keep_rectangles_and_independent_hands_without_warm_pixel_work() {
    let (mut stream, _, _, _) = fixture(None);
    update(&mut stream, 266, 0, &[0xff332211, 0xff665544]);
    update(&mut stream, -267, 0, &[0xff998877]);
    let mut cache = MapCache::default();
    let main = cache.image(&stream, 0, Some(266)).unwrap();
    let off = cache.image(&stream, 1, Some(-267)).unwrap();
    assert_eq!(
        &main.rgba8[..8],
        &[0x11, 0x22, 0x33, 0xff, 0x44, 0x55, 0x66, 0xff]
    );
    assert_eq!(&off.rgba8[..4], &[0x77, 0x88, 0x99, 0xff]);
    let allocations = test_alloc::thread_allocations();
    let unchanged = cache.image(&stream, 0, Some(266)).unwrap();
    assert_eq!(allocations, test_alloc::thread_allocations());
    assert!(Arc::ptr_eq(&main.rgba8, &unchanged.rgba8));
    update(&mut stream, 266, 1, &[0xffabcdef]);
    let patched = cache.image(&stream, 0, Some(266)).unwrap();
    assert_eq!(
        &patched.rgba8[..8],
        &[0x11, 0x22, 0x33, 0xff, 0xef, 0xcd, 0xab, 0xff]
    );
    assert!(Arc::ptr_eq(
        &off.rgba8,
        &cache.image(&stream, 1, Some(-267)).unwrap().rgba8
    ));
    assert!(cache.image(&stream, 0, None).is_none());
}

#[test]
fn replaced_and_new_session_maps_cannot_reuse_pixels_with_the_same_revision() {
    let (mut stream, _, _, _) = fixture(None);
    update(&mut stream, 266, 0, &[1]);
    let mut cache = MapCache::default();
    let old = cache.image(&stream, 0, Some(266)).unwrap();
    let mut session_cache = MapCache::default();
    session_cache.image(&stream, 0, Some(266)).unwrap();
    for id in 0..65 {
        update(&mut stream, id, 0, &[2]);
    }
    assert!(
        stream.map_image(266).is_none(),
        "fixture must evict the original image"
    );
    assert!(stream.replaced_maps() > 0);
    update(&mut stream, 266, 0, &[3]);
    assert_eq!(stream.map_image(266).unwrap().revision, 1);
    let replaced = cache.image(&stream, 0, Some(266)).unwrap();
    assert_eq!(replaced.rgba8[0], 3);
    assert!(!Arc::ptr_eq(&old.rgba8, &replaced.rgba8));
    let (mut next, _, _, _) = fixture(None);
    update(&mut next, 266, 0, &[4]);
    assert_eq!(next.replaced_maps(), 0);
    assert_eq!(next.map_image(266).unwrap().revision, 1);
    assert_eq!(
        session_cache.image(&next, 0, Some(266)).unwrap().rgba8[0],
        4
    );
}

#[test]
fn filled_map_submits_paper_and_server_pixels_in_both_animation_modes() {
    for java in [false, true] {
        let (mut stream, mut equipment, artwork, mut input) =
            fixture(Some(assets::facts::FILLED_MAP_IDENTIFIER));
        input.main.as_mut().unwrap().map_id = Some(266);
        update(&mut stream, 266, 0, &[0xff332211]);
        let mut cache = java::HandCache::default();
        let source = hand_source_for_mode(
            &stream,
            &mut equipment,
            &artwork,
            &input,
            &mut cache,
            1.0,
            java,
        );
        assert!(
            source.maps.paper[0].is_some(),
            "held map must draw its pack paper"
        );
        assert!(
            source.items[0].is_some(),
            "held map must submit the received image"
        );
        assert_eq!(
            &source.items[0].as_ref().unwrap().1.rgba8[..4],
            &[0x11, 0x22, 0x33, 0xff]
        );
        assert_eq!(
            equipment
                .take_pending_geometries()
                .iter()
                .filter(|geometry| geometry.vertices.len() == 6)
                .count(),
            2
        );
    }
}

#[test]
fn missing_pixels_keep_paper_and_each_hand_switches_its_own_map() {
    let (mut stream, mut equipment, artwork, mut input) =
        fixture(Some(assets::facts::FILLED_MAP_IDENTIFIER));
    input.main.as_mut().unwrap().map_id = Some(266);
    input.off = input.main.clone();
    input.off.as_mut().unwrap().map_id = Some(267);
    let mut cache = java::HandCache::default();
    let missing = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    assert!(missing.maps.paper.iter().all(Option::is_some));
    assert!(missing.items.iter().all(Option::is_none));
    update(&mut stream, 266, 0, &[0xff332211]);
    update(&mut stream, 267, 0, &[0xff665544]);
    let both = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    let main = both.items[0].as_ref().unwrap().1.rgba8.clone();
    let off = both.items[1].as_ref().unwrap().1.rgba8.clone();
    assert_eq!(main[0], 0x11);
    assert_eq!(off[0], 0x44);
    update(&mut stream, 268, 0, &[0xff998877]);
    input.main.as_mut().unwrap().map_id = Some(268);
    let switched = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    assert_eq!(switched.items[0].as_ref().unwrap().1.rgba8[0], 0x77);
    assert!(Arc::ptr_eq(
        &off,
        &switched.items[1].as_ref().unwrap().1.rgba8
    ));
    input.off = None;
    let main_only = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    assert!(main_only.maps.paper[1].is_none());
    assert!(main_only.items[1].is_none());
}

#[test]
fn pack_paper_keeps_resolution_and_alpha_and_rolls_back_without_rebuilding_map_meshes() {
    let ((mut stream, mut equipment, artwork, mut input), entities) =
        super::native_tests::fixture_with_clock(Some(assets::facts::FILLED_MAP_IDENTIFIER), false);
    input.main.as_mut().unwrap().map_id = Some(266);
    update(&mut stream, 266, 0, &[0xff332211]);
    let mut cache = java::HandCache::default();
    let startup = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    let startup_paper = startup.maps.background.unwrap();
    equipment.take_pending_geometries();
    let pixels: Arc<[u8]> = [10, 20, 30, 127].repeat(257 * 129).into();
    let catalog = Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            vec![],
            vec![assets::EquipmentTexture {
                identifier: assets::entity::MAP_BACKGROUND_TEXTURE_IDENTIFIER.into(),
                width: 257,
                height: 129,
                rgba8: pixels.clone(),
            }],
        )
        .unwrap(),
    );
    equipment.set_pack_layer(Some((entities, Some(catalog), vec![None])));
    let changed = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    let paper = changed.maps.background.unwrap();
    assert_eq!((paper.width, paper.height, paper.layers), (257, 129, 1));
    assert!(Arc::ptr_eq(&pixels, &paper.rgba8));
    assert!(equipment.take_pending_geometries().is_empty());
    equipment.set_pack_layer(None);
    let restored = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    assert!(Arc::ptr_eq(
        &startup_paper.rgba8,
        &restored.maps.background.unwrap().rgba8
    ));
}

#[test]
fn equipped_map_is_visible_and_the_equip_dip_moves_it_below_the_view() {
    let hand = FirstPersonHand {
        swing: 0.0,
        equip: 1.0,
        consume: None,
        aspect: 16.0 / 9.0,
    };
    let center = bevy::math::Vec3::new(
        protocol::MAP_IMAGE_SIDE as f32 * 0.5,
        protocol::MAP_IMAGE_SIDE as f32 * 0.5,
        0.0,
    );
    let held = map_camera(hand, 30.0, false, false).transform_point3(center);
    assert!(held.z < -0.5 && held.z > -1.0);
    assert!(
        held.x.abs() < 0.1 && held.y > -0.5 && held.y < 0.5,
        "equipped map center must stay on screen: {held}"
    );
    let lowered = map_camera(FirstPersonHand { equip: 0.0, ..hand }, 30.0, false, false)
        .transform_point3(center);
    assert!((held.y - lowered.y - 1.2).abs() < 1e-5);
    assert!(lowered.y < -1.0);
}

#[test]
fn held_map_wire_colors_reach_the_rgba_atlas() {
    let (mut stream, _, _, _) = fixture(None);
    let expected = [
        [0x80, 0x80, 0x50, 0xff],
        [0x11, 0x22, 0x33, 0x44],
        [0x12, 0x34, 0x56, 0],
    ];
    stream
        .submit(
            stream.committed_sequence() + 1,
            protocol::test_support::packets::map_pixel_row(266, &expected),
        )
        .unwrap();
    let mut cache = MapCache::default();
    let atlas = cache.image(&stream, 0, Some(266)).unwrap();
    assert_eq!(
        &atlas.rgba8[..expected.len() * 4],
        expected.concat(),
        "held map must preserve wire colors and alpha"
    );
}
