//! Bounds over retained vertex runs; camera turns change only which runs are submitted.

use super::BlockEntityVertex;
use crate::visibility_bounds::box_intersects_view;
use glam::Mat4;
use std::ops::Range;

/// Thirty-two quads per run, preserving triangle boundaries and bounding draw-call growth.
const VERTICES_PER_RUN: usize = 32 * 6;

#[derive(Clone, Debug, PartialEq)]
pub struct DrawRun {
    pub vertices: Range<u32>,
    pub low: [f32; 3],
    pub high: [f32; 3],
}

/// Recomputes bounds only when the retained vertex stream changes.
pub fn rebuild_runs(runs: &mut Vec<DrawRun>, vertices: &[BlockEntityVertex]) {
    runs.clear();
    for (index, vertices) in vertices.chunks(VERTICES_PER_RUN).enumerate() {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for vertex in vertices {
            for axis in 0..3 {
                // Overlay shaders can lift either side of a face toward the camera.
                let padding = if vertex.actor_light == 0 {
                    vertex.normal[axis].abs() * render_api::BLOCK_OVERLAY_FACE_OFFSET
                } else {
                    0.0
                };
                if !vertex.position[axis].is_finite() || !padding.is_finite() {
                    low[axis] = f32::NAN;
                    high[axis] = f32::NAN;
                } else if !low[axis].is_nan() {
                    low[axis] = low[axis].min(vertex.position[axis] - padding);
                    high[axis] = high[axis].max(vertex.position[axis] + padding);
                }
            }
        }
        let start = (index * VERTICES_PER_RUN) as u32;
        runs.push(DrawRun {
            vertices: start..start + vertices.len() as u32,
            low,
            high,
        });
    }
}

/// Adjacent visible runs are merged, so a fully visible stream still makes one draw call.
pub fn visible_ranges(runs: &[DrawRun], matrix: Mat4) -> impl Iterator<Item = Range<u32>> + '_ {
    let mut next = 0;
    std::iter::from_fn(move || {
        while next < runs.len() && !box_intersects_view(matrix, runs[next].low, runs[next].high) {
            next += 1;
        }
        let first = runs.get(next)?;
        let mut range = first.vertices.clone();
        next += 1;
        while let Some(run) = runs.get(next) {
            if run.vertices.start != range.end || !box_intersects_view(matrix, run.low, run.high) {
                break;
            }
            range.end = run.vertices.end;
            next += 1;
        }
        Some(range)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_lift_can_reach_the_view_even_when_the_authored_plane_is_outside() {
        let mut vertex = BlockEntityVertex {
            position: [1.0 + render_api::BLOCK_OVERLAY_FACE_OFFSET * 0.5, 0.0, 0.5],
            normal: [1.0, 0.0, 0.0],
            ..Default::default()
        };
        let mut runs = Vec::new();
        rebuild_runs(&mut runs, &[vertex]);
        assert_eq!(visible_ranges(&runs, Mat4::IDENTITY).count(), 1);
        vertex.normal = [f32::NAN; 3];
        rebuild_runs(&mut runs, &[vertex]);
        assert_eq!(visible_ranges(&runs, Mat4::IDENTITY).count(), 1);
    }

    #[test]
    fn rotations_cull_retained_runs_without_rebuilding_vertices() {
        let vertices = vec![
            BlockEntityVertex {
                position: [0.0, 0.0, -10.0],
                ..Default::default()
            };
            VERTICES_PER_RUN * 2
        ];
        let mut runs = Vec::new();
        rebuild_runs(&mut runs, &vertices);
        let retained = runs.clone();
        let projection =
            glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, 0.1);
        assert_eq!(
            visible_ranges(&runs, projection).collect::<Vec<_>>(),
            std::iter::once(0..vertices.len() as u32).collect::<Vec<_>>()
        );
        assert_eq!(
            visible_ranges(
                &runs,
                projection * Mat4::from_rotation_y(std::f32::consts::PI)
            )
            .count(),
            0
        );
        assert_eq!(runs, retained);
        assert_eq!(visible_ranges(&runs, Mat4::ZERO).count(), 1);
        assert_eq!(
            visible_ranges(&runs, Mat4::from_cols_array(&[f32::NAN; 16])).count(),
            1
        );
    }

    #[test]
    fn visible_runs_do_not_bridge_an_invisible_gap() {
        let run = |start, x| DrawRun {
            vertices: start..start + 6,
            low: [x, 0.0, 0.5],
            high: [x, 0.0, 0.5],
        };
        let runs = [run(0, 0.0), run(6, 4.0), run(12, 0.0)];
        assert_eq!(
            visible_ranges(&runs, Mat4::IDENTITY).collect::<Vec<_>>(),
            vec![0..6, 12..18]
        );
    }
}
