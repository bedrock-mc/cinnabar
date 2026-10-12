use super::*;
use crate::{Beam, Billboard, Decal, Ribbon};

#[test]
fn decals_lie_flat_on_their_centre_height() {
    let vertices = build(&Primitives {
        decals: vec![Decal {
            center: [10.0, 64.0, -5.0],
            radius: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
            progress: 1.5,
            style: DecalStyle::Crater,
        }],
        ..Default::default()
    });
    assert_eq!(vertices.len(), 6);
    for v in &vertices {
        assert_eq!(v.anchor[1], 64.0 + DECAL_LIFT_BLOCKS);
        assert_eq!(v.anchor[3], KIND_GROUND);
        assert_eq!((v.anchor[0] - 10.0).abs(), 2.0);
        assert_eq!((v.anchor[2] + 5.0).abs(), 2.0);
        assert_eq!(v.style[0], decal_style_id(DecalStyle::Crater));
        assert_eq!(v.style[1], 1.0, "progress clamps to 1");
    }
}

#[test]
fn ribbons_emit_two_triangles_per_segment_and_taper_to_the_tail() {
    let vertices = build(&Primitives {
        ribbons: vec![Ribbon {
            taper: true,
            pattern: crate::LinePattern::Solid,
            points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
            ],
            width: 1.0,
            color: [1.0; 4],
        }],
        ..Default::default()
    });
    assert_eq!(vertices.len(), 3 * 6);
    let head = vertices.iter().find(|v| v.uv[1] == 0.0).unwrap();
    let tail = vertices.iter().find(|v| v.uv[1] == 1.0).unwrap();
    assert_eq!(head.shape[3], 0.5);
    assert!((tail.shape[3] - 0.5 * RIBBON_TAIL_WIDTH).abs() < 1e-6);
    for v in &vertices {
        assert_eq!(v.anchor[3], KIND_STRIP);
        assert!(
            v.shape.iter().all(|c| c.is_finite()),
            "repeated points stay finite"
        );
        assert_eq!(v.uv[0].abs(), 1.0);
    }
}

#[test]
fn beams_carry_length_and_intensity_for_animation() {
    let vertices = build(&Primitives {
        beams: vec![Beam {
            start: [0.0, 0.0, 0.0],
            end: [0.0, 0.0, 12.0],
            width: 1.5,
            color: [0.3, 0.6, 1.0, 1.0],
            intensity: 2.0,
        }],
        ..Default::default()
    });
    assert_eq!(vertices.len(), 6);
    for v in &vertices {
        assert_eq!(&v.shape[..3], &[0.0, 0.0, 1.0]);
        assert_eq!(v.shape[3], 0.75);
        assert_eq!(v.style, [STYLE_BEAM, 0.0, 12.0, 2.0]);
    }
}

#[test]
fn billboards_keep_anchor_and_select_facing_mode() {
    let billboard = Billboard {
        position: [1.0, 2.0, 3.0],
        width: 2.0,
        height: 4.0,
        color: [1.0; 4],
        pattern: BillboardPattern::Silhouette,
        upright: true,
    };
    let vertices = build(&Primitives {
        billboards: vec![
            billboard,
            Billboard {
                upright: false,
                pattern: BillboardPattern::Sphere,
                ..billboard
            },
        ],
        ..Default::default()
    });
    assert_eq!(vertices.len(), 12);
    assert!(
        vertices[..6]
            .iter()
            .all(|v| v.anchor == [1.0, 2.0, 3.0, KIND_UPRIGHT])
    );
    assert!(vertices[6..].iter().all(|v| v.anchor[3] == KIND_BILLBOARD));
    assert_eq!(vertices[0].shape, [1.0, 2.0, 0.0, 0.0]);
    assert_eq!(
        vertices[0].style[0],
        billboard_style_id(BillboardPattern::Silhouette)
    );
    assert_eq!(
        vertices[6].style[0],
        billboard_style_id(BillboardPattern::Sphere)
    );
}

#[test]
fn full_budgets_fit_the_vertex_bound() {
    let ribbon = Ribbon {
        taper: true,
        pattern: crate::LinePattern::Solid,
        points: vec![[0.0; 3]; mod_api::MAX_RIBBON_POINTS],
        width: 1.0,
        color: [1.0; 4],
    };
    let primitives = Primitives {
        ribbons: vec![ribbon; mod_api::MAX_RENDER_RIBBONS],
        ..Default::default()
    };
    assert!(build(&primitives).len() <= MAX_VERTICES);
}

/// Explicit untapered strips retain width and use uniform solid coverage at both ends.
#[test]
fn untapered_ribbons_keep_constant_width_and_solid_style() {
    let vertices = build(&Primitives {
        ribbons: vec![Ribbon {
            taper: false,
            pattern: crate::LinePattern::Solid,
            points: vec![[0., 0., 0.], [1., 0., 0.], [1., 2., 0.]],
            width: 0.5,
            color: [0.2, 0.4, 0.6, 0.8],
        }],
        ..Default::default()
    });
    assert_eq!(vertices.len(), 12);
    for vertex in vertices {
        assert_eq!(vertex.shape[3], 0.25);
        assert_eq!(vertex.style[0], STYLE_UNTAPERED_RIBBON);
        assert_eq!(vertex.color, [0.2, 0.4, 0.6, 0.8]);
    }
}

/// Pattern phase follows distance rather than point index across unequal segments.
#[test]
fn line_patterns_keep_continuous_distance_and_distinct_styles() {
    for (pattern, style) in [
        (crate::LinePattern::Dashed, 31.),
        (crate::LinePattern::Dotted, 32.),
    ] {
        let vertices = build(&Primitives {
            ribbons: vec![Ribbon {
                taper: false,
                pattern,
                points: vec![[0., 0., 0.], [1., 0., 0.], [1., 3., 0.]],
                width: 1.,
                color: [1.; 4],
            }],
            ..Default::default()
        });
        assert_eq!(vertices.len(), 12);
        for vertex in &vertices {
            assert_eq!(vertex.style[0], style);
            assert_eq!(vertex.style[2], 4.);
            assert_eq!(vertex.shape[3], 0.5);
        }
        let joint = vertices
            .iter()
            .find(|vertex| vertex.anchor[..3] == [1., 0., 0.])
            .unwrap();
        assert_eq!(joint.uv[1], 0.25);
        assert_eq!(vertices.last().unwrap().uv[1], 1.);
    }
}
