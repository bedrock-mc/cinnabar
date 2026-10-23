use super::*;

/// Labels every face and texel so an axis or orientation error changes the sampled color.
fn labelled_faces() -> PanoramaFaces {
    PanoramaFaces::new(
        4,
        std::array::from_fn(|face| {
            (0..4)
                .flat_map(|y| (0..4).flat_map(move |x| [face as u8 * 30, x * 40, y * 40, 255]))
                .collect()
        }),
    )
    .unwrap()
}

/// Builds a one-pixel camera that looks along a chosen axis.
fn view(yaw: f32, pitch: f32) -> PanoramaView {
    PanoramaView {
        yaw_radians: yaw.to_radians(),
        pitch_radians: pitch.to_radians(),
        vertical_fov_radians: 60.0_f32.to_radians(),
        aspect: 1.0,
        tint: [0.0; 4],
    }
}

#[test]
fn projection_centers_select_all_six_pack_faces() {
    let faces = labelled_faces();
    for (yaw, pitch, face) in [
        (0.0, 0.0, 0),
        (90.0, 0.0, 1),
        (180.0, 0.0, 2),
        (270.0, 0.0, 3),
        (0.0, 90.0, 4),
        (0.0, -90.0, 5),
    ] {
        assert_eq!(
            faces.project(view(yaw, pitch), [1, 1]).unwrap(),
            [face * 30, 64, 64, 255]
        );
    }
}

#[test]
fn known_rays_select_the_correct_texels_on_every_face() {
    let faces = labelled_faces();
    for (face, dir) in [
        (0, Vec3::new(0.25, 0.75, -1.0)),
        (1, Vec3::new(1.0, 0.75, 0.25)),
        (2, Vec3::new(-0.25, 0.75, 1.0)),
        (3, Vec3::new(-1.0, 0.75, -0.25)),
        (4, Vec3::new(0.25, 1.0, 0.75)),
        (5, Vec3::new(0.25, -1.0, -0.75)),
    ] {
        assert_eq!(faces.sample(dir).map(linear_to_srgb), [face * 30, 80, 0]);
    }
}

#[test]
fn perspective_spans_adjacent_faces_and_filters_the_shared_edge() {
    let faces = PanoramaFaces::new(
        2,
        std::array::from_fn(|face| [face as u8 * 30, 0, 0, 255].repeat(4)),
    )
    .unwrap();
    let pixels = faces.project(view(45.0, 0.0), [3, 1]).unwrap();
    assert_eq!(pixels[0], 0);
    assert_eq!(pixels[4], 19);
    assert_eq!(pixels[8], 30);
    for face in 0..6 {
        for uv in [[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]] {
            let a = faces.sample(face_direction(face, uv.map(|v| v * 0.9999)));
            let b = faces.sample(face_direction(face, uv.map(|v| v * 1.0001)));
            assert!((a[0] - b[0]).abs() < 0.0001, "face {face}, edge {uv:?}");
        }
    }
}

#[test]
fn invalid_projection_has_no_image() {
    let faces = labelled_faces();
    assert!(faces.project(view(0.0, 0.0), [0, 1]).is_none());
    assert!(faces.project(view(f32::NAN, 0.0), [1, 1]).is_none());
    let mut invalid = view(0.0, 0.0);
    invalid.vertical_fov_radians = std::f32::consts::PI;
    assert!(faces.project(invalid, [1, 1]).is_none());
}

#[test]
fn panorama_filtering_blends_light_before_encoding_srgb() {
    let faces = PanoramaFaces::new(
        2,
        std::array::from_fn(|_| {
            [
                [0, 0, 0, 255],
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                [255, 255, 255, 255],
            ]
            .concat()
        }),
    )
    .unwrap();
    assert_eq!(
        faces.project(view(0.0, 0.0), [1, 1]).unwrap(),
        [188, 188, 188, 255]
    );
}

#[test]
fn panorama_incomplete_and_mismatched_face_sets_are_rejected() {
    let png = |side| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(side, side, image::Rgba([40, 80, 120, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    for missing in 0..6 {
        assert!(crate::panorama::decode_faces(|face| (face != missing).then(|| png(2))).is_none());
    }
    assert!(
        crate::panorama::decode_faces(|face| Some(png(if face == 5 { 4 } else { 2 }))).is_none()
    );
}
