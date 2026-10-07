//! Cached coverage meshes keep small icon strokes continuous at physical-pixel scale.
use super::Painter;
use std::{cell::RefCell, collections::HashMap, sync::Arc};
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiVisual};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Icon {
    Pointer,
    Crosshair,
    Ruler,
    Settings,
    Brand,
    Sword,
    Close,
    Chevron,
}
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key {
    icon: Icon,
    width: u32,
    height: u32,
    page: u16,
    color: [u8; 4],
}
thread_local! { static CACHE: RefCell<HashMap<Key, Arc<UiMesh>>> = RefCell::default(); }

impl Painter<'_> {
    pub(super) fn vector_icon(
        &self,
        data: &std::collections::BTreeMap<String, serde_json::Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<UiVisual> {
        let icon = match data.get("icon")?.as_str()? {
            "pointer" => Icon::Pointer,
            "crosshair" => Icon::Crosshair,
            "ruler" => Icon::Ruler,
            "settings" => Icon::Settings,
            "brand" => Icon::Brand,
            "sword" => Icon::Sword,
            "close" => Icon::Close,
            "chevron" => Icon::Chevron,
            _ => return None,
        };
        let width = dest[2] - dest[0];
        let height = dest[3] - dest[1];
        if width <= 0. || height <= 0. {
            return None;
        }
        let color = data
            .get("color")
            .and_then(json_ui::color_value)
            .unwrap_or([255; 4]);
        let key = Key {
            icon,
            width: width.to_bits(),
            height: height.to_bits(),
            page: self.solid_page,
            color: alpha(color),
        };
        CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(mesh) = cache.get(&key) {
                return Some(UiVisual::Mesh(Arc::clone(mesh)));
            }
            if cache.len() == 128 {
                cache.clear();
            }
            let mesh = Arc::new(mesh(key)?);
            cache.insert(key, Arc::clone(&mesh));
            Some(UiVisual::Mesh(mesh))
        })
    }
}

struct Shape {
    lines: Vec<([f32; 2], [f32; 2])>,
    rings: Vec<([f32; 2], f32)>,
    weight: f32,
}
impl Shape {
    fn path(&mut self, points: &[[f32; 2]]) {
        self.lines.extend(points.windows(2).map(|p| (p[0], p[1])));
    }
    fn new(icon: Icon) -> Self {
        let mut shape = Self {
            lines: Vec::new(),
            rings: Vec::new(),
            weight: 1.4,
        };
        match icon {
            Icon::Pointer => shape.path(&[
                [3., 2.],
                [5., 14.],
                [7.5, 10.],
                [10., 14.],
                [12., 13.],
                [9.5, 9.],
                [14., 8.],
                [3., 2.],
            ]),
            Icon::Crosshair => {
                shape.rings.push(([8., 8.], 4.5));
                for points in [
                    [[8., 1.], [8., 5.]],
                    [[8., 11.], [8., 15.]],
                    [[1., 8.], [5., 8.]],
                    [[11., 8.], [15., 8.]],
                ] {
                    shape.path(&points);
                }
            }
            Icon::Ruler => {
                shape.path(&[[2., 11.], [11., 2.], [14., 5.], [5., 14.], [2., 11.]]);
                for points in [
                    [[5., 8.], [7., 10.]],
                    [[8., 5.], [10., 7.]],
                    [[11., 2.], [13., 4.]],
                ] {
                    shape.path(&points);
                }
            }
            Icon::Settings => {
                shape.rings.push(([8., 8.], 2.));
                let points: Vec<_> = (0..=32)
                    .map(|n| {
                        let angle = n as f32 * std::f32::consts::TAU / 32.;
                        let r = if n % 4 == 1 || n % 4 == 2 { 6.3 } else { 5. };
                        [8. + r * angle.cos(), 8. + r * angle.sin()]
                    })
                    .collect();
                shape.path(&points);
            }
            Icon::Brand => {
                shape.weight = 2.6;
                shape.path(&[
                    [13., 4.],
                    [8., 1.5],
                    [2.5, 4.5],
                    [2.5, 11.5],
                    [8., 14.5],
                    [13., 12.],
                ]);
            }
            Icon::Sword => {
                shape.path(&[[2.5, 13.5], [5.5, 10.5]]);
                shape.path(&[[2.5, 8.], [8., 13.5]]);
                shape.path(&[[5., 9.], [10.5, 3.5], [14., 2.], [12.5, 5.5], [7., 11.]]);
            }
            Icon::Close => {
                shape.path(&[[2., 2.], [14., 14.]]);
                shape.path(&[[14., 2.], [2., 14.]]);
            }
            Icon::Chevron => {
                shape.weight = 3.;
                shape.path(&[[2., 4.], [8., 12.], [14., 4.]]);
            }
        }
        shape
    }
    fn coverage(&self, point: [f32; 2], size: [f32; 2]) -> f32 {
        let scale = size.map(|v| v / 16.);
        let physical = |p: [f32; 2]| [p[0] * scale[0], p[1] * scale[1]];
        let mut distance = f32::INFINITY;
        for &(a, b) in &self.lines {
            let (a, b) = (physical(a), physical(b));
            let delta = [b[0] - a[0], b[1] - a[1]];
            let t = (((point[0] - a[0]) * delta[0] + (point[1] - a[1]) * delta[1])
                / (delta[0] * delta[0] + delta[1] * delta[1]))
                .clamp(0., 1.);
            distance = distance
                .min((point[0] - a[0] - t * delta[0]).hypot(point[1] - a[1] - t * delta[1]));
        }
        for &(center, radius) in &self.rings {
            let center = physical(center);
            distance = distance.min(
                ((point[0] - center[0]).hypot(point[1] - center[1])
                    - radius * scale[0].min(scale[1]))
                .abs(),
            );
        }
        (0.5 + self.weight * scale[0].min(scale[1]) * 0.5 - distance).clamp(0., 1.)
    }
}

