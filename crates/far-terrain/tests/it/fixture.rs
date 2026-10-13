//! A synthetic pack and column builder for surface extraction tests.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, LinearBiomeTints, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_WATER_TINT, Material, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode,
    ResolvedBiomeTints, RuntimeAssets, TextureArray, TextureMip, TexturePage, TextureRef,
    VisualKind,
};
use far_terrain::{ColumnSnapshot, SurfaceColours, SurfaceContext};
use world::{RawBlockIds, SubChunk};

pub const AIR: u32 = 0;
pub const GRASS: u32 = 1;
pub const SAND: u32 = 2;
pub const WATER: u32 = 3;
pub const FLOWER: u32 = 4;
pub const LEAVES: u32 = 5;

pub const SAND_RGB: [u8; 3] = [200, 180, 100];
pub const LEAF_RGB: [u8; 3] = [60, 160, 60];
pub const GRASS_TINT: [f32; 3] = [0.2, 0.5, 0.1];
pub const FOLIAGE_TINT: [f32; 3] = [0.1, 0.4, 0.1];
pub const WATER_TINT: [f32; 3] = [0.1, 0.2, 0.8];

const LEAF_MATERIAL_BASE: u32 = 3;

fn visual(kind: VisualKind, flags: BlockFlags, material: u32, variant: u32) -> BlockVisual {
    BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        faces: [material; 6],
        flags,
        kind,
        support: assets::VisualSupport::Exact,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    }
}

/// Layers: white, sand, grey, and a leaf texture whose transparent half must not count.
fn textures() -> TextureArray {
    let layers: [Box<dyn Fn(u32) -> [u8; 4]>; 4] = [
        Box::new(|_| [255; 4]),
        Box::new(|_| [SAND_RGB[0], SAND_RGB[1], SAND_RGB[2], 255]),
        Box::new(|_| [128, 128, 128, 255]),
        Box::new(|texel| {
            if texel % 2 == 0 {
                [LEAF_RGB[0], LEAF_RGB[1], LEAF_RGB[2], 255]
            } else {
                [0; 4]
            }
        }),
    ];
    TextureArray {
        layers: layers.len() as u32,
        mips: [16_u32, 8, 4, 2, 1]
            .into_iter()
            .map(|size| {
                let mut rgba8 = Vec::new();
                for layer in &layers {
                    for texel in 0..size * size {
                        rgba8.extend_from_slice(&layer(texel));
                    }
                }
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect(),
    }
}

fn material(layer: u32, flags: u32) -> Material {
    Material {
        texture: TextureRef::new(0, layer).unwrap(),
        flags,
        animation: NO_ANIMATION,
        ..Material::unvaried()
    }
}

pub fn runtime_assets() -> RuntimeAssets {
    let cube = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
    let visuals = vec![
        visual(
            VisualKind::Invisible,
            BlockFlags::AIR,
            DIAGNOSTIC_MATERIAL,
            0,
        ),
        visual(VisualKind::Cube, cube, 0, 0),
        visual(VisualKind::Cube, cube, 1, 0),
        visual(VisualKind::Liquid, BlockFlags::empty(), 2, 0),
        visual(VisualKind::Cross, BlockFlags::empty(), 1, 0),
        visual(
            VisualKind::Cube,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            DIAGNOSTIC_MATERIAL,
            assets::BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF | LEAF_MATERIAL_BASE,
        ),
    ];
    let mut materials = vec![
        material(0, MATERIAL_FLAG_GRASS_TINT),
        material(1, 0),
        material(2, MATERIAL_FLAG_WATER_TINT),
    ];
    materials.extend((0..6).map(|_| material(3, MATERIAL_FLAG_FOLIAGE_TINT)));
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::default(); visuals.len()]
            .into_boxed_slice(),
        hashed: (0..visuals.len() as u32)
            .map(|id| (0x10000 + id, id))
            .collect(),
        visuals: visuals.into_boxed_slice(),
        materials: materials.into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(textures())].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&assets::encode_blob(&compiled).expect("encode far-terrain assets"))
        .expect("decode far-terrain assets")
}

fn rgba(rgb: [f32; 3]) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], 1.0]
}

pub fn tints() -> ResolvedBiomeTints {
    ResolvedBiomeTints {
        skipped_definitions: 0,
        records: Box::new([LinearBiomeTints {
            raw_id: 0,
            flags: 0,
            grass: rgba(GRASS_TINT),
            foliage: rgba(FOLIAGE_TINT),
            birch: rgba(FOLIAGE_TINT),
            evergreen: rgba(FOLIAGE_TINT),
            water: rgba(WATER_TINT),
            dry_foliage: rgba(FOLIAGE_TINT),
            seasonal_foliage: [[1.0; 4]; assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT],
        }]),
        biomes: Box::new([None]),
        swamp_grass_palette: Box::new([]),
        raw_id_to_dense: Box::new([0]),
    }
}

pub fn context() -> SurfaceContext {
    let assets = Arc::new(runtime_assets());
    SurfaceContext {
        colours: Arc::new(SurfaceColours::new(&assets)),
        assets,
        mode: NetworkIdMode::Sequential,
        tints: Arc::new(tints()),
        default_biome: 0,
    }
}

fn zig_zag(value: u32) -> Vec<u8> {
    let mut value = (value << 1) ^ ((value as i32 >> 31) as u32);
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            return bytes;
        }
        bytes.push(byte | 0x80);
    }
}

/// One v9 network sub-chunk whose two layers are `block(x, y, z)`.
pub fn section(block: impl Fn(u8, u8, u8) -> [u32; 2]) -> Arc<SubChunk> {
    let mut bytes = vec![9, 2, 0];
    for layer in 0..2 {
        let mut palette = Vec::<u32>::new();
        let mut indices = [0_u32; 4096];
        for (linear, index) in indices.iter_mut().enumerate() {
            let (x, z, y) = (
                (linear >> 8) as u8,
                ((linear >> 4) & 15) as u8,
                (linear & 15) as u8,
            );
            let id = block(x, y, z)[layer];
            *index = palette
                .iter()
                .position(|&known| known == id)
                .unwrap_or_else(|| {
                    palette.push(id);
                    palette.len() - 1
                }) as u32;
        }
        bytes.push((8 << 1) | 1);
        for word in indices.chunks_exact(4) {
            let packed = word
                .iter()
                .enumerate()
                .fold(0_u32, |packed, (slot, &index)| {
                    packed | (index << (slot * 8))
                });
            bytes.extend_from_slice(&packed.to_le_bytes());
        }
        bytes.extend(zig_zag(palette.len() as u32));
        for id in palette {
            bytes.extend(zig_zag(id));
        }
    }
    Arc::new(SubChunk::decode(&bytes, &RawBlockIds { air: AIR }))
}

pub fn column(sections: impl IntoIterator<Item = (i32, Arc<SubChunk>)>) -> ColumnSnapshot {
    ColumnSnapshot::new(
        Arc::new(sections.into_iter().collect::<BTreeMap<_, _>>()),
        Vec::new(),
    )
}
