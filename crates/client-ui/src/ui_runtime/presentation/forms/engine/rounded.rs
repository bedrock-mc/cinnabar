//! Cached antialiased geometry for host-owned rounded JSON-UI chrome.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiVisual};

use super::Painter;

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
}

thread_local! { static SHAPES: RefCell<HashMap<Key, Arc<UiMesh>>> = RefCell::default(); }

impl Painter<'_> {
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
        let radius = data
            .get("radius")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(6.0) as f32
            * self.px;
        let radius = radius.clamp(0.0, width.min(height) * 0.5);
        let color = data
            .get("#color")
            .or_else(|| data.get("color"))
            .and_then(json_ui::color_value)
            .unwrap_or([255; 4]);
        let key = Key {
            width: width.to_bits(),
            height: height.to_bits(),
            radius: radius.to_bits(),
            fringe: (self.px * 0.5).min(1.0).to_bits(),
            page: self.solid_page,
            color: alpha(color),
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

fn mesh(key: Key) -> Option<UiMesh> {
    let [width, height, radius, fringe] =
        [key.width, key.height, key.radius, key.fringe].map(f32::from_bits);
    let fringe = fringe.min(width.min(height) * 0.25);
    let inner_radius = (radius - fringe).max(0.0);
    let centers = [
        [radius, radius],
        [width - radius, radius],
        [width - radius, height - radius],
        [radius, height - radius],
    ];
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
    let mut vertices = vec![vertex([width * 0.5, height * 0.5], key.color)];
    for (corner, center) in centers.into_iter().enumerate() {
        let start = std::f32::consts::PI + corner as f32 * std::f32::consts::FRAC_PI_2;
        for step in 0..=CORNER_STEPS {
            let angle = start + step as f32 / CORNER_STEPS as f32 * std::f32::consts::FRAC_PI_2;
            let direction = [angle.cos(), angle.sin()];
            vertices.push(vertex(
                [
                    center[0] + direction[0] * inner_radius,
                    center[1] + direction[1] * inner_radius,
                ],
                key.color,
            ));
            let mut transparent = key.color;
            transparent[3] = 0;
            vertices.push(vertex(
                [
                    center[0] + direction[0] * radius,
                    center[1] + direction[1] * radius,
                ],
                transparent,
            ));
        }
    }
    let count = 4 * (CORNER_STEPS + 1);
    let mut indices = Vec::with_capacity(count * 9);
    for point in 0..count {
        let inner = 1 + point as u32 * 2;
        let next = 1 + ((point + 1) % count) as u32 * 2;
        indices.extend([
            0,
            inner,
            next,
            inner,
            inner + 1,
            next + 1,
            inner,
            next + 1,
            next,
        ]);
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
