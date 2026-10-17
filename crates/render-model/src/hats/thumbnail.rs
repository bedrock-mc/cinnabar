//! Software gallery thumbnails from the same admitted attachment surface and texture.
use super::*;

/// Fits the authored surface into a transparent, bounded gallery image.
pub fn render(item: Item, width: u32, height: u32) -> Option<CapeImage> {
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return None;
    }
    let model = rig(item);
    let rotation = Quat::from_rotation_x(-8.0_f32.to_radians())
        * Quat::from_rotation_y(-20.0_f32.to_radians());
    let mut points: Vec<_> = model
        .rest_vertices
        .iter()
        .map(|vertex| {
            let point = rotation * Vec3::from_array(vertex.position);
            ([point.x, -point.y, point.z], vertex.uv)
        })
        .collect();
    let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for (point, _) in &points {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    let scale =
        ((width as f32 - 12.0) / (max[0] - min[0])).min((height as f32 - 12.0) / (max[1] - min[1]));
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    for (point, _) in &mut points {
        for axis in 0..2 {
            point[axis] = (point[axis] - (min[axis] + max[axis]) * 0.5) * scale
                + [width, height][axis] as f32 * 0.5;
        }
    }
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
