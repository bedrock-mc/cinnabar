//! Cached antialiased geometry for host-owned rounded JSON-UI chrome.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiVisual};

use super::Canvas;

const MAX_CACHED_SHAPES: usize = 128;
const CORNER_STEPS: usize = 8;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key {
    width: u32,
    height: u32,
    radius: u32,
    fringe: u32,
    page: u16,
    color: [u8; 4],
    /// Color at the far edge of an optional linear gradient.
    end_color: [u8; 4],
    horizontal: bool,
    /// Width of the optional inside border; zero draws no border.
    border: u32,
    border_color: [u8; 4],
}

thread_local! { static SHAPES: RefCell<HashMap<Key, Arc<UiMesh>>> = RefCell::default(); }

impl Canvas<'_> {
    /// Draws an antialiased rounded rectangle. `color2` with `gradient_direction`
    /// (`vertical` by default, or `horizontal`) fades the fill linearly from `color`;
    /// `border_width` and `border_color` add an inside border that keeps translucent
    /// fills from blending with it.
    pub(super) fn rounded_rectangle(
        &self,
        data: &std::collections::BTreeMap<String, serde_json::Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<UiVisual> {
        let width = dest[2] - dest[0];
        let height = dest[3] - dest[1];
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let length = |name: &str, default: f64| {
            data.get(name)
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or(default) as f32
                * self.px
        };
        let color = |name: &str| {
            data.get(&format!("#{name}"))
                .or_else(|| data.get(name))
                .and_then(json_ui::color_value)
        };
        let radius = length("radius", 6.0).clamp(0.0, width.min(height) * 0.5);
        let fill = color("color").unwrap_or([255; 4]);
        let border_color = color("border_color");
        let border = if border_color.is_some() {
            length("border_width", 1.0).clamp(0.0, width.min(height) * 0.5)
        } else {
            0.0
        };
        let key = Key {
            width: width.to_bits(),
            height: height.to_bits(),
            radius: radius.to_bits(),
            fringe: (self.px * 0.5).min(1.0).to_bits(),
            page: self.solid_page,
            color: alpha(fill),
            end_color: alpha(color("color2").unwrap_or(fill)),
            horizontal: data
                .get("gradient_direction")
                .and_then(serde_json::Value::as_str)
                == Some("horizontal"),
            border: border.to_bits(),
            border_color: alpha(border_color.unwrap_or([0; 4])),
        };
        SHAPES.with(|shapes| {
            let mut shapes = shapes.borrow_mut();
            if let Some(mesh) = shapes.get(&key) {
                return Some(UiVisual::Mesh(Arc::clone(mesh)));
            }
            if shapes.len() == MAX_CACHED_SHAPES {
                shapes.clear();
            }
            let mesh = Arc::new(mesh(key)?);
            shapes.insert(key, Arc::clone(&mesh));
            Some(UiVisual::Mesh(mesh))
        })
    }
}

/// Linearly interpolates two straight-alpha colors.
fn mix(from: [u8; 4], to: [u8; 4], amount: f32) -> [u8; 4] {
    std::array::from_fn(|channel| {
        (f32::from(from[channel]) + (f32::from(to[channel]) - f32::from(from[channel])) * amount)
            .round() as u8
    })
}

