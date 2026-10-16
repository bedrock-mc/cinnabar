//! Gallery art renders the same authored mesh and texture as the equipped attachment.

use render_api::Hat;

/// Decodes either original texels or a fitted model thumbnail on the artwork worker.
pub(in crate::ui_runtime::presentation) fn decode(
    hat: Hat,
    thumbnail: bool,
) -> Option<(Vec<u8>, u32, u32)> {
    let model = render_model::hats::rig(hat);
    if !thumbnail {
        return Some((
            model.texture.rgba8.to_vec(),
            model.texture.width,
            model.texture.height,
        ));
    }
    let texture = super::cape::texture(&model.texture)?;
    let width = super::PREVIEW_WIDTH as usize;
    let height = super::PREVIEW_HEIGHT as usize;
    let mut pixels = vec![0; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let rig = super::Rig::new(
        Default::default(),
        super::PreviewView::Doll {
            yaw: -20.0,
            tilt: -8.0,
        },
        0.0,
        [false; 2],
    );
    let mut vertices: Vec<_> = model
        .rest_vertices
        .iter()
        .map(|vertex| rig.project(*vertex))
        .collect();
    let (min, max) = vertices.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(mut min, mut max), vertex| {
            for axis in 0..2 {
                min[axis] = min[axis].min(vertex.screen[axis]);
                max[axis] = max[axis].max(vertex.screen[axis]);
            }
            (min, max)
        },
    );
    let size = [width as f32, height as f32];
    let scale = ((size[0] - 12.0) / (max[0] - min[0])).min((size[1] - 12.0) / (max[1] - min[1]));
    for vertex in &mut vertices {
        vertex.screen = std::array::from_fn(|axis| {
            (vertex.screen[axis] - (min[axis] + max[axis]) * 0.5) * scale + size[axis] * 0.5
        });
    }
    for triangle in vertices.as_chunks::<3>().0 {
        super::rasterize_triangle(
            &mut pixels,
            &mut depth,
            width,
            height,
            &|uv| texture.sample(uv),
            [triangle[0], triangle[1], triangle[2]],
        );
    }
    Some((pixels, width as u32, height as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_gallery_renders_the_original_black_and_pink_geometry() {
        let (pixels, width, height) = decode(Hat::BunnyEars, true).unwrap();
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0));
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0 && u16::from(pixel[0]) > u16::from(pixel[1]) * 3 / 2)
        );
        assert_eq!(pixels, decode(Hat::BunnyEars, true).unwrap().0);
    }
}
