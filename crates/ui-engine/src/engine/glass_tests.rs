use super::*;

/// A two-triangle square with partial coverage on one corner, like a rounded shape's fringe.
fn shape() -> UiMesh {
    let vertex = |position: [f32; 2], coverage: f32| UiMeshVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [position[0], coverage],
        color: [9, 8, 7, 255],
        model_light: 0.5,
        overlay_color: [513.0; 4],
        style_flags: render_api::UI_STYLE_COVERAGE_PAINT,
        alpha_test: false,
    };
    UiMesh::new(
        vec![
            vertex([0.0, 0.0], 0.25),
            vertex([1.0, 0.0], 1.0),
            vertex([1.0, 1.0], 1.0),
            vertex([0.0, 1.0], 1.0),
        ]
        .into(),
        Arc::from([0, 1, 2, 0, 2, 3]),
        Arc::from([UiMeshBatch {
            texture_page: 3,
            index_range: 0..6,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .unwrap()
}

#[test]
fn backdrop_geometry_keeps_the_outline_and_coverage_and_carries_only_its_paint() {
    let paint = UiBackdropPaint {
        blur: 52.0,
        saturation: 1.12,
        brightness: 0.62,
    };
    let source = shape();
    let backdrop = backdrop_of(&source, 128, paint).unwrap();
    assert_eq!(backdrop.indices(), source.indices());
    assert_eq!(backdrop.batches().len(), 1);
    let batch = &backdrop.batches()[0];
    assert_eq!(batch.blend, UiBlendMode::Backdrop);
    assert_eq!(batch.texture_page, 3);
    assert_eq!(batch.index_range, 0..6);
    for (vertex, original) in backdrop.vertices().iter().zip(source.vertices()) {
        assert_eq!(vertex.position, original.position);
        assert_eq!(vertex.uv[1], original.uv[1]);
        assert_eq!(vertex.color[3], 128);
        assert_eq!(vertex.model_light, 0.0);
        assert_eq!(vertex.style_flags, 0);
        assert_eq!(UiBackdropPaint::from_overlay(vertex.overlay_color), paint);
    }
    // Fading never reinterprets the paint as packed colours.
    let faded = backdrop.clone().with_opacity(0.5);
    assert_eq!(faded.vertices()[0].color[3], 64);
    assert_eq!(faded.vertices()[0].overlay_color, paint.overlay());
}

#[test]
fn world_backdrop_covers_its_node_completely() {
    let paint = Backdrop {
        blur: 3.0,
        saturation: 1.0,
        brightness: 0.62,
    }
    .paint(2.0);
    assert_eq!(paint.blur, 6.0);
    let quad = cover_quad(1, paint, UiBlendMode::Backdrop).unwrap();
    assert_eq!(quad.batches()[0].blend, UiBlendMode::Backdrop);
    let corners: Vec<_> = quad
        .vertices()
        .iter()
        .map(|vertex| vertex.position)
        .collect();
    assert_eq!(corners, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    assert!(
        quad.vertices()
            .iter()
            .all(|vertex| vertex.uv[1] == 1.0 && vertex.color[3] == 255)
    );
}

#[test]
fn authored_backdrops_clamp_and_identity_draws_nothing() {
    let read = Backdrop::read(|property| match property.name {
        "blur" => Some(80.0),
        "saturation" => Some(-1.0),
        _ => None,
    });
    assert_eq!(read.blur, BLUR.max);
    assert_eq!(read.saturation, 0.0);
    assert_eq!(read.brightness, 1.0);
    assert!(Backdrop::read(|_| None).is_identity());
    assert!(!read.is_identity());
}

#[test]
fn highlight_keeps_the_top_edge_and_fades_out_down_each_corner() {
    let border = shape();
    // Normalized y 0 is the top edge and 1 the bottom, in a 20-pixel-tall shape.
    let band = top_band(&border, 20.0, 8.0, 1.0).unwrap();
    let lights: Vec<_> = band
        .vertices()
        .iter()
        .map(|vertex| (vertex.position[1], vertex.model_light))
        .collect();
    assert_eq!(lights, [(0.0, 0.5), (0.0, 0.5), (1.0, 0.0), (1.0, 0.0)]);
    assert_eq!(band.indices(), border.indices());
    let halfway = top_band(&border, 2.0 * (8.0 + 1.0) / 2.0, 8.0, 1.0).unwrap();
    assert!(
        halfway
            .vertices()
            .iter()
            .all(|vertex| (0.0..=0.5).contains(&vertex.model_light))
    );
}
