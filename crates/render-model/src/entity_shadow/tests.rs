use super::*;

fn shadow(radius: f32) -> EntityShadow {
    EntityShadow {
        feet: [10.5, 64.0, -3.5],
        radius,
    }
}

/// The surface right under the feet is shaded; the volume ends just above them.
#[test]
fn volume_spans_from_just_above_the_feet_to_three_radii_below() {
    let caster = shadow(0.6);
    assert!(caster.contains([10.5, 64.0, -3.5]));
    assert!(caster.contains([10.5, 64.0 + 0.005, -3.5]));
    assert!(!caster.contains([10.5, 64.0 + 0.007, -3.5]));
    assert!(caster.contains([10.5, 64.0 - 1.79, -3.5]));
    assert!(!caster.contains([10.5, 64.0 - 1.81, -3.5]));
}

/// The footprint narrows linearly from 0.75 radii at the feet to 0.25 at the bottom.
#[test]
fn footprint_narrows_with_depth_below_the_feet() {
    let caster = shadow(1.0);
    let apothem = (std::f32::consts::PI / 13.0).cos();
    for depth in [0.0, 1.5, 2.9] {
        let ring = 0.25 + 0.5 * (3.0 - depth) / 3.01;
        let y = 64.0 - depth;
        // Along a face normal the edge sits at the apothem; at a vertex, at the ring radius.
        let (sine, cosine) = (std::f32::consts::PI / 13.0).sin_cos();
        let inside = ring * apothem - 0.01;
        let outside = ring * apothem + 0.01;
        assert!(caster.contains([10.5 + inside * cosine, y, -3.5 - inside * sine]));
        assert!(!caster.contains([10.5 + outside * cosine, y, -3.5 - outside * sine]));
        assert!(caster.contains([10.5 + ring - 0.01, y, -3.5]));
        assert!(!caster.contains([10.5 + ring + 0.01, y, -3.5]));
    }
}

/// A slab top half a block below feet on a block edge is shaded only within the footprint.
#[test]
fn shading_follows_partial_blocks_and_edges() {
    let caster = EntityShadow {
        feet: [0.95, 65.0, 0.5],
        radius: 0.6,
    };
    // Ledge top under the feet, and the drop's vertical face just past the edge.
    assert!(caster.contains([0.9, 65.0, 0.5]));
    assert!(caster.contains([1.0, 64.8, 0.5]));
    // A slab top half a block down.
    let ring = 0.6 * (0.25 + 0.5 * (3.0 - 0.5 / 0.6) / 3.01);
    assert!(caster.contains([0.95 + ring - 0.02, 64.5, 0.5]));
    assert!(!caster.contains([0.95 + ring + 0.02, 64.5, 0.5]));
}

#[test]
fn zero_radius_casts_nothing() {
    assert!(!shadow(0.0).contains([10.5, 64.0, -3.5]));
    assert!(!shadow(f32::NAN).contains([10.5, 64.0, -3.5]));
}

#[test]
fn mesh_is_closed_and_wound_outward() {
    let mesh = shadow_volume_mesh();
    let centre = [
        0.0,
        (SHADOW_VOLUME_TOP_Y + SHADOW_VOLUME_BOTTOM_Y) * 0.5,
        0.0,
    ];
    for triangle in mesh.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        let e1 = glam::Vec3::from(b) - glam::Vec3::from(a);
        let e2 = glam::Vec3::from(c) - glam::Vec3::from(a);
        let normal = e1.cross(e2);
        let middle = (glam::Vec3::from(a) + glam::Vec3::from(b) + glam::Vec3::from(c)) / 3.0;
        assert!(normal.length() > 1.0e-4, "degenerate triangle");
        assert!(
            normal.dot(middle - glam::Vec3::from(centre)) > 0.0,
            "inward face"
        );
    }
    // Every edge is shared by exactly two triangles in opposite directions.
    let key = |p: [f32; 3]| p.map(|v| (v * 1.0e4).round() as i64);
    let mut edges = std::collections::HashMap::new();
    for triangle in mesh.chunks_exact(3) {
        for (from, to) in [(0, 1), (1, 2), (2, 0)] {
            *edges
                .entry((key(triangle[from]), key(triangle[to])))
                .or_insert(0) += 1;
        }
    }
    for ((from, to), count) in &edges {
        assert_eq!(*count, 1);
        assert_eq!(edges.get(&(*to, *from)), Some(&1));
    }
}

