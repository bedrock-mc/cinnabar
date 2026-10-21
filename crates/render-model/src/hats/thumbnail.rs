//! Software gallery thumbnails from the same admitted attachment surface and texture.
use super::*;

/// Source canvas shared by live cosmetic tiles and their artwork fallback.
pub const SIZE: [u32; 2] = [96, 112];

/// Fixed gallery scale fitted to the repeating motion while keeping isolated items centered.
pub struct Frame {
    rotation: Quat,
    span: Vec3,
    model_min: Vec3,
    model_max: Vec3,
}

impl Frame {
    /// Samples a bounded cycle on the artwork worker, retaining one camera for all frames.
    fn new(model: &HatRig) -> Self {
        let rotation = Quat::from_rotation_x(-8.0_f32.to_radians())
            * Quat::from_rotation_y(-20.0_f32.to_radians());
        let (mut model_min, mut model_max) =
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut span = Vec3::ZERO;
        let mut include = |vertices: &[ActorVertex]| {
            let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for vertex in vertices {
                let point = Vec3::from_array(vertex.position);
                model_min = model_min.min(point);
                model_max = model_max.max(point);
                let p = rotation * point;
                min = min.min(p);
                max = max.max(p);
            }
            span = span.max(max - min);
        };
        include(&model.rest_vertices);
        if let Some(clip) = &model.playback {
            const SAMPLES: usize = 128;
            for sample in 1..SAMPLES {
                let seconds =
                    f64::from(clip.length - clip.rest_time()) * sample as f64 / SAMPLES as f64;
                include(&model.preview_vertices(model.preview_motion(seconds)));
            }
        }
        Self {
            rotation,
            span,
            model_min,
            model_max,
        }
    }

    /// Returns the sampled model envelope for character preview fitting.
    pub fn model_bounds(&self) -> [[f32; 3]; 2] {
        [self.model_min.to_array(), self.model_max.to_array()]
    }

    /// Centers an isolated posed cosmetic without resizing it as joints move.
    pub fn center(&self, vertices: &[ActorVertex]) -> [f32; 3] {
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for vertex in vertices {
            let point = self.rotation * Vec3::from_array(vertex.position);
            min = min.min(point);
            max = max.max(point);
        }
        ((min + max) * 0.5).to_array()
    }

    /// Projects onto a stable unit rectangle with the same six-pixel inset as gallery artwork.
    pub fn project(
        &self,
        position: [f32; 3],
        center: [f32; 3],
        width: u32,
        height: u32,
    ) -> [f32; 3] {
        let point = self.rotation * Vec3::from_array(position);
        let span = self.span;
        let scale = ((width as f32 - 12.0) / span.x).min((height as f32 - 12.0) / span.y);
        let center = Vec3::from_array(center);
        let depth = span.z.max(f32::EPSILON);
        [
            0.5 + (point.x - center.x) * scale / width as f32,
            0.5 - (point.y - center.y) * scale / height as f32,
            0.5 + (point.z - center.z) / depth * 0.5,
        ]
    }
}

/// Returns the worker-prepared camera without doing first-use fitting on the frame thread.
pub fn prepared_frame(item: Item) -> Option<&'static Frame> {
    rig(item).preview_frame.get()
}

/// Fits a camera once on an artwork worker before either textures or thumbnails are published.
pub fn prepare_frame(item: Item) -> &'static Frame {
    let model = rig(item);
    model.preview_frame.get_or_init(|| Frame::new(model))
}

/// Fits the authored surface into a transparent, bounded gallery image.
pub fn render(item: Item, width: u32, height: u32) -> Option<CapeImage> {
    render_vertices(item, width, height, &rig(item).rest_vertices)
}

/// Renders an authored preview phase with the same geometry and fixed camera as live gallery tiles.
pub fn render_at(item: Item, width: u32, height: u32, seconds: f64) -> Option<CapeImage> {
    let model = rig(item);
    let vertices = model.preview_vertices(model.preview_motion(seconds));
    render_vertices(item, width, height, &vertices)
}

/// Rasterizes one posed surface through the prepared gallery camera and original texels.
fn render_vertices(
    item: Item,
    width: u32,
    height: u32,
    vertices: &[ActorVertex],
) -> Option<CapeImage> {
    if width <= 12 || height <= 12 || width > 256 || height > 256 {
        return None;
    }
    let model = rig(item);
    let frame = prepare_frame(item);
    let center = frame.center(vertices);
    let points: Vec<_> = vertices
        .iter()
        .map(|vertex| {
            let [x, y, z] = frame.project(vertex.position, center, width, height);
            ([x * width as f32, y * height as f32, z], vertex.uv)
        })
        .collect();
    let mut rgba8 = vec![0; (width * height * 4) as usize];
    let mut depth = vec![f32::NEG_INFINITY; (width * height) as usize];
    for triangle in points.as_chunks::<3>().0.iter() {
        let area = edge(triangle[0].0, triangle[1].0, triangle[2].0);
        if area.abs() < 1e-6 {
            continue;
        }
        let left = triangle
            .iter()
            .map(|(p, _)| p[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let right = triangle
            .iter()
            .map(|(p, _)| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(width as f32) as u32;
        let top = triangle
            .iter()
            .map(|(p, _)| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let bottom = triangle
            .iter()
            .map(|(p, _)| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(height as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                let sample = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
                let weights = [
                    edge(triangle[1].0, triangle[2].0, sample) / area,
                    edge(triangle[2].0, triangle[0].0, sample) / area,
                    edge(triangle[0].0, triangle[1].0, sample) / area,
                ];
                if weights.iter().any(|weight| *weight < -1e-6) {
                    continue;
                }
                let z: f32 = (0..3).map(|i| weights[i] * triangle[i].0[2]).sum();
                let index = (y * width + x) as usize;
                if z < depth[index] {
                    continue;
                }
                let uv: [f32; 2] = std::array::from_fn(|axis| {
                    (0..3).map(|i| weights[i] * triangle[i].1[axis]).sum()
                });
                let tx = (uv[0].clamp(0.0, 1.0) * model.texture.width as f32) as u32;
                let ty = (uv[1].clamp(0.0, 1.0) * model.texture.height as f32) as u32;
                let source = ((ty.min(model.texture.height - 1) * model.texture.width
                    + tx.min(model.texture.width - 1))
                    * 4) as usize;
                if model.texture.rgba8[source + 3] < 128 {
                    continue;
                }
                rgba8[index * 4..index * 4 + 4]
                    .copy_from_slice(&model.texture.rgba8[source..source + 4]);
                depth[index] = z;
            }
        }
    }
    Some(CapeImage {
        width,
        height,
        rgba8: rgba8.into(),
    })
}

/// Returns a signed triangle edge area in screen coordinates.
fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])
}
