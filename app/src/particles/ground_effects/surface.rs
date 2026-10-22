//! Flat support for decals, including adjacent coplanar block shapes.
use particles::ParticleWorld;

/// Finds a nearby floor supporting the whole decal square with clear space above it.
pub(super) fn height(world: &dyn ParticleWorld, point: [f32; 3], extent: f32) -> Option<f32> {
    let [x, y, z] = point;
    let mut boxes = Vec::new();
    world.solid_boxes(
        [x - extent, y - 0.2, z - extent],
        [x + extent, y + 0.15, z + extent],
        &mut boxes,
    );
    let top = boxes
        .iter()
        .filter(|b| x >= b[0] && x <= b[3] && z >= b[2] && z <= b[5] && (b[4] - y).abs() < 0.15)
        .map(|b| b[4])
        .max_by(f32::total_cmp)?;
    let low = [x - extent, z - extent];
    let high = [x + extent, z + extent];
    let intersects =
        |b: &&[f32; 6]| b[0] < high[0] && b[3] > low[0] && b[2] < high[1] && b[5] > low[1];
    if boxes
        .iter()
        .filter(intersects)
        .any(|b| b[1] < top + 0.012 && b[4] > top + 0.002)
    {
        return None;
    }
    let floor: Vec<_> = boxes
        .iter()
        .filter(intersects)
        .filter(|b| (b[4] - top).abs() < 0.001)
        .collect();
    let mut xs = vec![low[0], high[0]];
    for b in &floor {
        xs.extend([b[0].clamp(low[0], high[0]), b[3].clamp(low[0], high[0])]);
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    for strip in xs.windows(2) {
        let x = (strip[0] + strip[1]) * 0.5;
        let mut zs: Vec<_> = floor
            .iter()
            .filter(|b| b[0] <= x && b[3] >= x)
            .map(|b| [b[2], b[5]])
            .collect();
        zs.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut edge = low[1];
        for span in zs {
            if span[0] > edge + 0.001 {
                break;
            }
            edge = edge.max(span[1]);
        }
        if edge < high[1] - 0.001 {
            return None;
        }
    }
    Some(top)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Shapes(Vec<[f32; 6]>);
    impl ParticleWorld for Shapes {
        fn solid_boxes(&self, _min: [f32; 3], _max: [f32; 3], out: &mut Vec<[f32; 6]>) {
            out.extend_from_slice(&self.0);
        }
        fn light(&self, _: [i32; 3]) -> (u8, u8) {
            (0, 15)
        }
        fn fluid(&self, _: [i32; 3]) -> particles::Fluid {
            particles::Fluid::None
        }
    }
    #[test]
    fn adjacent_coplanar_shapes_support_a_stamp_but_a_gap_or_step_does_not() {
        let mut world = Shapes(vec![
            [-1.0, -1.0, -1.0, 0.0, 0.0, 1.0],
            [0.0, -1.0, -1.0, 1.0, 0.0, 1.0],
        ]);
        assert_eq!(height(&world, [0.0; 3], 0.5), Some(0.0));
        world.0[1][0] = 0.1;
        assert_eq!(height(&world, [0.0; 3], 0.5), None);
        world.0[1][0] = 0.0;
        world.0[1][4] = 0.1;
        assert_eq!(height(&world, [-0.1, 0.0, 0.0], 0.5), None);
    }
    #[test]
    fn slabs_work_and_decals_do_not_float_past_edges_or_cut_into_walls() {
        let mut world = Shapes(vec![[-1.0, -1.0, -1.0, 1.0, -0.5, 1.0]]);
        assert_eq!(height(&world, [0.0, -0.5, 0.0], 0.5), Some(-0.5));
        assert_eq!(height(&world, [0.75, -0.5, 0.0], 0.5), None);
        world.0.push([0.3, -0.5, -1.0, 1.0, 1.0, 1.0]);
        assert_eq!(height(&world, [0.0, -0.5, 0.0], 0.5), None);
    }
}
