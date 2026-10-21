//! Cinnabar paint transforms on painted draws. Coverage meshes (rounded rectangles and vector
//! art) regenerate at their scaled size so their antialiased fringe stays one physical pixel;
//! everything else, and any remaining rotation, moves its vertices through a node transform.

use json_ui::{Draw, transform::Affine};
use ui::{UiNode, UiTransform};

use super::Canvas;

/// Custom renderers whose antialiasing is baked into geometry built for one size.
const COVERAGE_RENDERERS: [&str; 4] = [
    json_ui::rounded::RENDERER,
    ui::mod_panel::backdrop::GLASS_RENDERER,
    "cinnabar_vector_icon",
    json_ui::vector_path::RENDERER,
];

/// How one transformed draw paints: at `dest`, with lengths times `length_scale`, then
/// moved by `residual` when that map is not the identity.
pub(super) struct Placement {
    pub(super) dest: [f32; 4],
    pub(super) length_scale: f32,
    pub(super) residual: Option<Affine>,
}

/// Splits `transform`, a map of layout coordinates, into what `draw` paints at logical `dest`
/// under `px` logical px per layout unit. `None` when the transform collapses the draw.
pub(super) fn place(draw: &Draw, transform: &Affine, dest: [f32; 4], px: f32) -> Option<Placement> {
    let px = f64::from(px);
    let logical = Affine {
        linear: transform.linear,
        translation: transform.translation.map(|value| value * px),
    };
    let baked = matches!(draw, Draw::Custom { renderer, .. }
        if COVERAGE_RENDERERS.contains(&renderer.as_str()));
    if !baked {
        return Some(Placement {
            dest,
            length_scale: 1.0,
            residual: moves(&logical).then_some(logical),
        });
    }
    let [a, b, c, d] = logical.linear;
    let scale = [a.hypot(b), c.hypot(d)];
    if scale.iter().any(|axis| *axis < 1e-6) {
        return None;
    }
    let residual = Affine {
        linear: [a / scale[0], b / scale[0], c / scale[1], d / scale[1]],
        translation: logical.translation,
    };
    let [sx, sy] = scale.map(|axis| axis as f32);
    Some(Placement {
        dest: [dest[0] * sx, dest[1] * sy, dest[2] * sx, dest[3] * sy],
        length_scale: (sx * sy).sqrt(),
        residual: moves(&residual).then_some(residual),
    })
}

/// Whether `transform` moves any point.
fn moves(transform: &Affine) -> bool {
    *transform != Affine::IDENTITY
}

/// Applies a canvas map after a renderer's node-local map, about the node's absolute origin.
fn composed(residual: Affine, origin: [f64; 2], previous: Option<&UiTransform>) -> UiTransform {
    let moved = residual.apply(origin);
    let outer = Affine {
        linear: residual.linear,
        translation: [moved[0] - origin[0], moved[1] - origin[1]],
    };
    let transform = previous.map_or(outer, |previous| {
        outer.then(&Affine {
            linear: previous.linear.map(f64::from),
            translation: previous.translation.map(f64::from),
        })
    });
    UiTransform {
        linear: transform.linear.map(|value| value as f32),
        translation: transform.translation.map(|value| value as f32),
    }
}

impl Canvas<'_> {
    /// Gives each leaf node painted since `start` the canvas's pending residual transform,
    /// expressed relative to that node's own logical top-left. Clip groups stay in place.
    pub(super) fn transform_painted(&mut self, start: usize) {
        let Some(residual) = self.residual.take() else {
            return;
        };
        for index in start..self.nodes.len() {
            let node = &self.nodes[index];
            let Some(parent) = node.parent() else {
                continue;
            };
            let Some(group) = self.nodes[..index]
                .iter()
                .rev()
                .find(|candidate| candidate.id() == parent)
                .map(UiNode::bounds)
            else {
                continue;
            };
            let bounds = node.bounds();
            let origin = [
                f64::from(group.min().x() + bounds.min().x()),
                f64::from(group.min().y() + bounds.min().y()),
            ];
            let transform = composed(residual, origin, node.transform());
            self.nodes[index] = self.nodes[index].clone().with_transform(transform);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn scaled(scale: f64, rotation: f64) -> Affine {
        let (sin, cos) = rotation.to_radians().sin_cos();
        Affine {
            linear: [cos * scale, sin * scale, -sin * scale, cos * scale],
            translation: [1.0, 2.0],
        }
    }

    #[test]
    fn coverage_meshes_bake_scale_and_keep_only_rotation() {
        let rounded = Draw::Custom {
            renderer: "cinnabar_rounded_rectangle".into(),
            data: BTreeMap::new(),
        };
        let placement = place(&rounded, &scaled(2.0, 0.0), [10.0, 10.0, 20.0, 30.0], 3.0).unwrap();
        assert_eq!(placement.dest, [20.0, 20.0, 40.0, 60.0]);
        assert_eq!(placement.length_scale, 2.0);
        let residual = placement.residual.unwrap();
        assert_eq!(residual.linear, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(residual.translation, [3.0, 6.0]);
        let turned = place(&rounded, &scaled(1.5, 90.0), [0.0; 4], 1.0).unwrap();
        let [a, b, c, d] = turned.residual.unwrap().linear;
        assert!(
            a.abs() < 1e-9 && (b - 1.0).abs() < 1e-9 && (c + 1.0).abs() < 1e-9 && d.abs() < 1e-9
        );
        let collapsed = Affine {
            linear: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0; 2],
        };
        assert!(place(&rounded, &collapsed, [0.0; 4], 1.0).is_none());
    }

    #[test]
    fn other_draws_move_vertices_by_the_whole_transform() {
        let solid = Draw::Solid { color: [255; 4] };
        let placement = place(&solid, &scaled(2.0, 0.0), [1.0, 1.0, 2.0, 2.0], 2.0).unwrap();
        assert_eq!(placement.dest, [1.0, 1.0, 2.0, 2.0]);
        assert_eq!(placement.residual.unwrap().linear, [2.0, 0.0, 0.0, 2.0]);
        assert!(
            place(&solid, &Affine::IDENTITY, [0.0; 4], 2.0)
                .unwrap()
                .residual
                .is_none()
        );
    }

    #[test]
    fn outer_canvas_motion_composes_with_an_existing_fragment_transform() {
        let outer = scaled(2.0, 90.0);
        let previous = UiTransform {
            linear: [0.5, 0.0, 0.0, 0.5],
            translation: [3.0, 4.0],
        };
        let origin = [20.0, 30.0];
        let combined = composed(outer, origin, Some(&previous));
        let point = [5.0, 7.0];
        let expected = outer.apply([
            origin[0] + point[0] * 0.5 + 3.0,
            origin[1] + point[1] * 0.5 + 4.0,
        ]);
        let actual = Affine {
            linear: combined.linear.map(f64::from),
            translation: combined.translation.map(f64::from),
        }
        .apply(point);
        for axis in 0..2 {
            assert!((origin[axis] + actual[axis] - expected[axis]).abs() < 1e-5);
        }
    }
}
