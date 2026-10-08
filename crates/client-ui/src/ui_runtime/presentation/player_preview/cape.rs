use std::sync::OnceLock;

use assets::{EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv};
use render_model::ActorVertex;

use super::PreviewTexture;

pub(crate) fn same(old: Option<&protocol::CapeImage>, new: Option<&protocol::CapeImage>) -> bool {
    match (old, new) {
        (None, None) => true,
        (Some(old), Some(new)) => {
            old.width == new.width
                && old.height == new.height
                && std::sync::Arc::ptr_eq(&old.rgba8, &new.rgba8)
        }
        _ => false,
    }
}

pub(super) fn texture(cape: &protocol::CapeImage) -> Option<PreviewTexture> {
    cape.is_valid().then(|| PreviewTexture {
        rgba: cape.rgba8.clone(),
        width: cape.width as u16,
        height: cape.height as u16,
        tint: None,
    })
}

/// A resting cape uses its own rectangular UV net and follows the torso's transform.
pub(crate) fn rest_vertices() -> &'static [ActorVertex] {
    static VERTICES: OnceLock<Vec<ActorVertex>> = OnceLock::new();
    VERTICES.get_or_init(|| {
        let scalar = |value| EntityGeometryScalar::new(value).expect("bounded cape geometry");
        let cube = EntityGeometryCube {
            origin: [-5.0, 8.0, 3.0].map(scalar),
            size: [10.0, 16.0, 1.0].map(scalar),
            pivot: [0.0, 24.0, 3.0].map(scalar),
            rotation: [-6.0, 180.0, 0.0].map(scalar),
            uv: EntityGeometryUv::Box([EntityGeometryScalar::ZERO; 2]),
            inflate: EntityGeometryScalar::ZERO,
            mirror: false,
        };
        let mut vertices = Vec::new();
        render_model::append_entity_cube_vertices(&mut vertices, &cube, 1, (64, 32), false, 0.0)
            .expect("valid cape geometry");
        vertices
            .into_iter()
            .map(|vertex| ActorVertex {
                position: [-vertex.position[0], vertex.position[1], -vertex.position[2]],
                uv: vertex.uv,
                part: 1,
            })
            .collect()
    })
}

/// Cape-gallery artwork shows the back of the attached cape without obscuring it with a body.
pub fn render_cape_thumbnail(cape: &protocol::CapeImage) -> Option<Vec<u8>> {
    let texture = texture(cape)?;
    let width = super::PREVIEW_WIDTH as usize;
    let height = super::PREVIEW_HEIGHT as usize;
    let mut pixels = vec![0; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let sample = |uv| texture.sample(uv).filter(|texel| texel[3] >= 128);
    for triangle in thumbnail_projection().chunks_exact(3) {
        super::rasterize_triangle(
            &mut pixels,
            &mut depth,
            width,
            height,
            &sample,
            [triangle[0], triangle[1], triangle[2]],
        );
    }
    Some(pixels)
}

fn thumbnail_projection() -> &'static [super::ProjectedVertex] {
    static PROJECTED: OnceLock<Vec<super::ProjectedVertex>> = OnceLock::new();
    PROJECTED.get_or_init(|| {
        let rig = super::Rig::new(
            Default::default(),
            super::PreviewView::Doll {
                yaw: 180.0,
                tilt: -10.0,
            },
            0.0,
            [false; 2],
        );
        let mut vertices: Vec<_> = rest_vertices()
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
        let size = [super::PREVIEW_WIDTH as f32, super::PREVIEW_HEIGHT as f32];
        const PADDING: f32 = 6.0;
        let scale = ((size[0] - 2.0 * PADDING) / (max[0] - min[0]))
            .min((size[1] - 2.0 * PADDING) / (max[1] - min[1]));
        for vertex in &mut vertices {
            vertex.screen = std::array::from_fn(|axis| {
                (vertex.screen[axis] - (min[axis] + max[axis]) * 0.5) * scale + size[axis] * 0.5
            });
        }
        vertices
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cape_preserves_original_hd_texels_and_rejects_malformed_images() {
        let mut cape = protocol::CapeImage {
            width: 128,
            height: 64,
            rgba8: [17, 51, 199, 255].repeat(128 * 64).into(),
        };
        let texture = texture(&cape).expect("supported cape");
        assert!(std::sync::Arc::ptr_eq(&texture.rgba, &cape.rgba8));
        assert_eq!((texture.width, texture.height), (128, 64));
        assert_eq!(
            texture.sample([13.5 / 128.0, 11.5 / 64.0]),
            Some([17, 51, 199, 255])
        );
        cape.rgba8 = vec![255; 3].into();
        assert!(super::texture(&cape).is_none());
        assert!(render_cape_thumbnail(&cape).is_none());
    }

    #[test]
    fn cape_card_renders_its_rectangular_net_and_keeps_transparent_pixels() {
        let cape = protocol::CapeImage {
            width: 64,
            height: 32,
            rgba8: [23, 211, 37, 255].repeat(64 * 32).into(),
        };
        let image = render_cape_thumbnail(&cape).expect("valid cape card");
        assert_eq!(
            image.len(),
            (super::super::PREVIEW_WIDTH * super::super::PREVIEW_HEIGHT * 4) as usize
        );
        assert!(image.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(
            image
                .chunks_exact(4)
                .any(|pixel| pixel[3] == 255 && pixel[1] > pixel[0])
        );
        let transparent = protocol::CapeImage {
            rgba8: vec![0; 64 * 32 * 4].into(),
            ..cape
        };
        assert!(
            render_cape_thumbnail(&transparent)
                .unwrap()
                .iter()
                .all(|value| *value == 0)
        );
    }

    #[test]
    fn opaque_cape_thumbnail_fills_its_stage_with_safe_padding() {
        use super::super::{PREVIEW_HEIGHT, PREVIEW_WIDTH};
        let (width, height) = protocol::CAPE_DIMENSIONS[0];
        let cape = protocol::CapeImage {
            width,
            height,
            rgba8: [23, 211, 37, 255].repeat((width * height) as usize).into(),
        };
        let image = render_cape_thumbnail(&cape).expect("valid cape thumbnail");
        let mut bounds = [PREVIEW_WIDTH, PREVIEW_HEIGHT, 0, 0];
        for (index, pixel) in image.chunks_exact(4).enumerate() {
            if pixel[3] != 0 {
                let x = index as u32 % PREVIEW_WIDTH;
                let y = index as u32 / PREVIEW_WIDTH;
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x + 1),
                    bounds[3].max(y + 1),
                ];
            }
        }
        assert!(
            bounds[3] - bounds[1] >= PREVIEW_HEIGHT * 4 / 5,
            "cape occupies most of its stage: {bounds:?}"
        );
        assert!(
            bounds[0] >= 4 && bounds[1] >= 4,
            "cape retains top and left padding: {bounds:?}"
        );
        assert!(
            bounds[2] <= PREVIEW_WIDTH - 4 && bounds[3] <= PREVIEW_HEIGHT - 4,
            "cape retains bottom and right padding: {bounds:?}"
        );
    }
}