#[test]
fn mesh_vertices_lie_on_the_volume_surface() {
    for corner in shadow_volume_mesh() {
        let scaled = |factor: f32| [corner[0] * factor, corner[1], corner[2] * factor];
        assert!(unit_volume_contains(scaled(0.999)));
        assert!(!unit_volume_contains(scaled(1.01)));
    }
}

#[test]
fn neutral_sky_gives_the_plain_grey() {
    assert_eq!(
        entity_shadow_colour([0.5; 3], [0.0; 4]),
        [0.7, 0.7, 0.7, 1.0]
    );
}

/// The largest deviation from luminance maps to exactly 0.03 and the others scale with it.
#[test]
fn sky_hue_tints_the_grey_by_at_most_three_hundredths() {
    let sky = [0.47, 0.65, 1.0];
    let [r, g, b, a] = entity_shadow_colour(sky, [0.0; 4]);
    assert_eq!(a, 1.0);
    let tint = sky.map(|value| value * 0.5 + 0.4);
    let luminance = 0.2126 * tint[0] + 0.7152 * tint[1] + 0.0722 * tint[2];
    let largest = tint
        .map(|value| (value - luminance).abs())
        .into_iter()
        .fold(0.0, f32::max);
    assert!((b - 0.7 - 0.03 * (tint[2] - luminance) / largest).abs() < 1.0e-6);
    assert!((b - 0.73).abs() < 1.0e-6 || (r - 0.67).abs() < 1.0e-6);
    for channel in [r, g, b] {
        assert!((0.67 - 1.0e-6..=0.73 + 1.0e-6).contains(&channel));
    }
}

#[test]
fn opaque_sunrise_replaces_the_sky_tint() {
    let warm = entity_shadow_colour([0.2, 0.3, 1.0], [1.0, 0.6, 0.2, 1.0]);
    let same = entity_shadow_colour([1.2, 0.4, -0.4], [0.0; 4]);
    assert!(warm.iter().zip(same).all(|(a, b)| (a - b).abs() < 1.0e-5));
    assert!(warm[0] > 0.7 && warm[2] < 0.7);
}

#[test]
fn screen_rect_bounds_visible_volumes_and_covers_the_viewport_at_the_camera() {
    let clip_from_view = glam::Mat4::perspective_infinite_reverse_rh(1.2, 1.0, 0.05);
    let view_from_world = glam::Mat4::look_at_rh(
        glam::Vec3::new(0.0, 70.0, 10.0),
        glam::Vec3::new(0.0, 64.0, 0.0),
        glam::Vec3::Y,
    );
    let clip_from_world = clip_from_view * view_from_world;
    let viewport = [0, 0, 640, 640];
    let centre = EntityShadow {
        feet: [0.0, 64.0, 0.0],
        radius: 0.5,
    };
    let rect = shadow_screen_rect(clip_from_world, &[centre], viewport).unwrap();
    assert!(rect[0] < 320 && rect[2] > 320 && rect[1] < 320 && rect[3] > 320);
    assert!(rect[2] - rect[0] < 200);
    let behind = EntityShadow {
        feet: [0.0, 64.0, 40.0],
        radius: 0.5,
    };
    assert_eq!(
        shadow_screen_rect(clip_from_world, &[behind], viewport),
        None
    );
    let at_camera = EntityShadow {
        feet: [0.0, 70.5, 10.0],
        radius: 0.6,
    };
    assert_eq!(
        shadow_screen_rect(clip_from_world, &[at_camera], viewport),
        Some([0, 0, 640, 640])
    );
    assert_eq!(shadow_screen_rect(clip_from_world, &[], viewport), None);
}

#[test]
fn frame_revision_moves_only_when_the_casters_change() {
    let mut frame = EntityShadowFrame::default();
    assert!(!frame.publish(&[]));
    assert_eq!(frame.revision, 0);
    let casters = [shadow(0.6), shadow(0.3)];
    assert!(frame.publish(&casters));
    let published = Arc::clone(&frame.shadows);
    assert!(!frame.publish(&casters));
    assert_eq!(frame.revision, 1);
    assert!(Arc::ptr_eq(&published, &frame.shadows));
    assert!(frame.publish(&casters[..1]));
    assert_eq!(frame.revision, 2);
}
