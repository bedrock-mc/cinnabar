//! CPU composition of the launcher's panorama beneath transparent screen paint.

use assets::RuntimeUiAssets;
use render_model::panorama::{
    PanoramaFaces, PanoramaView, launcher_faces, launcher_view, overlay_tint,
};

use crate::carriers::Error;

/// Uses the title screen's faces, camera and tint at the snapshot's fixed time.
pub(super) fn composite(
    pixels: &mut [u8],
    size: [u32; 2],
    assets: &RuntimeUiAssets,
) -> Result<(), Error> {
    let faces = launcher_faces(assets)
        .ok_or_else(|| Error::Missing("title panorama faces; make assets".into()))?;
    let view = launcher_view(
        super::CAPTURE_SECONDS as f32,
        size[0] as f32 / size[1] as f32,
        overlay_tint(assets),
    );
    blend(pixels, size, &faces, view);
    Ok(())
}

/// Samples pixel centers with the panorama pass's cube convention and linear filtering.
fn blend(pixels: &mut [u8], size: [u32; 2], faces: &PanoramaFaces, view: PanoramaView) {
    let [yaw, pitch, tan_half, aspect, ..] = view.shader_uniform();
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let alpha = f32::from(pixel[3]) / 255.0;
        if alpha == 1.0 {
            continue;
        }
        let x = (index as u32 % size[0]) as f32 + 0.5;
        let y = (index as u32 / size[0]) as f32 + 0.5;
        let dx = (x / size[0] as f32 * 2.0 - 1.0) * tan_half * aspect;
        let dy = (1.0 - y / size[1] as f32 * 2.0) * tan_half;
        let pitched_y = dy * cp + sp;
        let pitched_z = dy * sp - cp;
        let direction = [
            dx * cy - pitched_z * sy,
            pitched_y,
            dx * sy + pitched_z * cy,
        ];
        let (face, uv) = face_uv(direction);
        let background = sample(faces, face, uv);
        for channel in 0..3 {
            let tinted =
                background[channel] * (1.0 - view.tint[3]) + view.tint[channel] * view.tint[3];
            let srgb = linear_to_srgb(tinted);
            pixel[channel] =
                (f32::from(pixel[channel]) * alpha + srgb * (1.0 - alpha)).round() as u8;
        }
        pixel[3] = 255;
    }
}

/// Selects the pack face seen from inside the cube and its normalized coordinates.
fn face_uv([x, y, z]: [f32; 3]) -> (usize, [f32; 2]) {
    let [ax, ay, az] = [x.abs(), y.abs(), z.abs()];
    let (face, u, v) = if ay >= ax && ay >= az {
        if y > 0.0 {
            (4, x / ay, -z / ay)
        } else {
            (5, x / ay, z / ay)
        }
    } else if ax >= az {
        if x > 0.0 {
            (1, z / ax, -y / ax)
        } else {
            (3, -z / ax, -y / ax)
        }
    } else if z < 0.0 {
        (0, x / az, -y / az)
    } else {
        (2, -x / az, -y / az)
    };
    (face, [u * 0.5 + 0.5, v * 0.5 + 0.5])
}

/// Bilinearly samples one sRGB face in linear color, clamping at its edges.
fn sample(faces: &PanoramaFaces, face: usize, uv: [f32; 2]) -> [f32; 3] {
    let side = faces.side() as usize;
    let [x, y] =
        uv.map(|coordinate| (coordinate * side as f32 - 0.5).clamp(0.0, (side - 1) as f32));
    let [left, top] = [x as usize, y as usize];
    let [right, bottom] = [(left + 1).min(side - 1), (top + 1).min(side - 1)];
    let [fx, fy] = [x - left as f32, y - top as f32];
    let texel = |x, y, channel| {
        let index = ((face * side + y) * side + x) * 4 + channel;
        srgb_to_linear(faces.layer_major()[index])
    };
    std::array::from_fn(|channel| {
        let a = texel(left, top, channel) * (1.0 - fx) + texel(right, top, channel) * fx;
        let b = texel(left, bottom, channel) * (1.0 - fx) + texel(right, bottom, channel) * fx;
        a * (1.0 - fy) + b * fy
    })
}

/// Decodes an sRGB texture channel before filtering, as the GPU texture format does.
fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes the panorama pass's linear output for the CPU screenshot target.
fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    255.0
        * if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panorama_composition_preserves_panels_and_fills_transparent_pixels() {
        let faces =
            PanoramaFaces::new(1, std::array::from_fn(|_| vec![120, 160, 200, 255])).unwrap();
        let mut pixels = [0, 0, 0, 0, 12, 34, 56, 255];
        blend(
            &mut pixels,
            [2, 1],
            &faces,
            launcher_view(0.0, 2.0, [0.0; 4]),
        );
        assert_eq!(pixels, [120, 160, 200, 255, 12, 34, 56, 255]);
    }

    #[test]
    fn cube_axes_select_the_pack_faces_in_order() {
        for (face, direction) in [
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(face_uv(direction), (face, [0.5, 0.5]));
        }
    }
}
