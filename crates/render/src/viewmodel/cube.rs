//! Bounded six-face geometry transport. Pose, lighting and action parity remain unavailable.
use super::*;
use assets::{
    BlockFace, BlockFlags, BlockVisualId, NetworkIdMode, RuntimeAssets, VisualKind, VisualSupport,
};
use sha2::{Digest, Sha256};

const TILE_SIDE: usize = assets::BLOCK_ITEM_FACE_SIDE as usize;
const TILE_PITCH: usize = TILE_SIDE + 2;
const TILE_COLUMNS: usize = assets::BLOCK_ITEM_SHEET_GRID[0] as usize;

impl ViewmodelGeometry {
    /// Builds an ordinary opaque cube from the current validated block carrier.
    /// The existing nearest-only skin transport contains six unmodified tiles.
    pub fn opaque_cube(
        assets: &RuntimeAssets,
        visual: BlockVisualId,
    ) -> Option<(Self, ViewmodelSkin)> {
        if !assets.provenance().is_complete() || visual.0 as usize >= assets.visual_count() {
            return None;
        }
        let block = assets.resolve(NetworkIdMode::Sequential, visual.0);
        if !block.is_known()
            || block.kind() != VisualKind::Cube
            || block.support() != VisualSupport::Exact
            || !block
                .flags()
                .contains(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            || block.model_template().is_some()
            || block.animation().is_some()
        {
            return None;
        }
        // Admit the whole source before allocating or copying any pixels.
        let mut tiles = [&[][..]; 6];
        let mut materials = [0; 6];
        for (index, face) in BlockFace::ALL.into_iter().enumerate() {
            let id = block.face(face).material_id();
            if id == assets::DIAGNOSTIC_MATERIAL {
                return None;
            }
            let material = assets.materials().get(id as usize)?;
            // World-position rotation leaves carried face pixels and UVs unchanged.
            if material.flags & !assets::MATERIAL_FLAG_ISOTROPIC != 0
                || material.animation != assets::NO_ANIMATION
            {
                return None;
            }
            let page = assets
                .texture_pages()
                .get(material.texture.page() as usize)?;
            let mip = page.texture.mips.first()?;
            if mip.size != TILE_SIDE as u32 || material.texture.layer() >= page.texture.layers {
                return None;
            }
            let first =
                (material.texture.layer() as usize).checked_mul(TILE_SIDE * TILE_SIDE * 4)?;
            let tile = mip
                .rgba8
                .get(first..first.checked_add(TILE_SIDE * TILE_SIDE * 4)?)?;
            if !tile.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255) {
                return None;
            }
            tiles[index] = tile;
            materials[index] = id;
        }
        let side = VIEWMODEL_TEXTURE_SIDE as usize;
        let mut pixels = vec![0; VIEWMODEL_TEXTURE_BYTES];
        for (index, tile) in tiles.into_iter().enumerate() {
            let [x, y] = [
                (index % TILE_COLUMNS) * TILE_PITCH,
                (index / TILE_COLUMNS) * TILE_PITCH,
            ];
            for row in 0..TILE_PITCH {
                for column in 0..TILE_PITCH {
                    let source = (row.saturating_sub(1).min(TILE_SIDE - 1) * TILE_SIDE
                        + column.saturating_sub(1).min(TILE_SIDE - 1))
                        * 4;
                    let target = ((y + row) * side + x + column) * 4;
                    pixels[target..target + 4].copy_from_slice(&tile[source..source + 4]);
                }
            }
        }
        // Same face corners and UV directions as the existing world cube pipeline.
        let corners = [
            [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
            [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
            [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
            [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        ];
        let horizontal = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        let transposed = [[0., 0.], [0., 1.], [1., 1.], [1., 0.]];
        let vertical = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
        let vertical_transposed = [[0., 1.], [0., 0.], [1., 0.], [1., 1.]];
        let transform = Mat4::from_translation(Vec3::new(0.56, -0.52, -0.72))
            * Mat4::from_rotation_y(45_f32.to_radians())
            * Mat4::from_scale(Vec3::splat(0.4));
        let mut vertices = Vec::with_capacity(36);
        for (face, positions) in corners.into_iter().enumerate() {
            let uv = match face {
                0 | 5 => vertical,
                1 | 4 => vertical_transposed,
                3 => transposed,
                _ => horizontal,
            };
            for corner in [0, 1, 2, 0, 2, 3] {
                let p = transform
                    .transform_point3(Vec3::from_array(positions[corner]) - Vec3::splat(0.5));
                vertices.push(HandVertex {
                    position: p.to_array(),
                    uv: [
                        ((face % TILE_COLUMNS * TILE_PITCH + 1) as f32
                            + uv[corner][0] * TILE_SIDE as f32)
                            / VIEWMODEL_TEXTURE_SIDE as f32,
                        ((face / TILE_COLUMNS * TILE_PITCH + 1) as f32
                            + uv[corner][1] * TILE_SIDE as f32)
                            / VIEWMODEL_TEXTURE_SIDE as f32,
                    ],
                });
            }
        }
        let pixel_identity: [u8; 32] = Sha256::digest(&pixels).into();
        let mut digest = Sha256::new();
        digest.update(b"opaque-cube-static-v1");
        digest.update(assets.provenance().source_manifest_sha256);
        digest.update(assets.provenance().block_registry_sha256);
        digest.update(visual.0.to_le_bytes());
        for material in materials {
            digest.update(material.to_le_bytes());
        }
        digest.update(pixel_identity);
        digest.update(bytemuck::cast_slice(&vertices));
        let geometry = Self {
            vertices: vertices.into(),
            identity: digest.finalize().into(),
            allowed_rigs: Arc::from([]),
            cube_origin: true,
        };
        let skin = ViewmodelSkin::new(pixels.into(), pixel_identity)?;
        Some((geometry, skin))
    }
}