fn mesh(key: Key) -> Option<UiMesh> {
    let size = [f32::from_bits(key.width), f32::from_bits(key.height)];
    let cells = size.map(|v| (v * 2.).ceil().clamp(1., 128.) as usize);
    let shape = Shape::new(key.icon);
    let mut vertices = Vec::with_capacity((cells[0] + 1) * (cells[1] + 1));
    for y in 0..=cells[1] {
        for x in 0..=cells[0] {
            let position = [x as f32 / cells[0] as f32, y as f32 / cells[1] as f32];
            let coverage = shape.coverage([position[0] * size[0], position[1] * size[1]], size);
            let mut color = key.color;
            color[3] = (f32::from(color[3]) * coverage).round() as u8;
            vertices.push(UiMeshVertex {
                position,
                clip_z: 0.,
                clip_w: 1.,
                uv: [0.; 2],
                color,
                model_light: 1.,
                overlay_color: [0.; 4],
                style_flags: 0,
                alpha_test: false,
            });
        }
    }
    let mut indices = Vec::new();
    for y in 0..cells[1] {
        for x in 0..cells[0] {
            let a = (y * (cells[0] + 1) + x) as u32;
            let corners = [a, a + 1, a + cells[0] as u32 + 1, a + cells[0] as u32 + 2];
            if corners.iter().all(|&i| vertices[i as usize].color[3] == 0) {
                continue;
            }
            indices.extend([
                corners[0], corners[1], corners[3], corners[0], corners[3], corners[2],
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
    #[test]
    fn thin_strokes_keep_continuous_coverage_at_each_display_scale() {
        let shape = Shape::new(Icon::Close);
        for size in [12., 16., 24., 32.] {
            for step in 0..=48 {
                let point = (2. + 12. * step as f32 / 48.) * size / 16.;
                assert!(shape.coverage([point; 2], [size; 2]) > 0.9);
            }
        }
    }
    #[test]
    fn gear_keeps_a_clear_center_and_antialiased_edges() {
        let shape = Shape::new(Icon::Settings);
        assert_eq!(shape.coverage([16.; 2], [32.; 2]), 0.);
        assert!(shape.coverage([20., 16.], [32.; 2]) > 0.9);
        let key = Key {
            icon: Icon::Settings,
            width: 32_f32.to_bits(),
            height: 32_f32.to_bits(),
            page: 0,
            color: [255; 4],
        };
        assert!(
            mesh(key)
                .unwrap()
                .vertices()
                .iter()
                .any(|v| v.color[3] > 0 && v.color[3] < 255)
        );
    }
}
