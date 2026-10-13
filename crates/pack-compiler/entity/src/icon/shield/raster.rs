use super::{EquipmentTexture, SIDE};

pub(super) fn triangle(
    pixels: &mut [u8],
    points: [[f32; 2]; 3],
    uvs: [[f32; 2]; 3],
    texture: &EquipmentTexture,
) {
    let edge = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    let area = edge(points[0], points[1], points[2]);
    if !area.is_finite() || area <= 0.0 {
        return;
    }
    let bounds = |axis: usize| {
        let low = points
            .iter()
            .map(|point| point[axis])
            .fold(f32::INFINITY, f32::min);
        let high = points
            .iter()
            .map(|point| point[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        low.floor().clamp(0.0, SIDE as f32) as usize..high.ceil().clamp(0.0, SIDE as f32) as usize
    };
    for y in bounds(1) {
        for x in bounds(0) {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let weights = [
                edge(points[1], points[2], p),
                edge(points[2], points[0], p),
                edge(points[0], points[1], p),
            ]
            .map(|value| value / area);
            let covered = weights.iter().enumerate().all(|(index, &weight)| {
                let a = points[(index + 1) % 3];
                let b = points[(index + 2) % 3];
                let top_left = b[1] > a[1] || (b[1] == a[1] && b[0] < a[0]);
                weight > 0.0 || (weight == 0.0 && top_left)
            });
            if !covered {
                continue;
            }
            let uv: [f32; 2] = std::array::from_fn(|axis| {
                (0..3).map(|index| weights[index] * uvs[index][axis]).sum()
            });
            let tx = (uv[0] * f32::from(texture.width))
                .floor()
                .clamp(0.0, f32::from(texture.width - 1)) as usize;
            let ty = (uv[1] * f32::from(texture.height))
                .floor()
                .clamp(0.0, f32::from(texture.height - 1)) as usize;
            let source = &texture.rgba8[(ty * usize::from(texture.width) + tx) * 4..][..4];
            // ui_shield.skinning inherits ALPHA_TEST; no FANCY side-lighting for UI_ENTITY.
            let alpha = f32::from(source[3]) / f32::from(u8::MAX);
            if alpha < assets::gui_item::SHIELD_ALPHA_CUTOFF {
                continue;
            }
            let output = &mut pixels[(y * SIDE + x) * 4..][..4];
            let previous = f32::from(output[3]) / 255.0;
            let final_alpha = alpha + previous * (1.0 - alpha);
            for channel in 0..3 {
                output[channel] = ((f32::from(source[channel]) * alpha
                    + f32::from(output[channel]) * previous * (1.0 - alpha))
                    / final_alpha)
                    .round() as u8;
            }
            output[3] = (final_alpha * 255.0).round() as u8;
        }
    }
}