fn mesh(key: Key) -> Option<UiMesh> {
    let [width, height, radius, fringe, border] =
        [key.width, key.height, key.radius, key.fringe, key.border].map(f32::from_bits);
    let mut fringe = fringe.min(width.min(height) * 0.25);
    if border > 0.0 {
        // Keep at least half of a hairline border fully opaque.
        fringe = fringe.min(border * 0.5);
    }
    let inner_radius = (radius - fringe).max(0.0);
    let centers = [
        [radius, radius],
        [width - radius, radius],
        [width - radius, height - radius],
        [radius, height - radius],
    ];
    // The fill inside a border follows the inset outline; its corners stay concentric
    // with the outer corners whenever the radius exceeds the border width.
    let fill_radius = (radius - border).max(0.0);
    let fill_centers = [
        [border + fill_radius, border + fill_radius],
        [width - border - fill_radius, border + fill_radius],
        [width - border - fill_radius, height - border - fill_radius],
        [border + fill_radius, height - border - fill_radius],
    ];
    let fill = |point: [f32; 2]| {
        let amount = if key.horizontal {
            point[0] / width
        } else {
            point[1] / height
        };
        mix(key.color, key.end_color, amount.clamp(0.0, 1.0))
    };
    let vertex = |point: [f32; 2], color: [u8; 4]| UiMeshVertex {
        position: [point[0] / width, point[1] / height],
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [0.0; 2],
        color,
        model_light: 1.0,
        overlay_color: [0.0; 4],
        style_flags: 0,
        alpha_test: false,
    };
    let center = [width * 0.5, height * 0.5];
    let mut vertices = vec![vertex(center, fill(center))];
    // Each perimeter sample emits the solid edge, its transparent fringe and, with a
    // border, the fill edge and a coincident border-colored edge for a crisp seam.
    let stride: u32 = if border > 0.0 { 4 } else { 2 };
    for corner in 0..4 {
        let start = std::f32::consts::PI + corner as f32 * std::f32::consts::FRAC_PI_2;
        for step in 0..=CORNER_STEPS {
            let angle = start + step as f32 / CORNER_STEPS as f32 * std::f32::consts::FRAC_PI_2;
            let direction = [angle.cos(), angle.sin()];
            let around = |center: [f32; 2], distance: f32| {
                [
                    center[0] + direction[0] * distance,
                    center[1] + direction[1] * distance,
                ]
            };
            let solid = around(centers[corner], inner_radius);
            let outer = around(centers[corner], radius);
            if border > 0.0 {
                let seam = around(fill_centers[corner], fill_radius);
                let mut transparent = key.border_color;
                transparent[3] = 0;
                vertices.push(vertex(seam, fill(seam)));
                vertices.push(vertex(seam, key.border_color));
                vertices.push(vertex(solid, key.border_color));
                vertices.push(vertex(outer, transparent));
            } else {
                let mut transparent = fill(outer);
                transparent[3] = 0;
                vertices.push(vertex(solid, fill(solid)));
                vertices.push(vertex(outer, transparent));
            }
        }
    }
    let count = 4 * (CORNER_STEPS + 1);
    let mut indices = Vec::with_capacity(count * if border > 0.0 { 15 } else { 9 });
    for point in 0..count {
        let here = 1 + point as u32 * stride;
        let next = 1 + ((point + 1) % count) as u32 * stride;
        indices.extend([0, here, next]);
        // Quads between consecutive rings: fill to border seam, then border, then fringe.
        let rings = if border > 0.0 {
            [1, 2].as_slice()
        } else {
            [0].as_slice()
        };
        for &ring in rings {
            let (inner, inner_next) = (here + ring, next + ring);
            indices.extend([
                inner,
                inner + 1,
                inner_next + 1,
                inner,
                inner_next + 1,
                inner_next,
            ]);
        }
    }
    let end = indices.len() as u32;
    UiMesh::new(
        vertices.into(),
        indices.into(),
        Arc::from([UiMeshBatch {
            texture_page: key.page,
            index_range: 0..end,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(color: [u8; 4], end_color: [u8; 4], border: f32, border_color: [u8; 4]) -> Key {
        Key {
            width: 40f32.to_bits(),
            height: 20f32.to_bits(),
            radius: 6f32.to_bits(),
            fringe: 1f32.to_bits(),
            page: 0,
            color,
            end_color,
            horizontal: false,
            border: border.to_bits(),
            border_color,
        }
    }

    /// Opaque vertices as (pixel position, color).
    fn solid(mesh: &UiMesh) -> Vec<([f32; 2], [u8; 4])> {
        mesh.vertices()
            .iter()
            .filter(|vertex| vertex.color[3] != 0)
            .map(|vertex| {
                (
                    [vertex.position[0] * 40.0, vertex.position[1] * 20.0],
                    vertex.color,
                )
            })
            .collect()
    }

    #[test]
    fn plain_fill_keeps_one_color() {
        let red = [255, 0, 0, 255];
        let mesh = mesh_of(key(red, red, 0.0, [0; 4]));
        assert!(solid(&mesh).iter().all(|(_, color)| *color == red));
    }

    #[test]
    fn vertical_gradient_runs_from_top_to_bottom() {
        let (top, bottom) = ([0, 0, 0, 255], [200, 100, 0, 255]);
        let mesh = mesh_of(key(top, bottom, 0.0, [0; 4]));
        let vertices = solid(&mesh);
        let (_, upper) = vertices
            .iter()
            .min_by(|a, b| a.0[1].total_cmp(&b.0[1]))
            .unwrap();
        let (_, lower) = vertices
            .iter()
            .max_by(|a, b| a.0[1].total_cmp(&b.0[1]))
            .unwrap();
        assert!(upper[0] < 20 && lower[0] > 180, "{upper:?} {lower:?}");
        let center = vertices.iter().find(|(at, _)| *at == [20.0, 10.0]).unwrap();
        assert_eq!(center.1, [100, 50, 0, 255]);
        let mut horizontal = key(top, bottom, 0.0, [0; 4]);
        horizontal.horizontal = true;
        let vertices = solid(&mesh_of(horizontal));
        let (_, left) = vertices
            .iter()
            .min_by(|a, b| a.0[0].total_cmp(&b.0[0]))
            .unwrap();
        assert!(left[0] < 10);
    }

    fn mesh_of(key: Key) -> UiMesh {
        mesh(key).unwrap()
    }

    #[test]
    fn border_occupies_the_edge_and_the_fill_stays_inside_it() {
        let (fill, border) = ([0, 0, 255, 64], [255, 255, 255, 255]);
        let mesh = mesh_of(key(fill, fill, 2.0, border));
        let vertices = solid(&mesh);
        for (at, color) in &vertices {
            if *color == fill {
                assert!(
                    at[0] >= 2.0 - 1e-4
                        && at[0] <= 38.0 + 1e-4
                        && at[1] >= 2.0 - 1e-4
                        && at[1] <= 18.0 + 1e-4,
                    "fill vertex {at:?} lies under the border"
                );
            }
        }
        assert!(
            vertices
                .iter()
                .any(|(at, color)| *color == border && at[1] <= 1.0)
        );
        assert!(
            mesh.vertices()
                .iter()
                .any(|vertex| vertex.color == [255, 255, 255, 0]),
            "the border's outer edge is antialiased"
        );
        let square = Key {
            radius: 0f32.to_bits(),
            ..key(fill, fill, 1.0, border)
        };
        assert!(
            solid(&mesh_of(square))
                .iter()
                .filter(|(_, color)| *color == fill)
                .all(|(at, _)| at[0] >= 1.0 - 1e-4 && at[1] >= 1.0 - 1e-4)
        );
    }
}
