//! Portal overlay pixels come from the active compiled block pack, like native atlas.terrain.

use assets::{
    BlockFace, MODEL_QUAD_FLAG_FACE_MASK, MODEL_TEMPLATE_FLAG_NETHER_PORTAL, Material,
    NO_ANIMATION, RuntimeAssets, TextureRef,
};

use crate::{ChunkAnimationClock, select_animation_frames};

pub(crate) struct PortalTexture {
    pub side: u32,
    pub pixels: Vec<u8>,
    references: Vec<TextureRef>,
    material: Material,
}

impl PortalTexture {
    pub fn from_assets(assets: &RuntimeAssets) -> Option<Self> {
        let template = assets
            .model_templates()
            .iter()
            .find(|template| template.flags & MODEL_TEMPLATE_FLAG_NETHER_PORTAL != 0)?;
        // Vanilla's portal overlay texture uses the default face zero.
        let start = template.quad_start as usize;
        let quads = assets
            .model_quads()
            .get(start..start.checked_add(template.quad_count as usize)?)?;
        let quad = quads.iter().find(|quad| {
            quad.flags & MODEL_QUAD_FLAG_FACE_MASK == BlockFace::Down.model_quad_face_id()
        })?;
        let material = assets.material(quad.material);
        let static_frames = [material.texture];
        let timeline = if material.animation == NO_ANIMATION {
            &static_frames[..]
        } else {
            let animation = assets.animations().get(material.animation as usize)?;
            let start = animation.frame_start as usize;
            assets
                .animation_frames()
                .get(start..start.checked_add(animation.frame_count as usize)?)?
        };
        let mut references = Vec::new();
        for &reference in timeline {
            if !references.contains(&reference) {
                references.push(reference);
            }
        }
        let first = *references.first()?;
        let side = assets
            .texture_pages()
            .get(first.page() as usize)?
            .texture
            .mips
            .first()?
            .size;
        let bytes = usize::try_from(side)
            .ok()?
            .checked_mul(side as usize)?
            .checked_mul(4)?;
        let mut pixels = Vec::with_capacity(bytes.checked_mul(references.len())?);
        for &reference in &references {
            let page = assets.texture_pages().get(reference.page() as usize)?;
            let mip = page.texture.mips.first()?;
            if mip.size != side {
                return None;
            }
            let start = bytes.checked_mul(reference.layer() as usize)?;
            pixels.extend_from_slice(mip.rgba8.get(start..start.checked_add(bytes)?)?);
        }
        Some(Self {
            side,
            pixels,
            references,
            material,
        })
    }

    pub fn layer_count(&self) -> u32 {
        self.references.len() as u32
    }

    /// Reuses the world shader's timeline oracle, including repeated frames and wrap blending.
    pub fn frame_uniform(&self, assets: &RuntimeAssets, clock: ChunkAnimationClock) -> [f32; 4] {
        let sample = select_animation_frames(
            self.material,
            assets.animations(),
            assets.animation_frames(),
            clock,
        );
        let current = self
            .references
            .iter()
            .position(|reference| *reference == sample.current);
        let next = self
            .references
            .iter()
            .position(|reference| *reference == sample.next);
        match current.zip(next) {
            Some((current, next)) => [current as f32, next as f32, sample.blend, 1.0],
            None => [0.0; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_assets_do_not_invent_a_portal_texture() {
        assert!(PortalTexture::from_assets(&RuntimeAssets::diagnostic()).is_none());
    }

    #[test]
    fn portal_overlay_uses_active_pack_face_and_both_texture_pages_with_world_timing() {
        use assets::{
            Animation, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets,
            ContributorRole, ModelQuad, ModelTemplate, TextureArray, TextureMip, TexturePage,
        };
        let reference = |page, layer| TextureRef::new(page, layer).unwrap();
        let pages = [10_u8, 50].map(|base| {
            TexturePage::new(TextureArray {
                layers: 3,
                mips: [16_u32, 8, 4, 2, 1]
                    .map(|size| TextureMip {
                        size,
                        rgba8: (0..3_u8)
                            .flat_map(|layer| vec![base + layer; size as usize * size as usize * 4])
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    })
                    .into(),
            })
        });
        let quad = |face: BlockFace, material| ModelQuad {
            positions: [[0, 0, 0], [256, 0, 0], [256, 0, 256], [0, 0, 256]],
            uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            material,
            flags: face.model_quad_face_id(),
        };
        let compiled = CompiledAssets {
            visuals: vec![BlockVisual::diagnostic(
                BlockFlags::empty(),
                ContributorRole::Primary,
            )]
            .into(),
            light_properties: vec![assets::LightProperties::default()].into(),
            hashed: Box::new([]),
            materials: vec![
                Material::unvaried(),
                Material {
                    texture: reference(1, 1),
                    animation: 0,
                    ..Material::unvaried()
                },
                Material {
                    texture: reference(0, 1),
                    ..Material::unvaried()
                },
            ]
            .into(),
            model_templates: vec![ModelTemplate {
                quad_start: 0,
                quad_count: BlockFace::ALL.len() as u32,
                flags: MODEL_TEMPLATE_FLAG_NETHER_PORTAL,
            }]
            .into(),
            model_quads: BlockFace::ALL
                .map(|face| quad(face, if face == BlockFace::Down { 1 } else { 2 }))
                .into(),
            animations: vec![Animation {
                frame_start: 0,
                frame_count: 3,
                ticks_per_frame: 2,
                atlas_index: 0,
                atlas_tile_variant: 0,
                replicate: 1,
                flags: assets::ANIMATION_FLAG_BLEND,
            }]
            .into(),
            animation_frames: vec![reference(1, 1), reference(0, 2), reference(1, 1)].into(),
            texture_pages: pages.into(),
            biomes: CompiledBiomeAssets::diagnostic(),
            provenance: assets::BlobProvenance {
                source_manifest_sha256: [1; 32],
                block_registry_sha256: [2; 32],
                light_registry_sha256: [3; 32],
                biome_registry_sha256: [4; 32],
            },
        };
        let assets = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        let portal = PortalTexture::from_assets(&assets).unwrap();
        assert_eq!(portal.layer_count(), 2);
        assert_eq!(portal.pixels[0], 51);
        assert_eq!(portal.pixels[16 * 16 * 4], 12);
        assert_eq!(
            portal.frame_uniform(&assets, ChunkAnimationClock::from_parts(2, 0.5)),
            [1.0, 0.0, 0.25, 1.0]
        );
        assert_eq!(
            portal.frame_uniform(&assets, ChunkAnimationClock::from_parts(5, 0.0)),
            [0.0, 0.0, 0.5, 1.0]
        );
    }
}
