use super::*;

const EYE: [f32; 3] = [0.5, 4.5, 0.5];
const LOW: [f32; 3] = [10.0, 3.0, -1.0];
const HIGH: [f32; 3] = [11.0, 6.0, 2.0];

#[test]
fn a_wall_inside_an_open_section_hides_the_whole_box() {
    let mut budget = OcclusionBudget::default();
    assert!(budget.hides_box(EYE, LOW, HIGH, |cell, _| cell[0] == 5));
    assert!(budget.remaining() > ENTITY_OCCLUSION_PROBES - 64);
}

#[test]
fn clear_sight_and_a_hole_between_corner_rays_draw() {
    for hole in [false, true] {
        assert!(
            !OcclusionBudget::default().hides_box(EYE, LOW, HIGH, |cell, _| {
                hole && cell[0] == 5 && cell != [5, 4, 0]
            })
        );
    }
}

#[test]
fn uncertain_and_exhausted_queries_draw() {
    assert!(!OcclusionBudget::new(1).hides_box(EYE, LOW, HIGH, |_, _| true));
    assert!(!OcclusionBudget::default().hides_box([f32::NAN; 3], LOW, HIGH, |_, _| true));
    assert!(!OcclusionBudget::default().hides_box(EYE, HIGH, LOW, |_, _| true));
    // The next frame checks current terrain rather than retaining the old hidden result.
    assert!(OcclusionBudget::default().hides_box(EYE, LOW, HIGH, |p, _| p[0] == 5));
    assert!(!OcclusionBudget::default().hides_box(EYE, LOW, HIGH, |_, _| false));
}

#[test]
fn proof_covers_both_directions_on_each_axis() {
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let rotate = |p: [f32; 3]| {
                let mut out = [0.0; 3];
                for i in 0..3 {
                    out[(i + axis) % 3] = p[i] * sign;
                }
                out
            };
            let a = rotate(LOW);
            let b = rotate(HIGH);
            let low = std::array::from_fn(|i| a[i].min(b[i]));
            let high = std::array::from_fn(|i| a[i].max(b[i]));
            let wall = if sign > 0.0 { 5 } else { -6 };
            assert!(
                OcclusionBudget::default()
                    .hides_box(rotate(EYE), low, high, |p, _| p[axis] == wall)
            );
        }
    }
}

#[test]
fn a_frame_never_exceeds_its_plane_and_cell_budget() {
    let mut budget = OcclusionBudget::new(31);
    let mut calls = 0;
    for _ in 0..256 {
        assert!(!budget.hides_box(EYE, LOW, HIGH, |_, _| {
            calls += 1;
            false
        }));
    }
    assert_eq!(budget.remaining(), 0);
    assert!(calls <= 31);
}
