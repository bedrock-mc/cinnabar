use super::*;

#[test]
fn block_face_orientations_and_horizontal_ties_match() {
    let expected = [
        (Vec3::X, Vec3::NEG_Z),
        (Vec3::X, Vec3::Z),
        (Vec3::X, Vec3::Y),
        (Vec3::NEG_X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y),
        (Vec3::Z, Vec3::Y),
    ];
    for (face, (right, up)) in expected.into_iter().enumerate() {
        let quad = AimAssistHighlight::block(Vec3::ONE, face as u8, Vec3::Z).unwrap();
        assert_eq!((quad.center, quad.right, quad.up), (Vec3::ONE, right, up));
    }
    let positive_z_tie = AimAssistHighlight::block(Vec3::ZERO, 1, Vec3::new(1., 0., 1.)).unwrap();
    assert_eq!(positive_z_tie.right, Vec3::X);
    let negative_z_tie = AimAssistHighlight::block(Vec3::ZERO, 1, Vec3::new(-1., 0., -1.)).unwrap();
    assert_eq!(negative_z_tie.right, Vec3::NEG_X);
    assert!(AimAssistHighlight::block(Vec3::ZERO, 6, Vec3::Z).is_none());
}

#[test]
fn actor_billboard_preserves_roll_and_unit_size_without_allocating() {
    let before = crate::alloc_count::thread_allocations();
    for _ in 0..100 {
        let quad = AimAssistHighlight::actor(Vec3::ONE, Vec3::Z, Vec3::X).unwrap();
        assert_eq!(quad.right, Vec3::NEG_Y);
        assert_eq!(quad.up, Vec3::X);
        assert_eq!(quad.texture, 1);
        assert_eq!(quad.record()[0..3], [1.; 3]);
    }
    assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
    assert!(AimAssistHighlight::actor(Vec3::ZERO, Vec3::ZERO, Vec3::Y).is_none());
}

#[test]
fn malformed_texture_pixels_never_enter_the_render_scene() {
    assert!(AimAssistTexture::new([0, 1], Arc::from([])).is_none());
    assert!(AimAssistTexture::new([2, 2], Arc::from([0; 4])).is_none());
    assert!(AimAssistTexture::new([1, 1], Arc::from([0; 4])).is_some());
}
