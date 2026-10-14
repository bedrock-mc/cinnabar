use std::{collections::HashMap, sync::RwLock};

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, Material, ModelQuad, ModelTemplate, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, TextureArray, TextureMip, TexturePage, VisualKind,
    encode_blob,
};
use lod::registry::{MaterialKey, MaterialRegistry};

use super::*;

/// Sequential ids of [`fixture_assets`].
const STONE: u32 = 1;
const GRASS: u32 = 2;
const LEAVES: u32 = 3;
const WATER: u32 = 4;
const FLOWER: u32 = 5;

const SIDE: u32 = 16;
const STONE_RGB: [u8; 3] = [128, 128, 128];
const GRASS_TOP_RGB: [u8; 3] = [200, 200, 200];
const DIRT_RGB: [u8; 3] = [120, 80, 50];
const OVERLAY_RGB: [u8; 3] = [150, 150, 150];
const LEAF_RGB: [u8; 3] = [60, 160, 60];
const WATER_RGB: [u8; 3] = [255, 255, 255];

/// One base layer whose texel `(x, y)` is `texel(x, y)`.
fn layer(texel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..SIDE * SIDE)
        .flat_map(|index| texel(index % SIDE, index / SIDE))
        .collect()
}

fn opaque(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255]
}

/// Stone, a grass block with a tinted top and overlay-masked sides, cutout leaves,
/// water and a flower model whose quads use the stone texture.
fn fixture_assets() -> RuntimeAssets {
    let layers = [
        layer(|_, _| opaque(STONE_RGB)),
        layer(|_, _| opaque(GRASS_TOP_RGB)),
        layer(|_, _| opaque(DIRT_RGB)),
        // The top half is the tinted overlay; the bottom half's alpha-zero texels are the
        // dirt the overlay sits on.
        layer(|_, y| {
            if y < SIDE / 2 {
                opaque(OVERLAY_RGB)
            } else {
                [DIRT_RGB[0], DIRT_RGB[1], DIRT_RGB[2], 0]
            }
        }),
        layer(|x, y| {
            if (x + y) % 2 == 0 {
                opaque(LEAF_RGB)
            } else {
                [0, 0, 0, 0]
            }
        }),
        layer(|_, _| [WATER_RGB[0], WATER_RGB[1], WATER_RGB[2], 180]),
    ];
    let mut size = SIDE;
    let mut mips = Vec::new();
    loop {
        let rgba8 = if size == SIDE {
            layers.concat()
        } else {
            vec![0xff; (size * size * 4) as usize * layers.len()]
        };
        mips.push(TextureMip {
            size,
            rgba8: rgba8.into_boxed_slice(),
        });
        if size == 1 {
            break;
        }
        size /= 2;
    }
    let texture = |layer: u32| TextureRef::new(0, layer).expect("fixture layer");
    let material = |layer: Option<u32>, flags: u32| Material {
        texture: layer.map_or(TextureRef::DIAGNOSTIC, texture),
        flags,
        animation: NO_ANIMATION,
        ..Material::unvaried()
    };
    let materials = vec![
        material(None, 0),
        material(Some(0), 0),
        material(Some(1), MATERIAL_FLAG_GRASS_TINT),
        material(Some(2), 0),
        material(
            Some(3),
            MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK,
        ),
        material(
            Some(4),
            MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_ALPHA_CUTOUT,
        ),
        material(
            Some(5),
            MATERIAL_FLAG_WATER_TINT | MATERIAL_FLAG_ALPHA_BLEND,
        ),
    ];
    let visual = |kind, flags, faces, model_template| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces,
        flags,
        kind,
        contributor_role: assets::ContributorRole::Primary,
        model_template,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let cube = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
    let visuals = vec![
        BlockVisual {
            contributor_role: assets::ContributorRole::Air,
            ..visual(
                VisualKind::Invisible,
                BlockFlags::AIR,
                [0; 6],
                NO_MODEL_TEMPLATE,
            )
        },
        visual(VisualKind::Cube, cube, [1; 6], NO_MODEL_TEMPLATE),
        // West, east, down, up, north, south.
        visual(
            VisualKind::Cube,
            cube,
            [4, 4, 3, 2, 4, 4],
            NO_MODEL_TEMPLATE,
        ),
        visual(
            VisualKind::Cube,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            [5; 6],
            NO_MODEL_TEMPLATE,
        ),
        BlockVisual {
            contributor_role: assets::ContributorRole::LiquidAdditional,
            ..visual(
                VisualKind::Liquid,
                BlockFlags::empty(),
                [6; 6],
                NO_MODEL_TEMPLATE,
            )
        },
        visual(VisualKind::Model, BlockFlags::empty(), [0; 6], 0),
    ];
    let quad = ModelQuad {
        positions: [[0, 0, 128], [256, 0, 128], [256, 256, 128], [0, 256, 128]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 1,
        flags: 0,
    };
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::default(); visuals.len()]
            .into_boxed_slice(),
        visuals: visuals.into_boxed_slice(),
        hashed: Box::new([]),
        materials: materials.into_boxed_slice(),
        model_templates: Box::new([ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }]),
        model_quads: Box::new([quad]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: layers.len() as u32,
            mips: mips.into_boxed_slice(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).expect("encode")).expect("decode")
}

/// A registry holding `ids` of [`fixture_assets`] in order, and the session identities.
fn registry_of(ids: &[u32]) -> (Arc<SessionBlocks>, SharedRegistry) {
    let blocks = Arc::new(SessionBlocks::new(
        Arc::new(fixture_assets()),
        NetworkIdMode::Sequential,
        Arc::new(HashMap::new()),
    ));
    let mut registry = MaterialRegistry::new();
    for &id in ids {
        let hash = assets::pinned_block_network_hash(id).expect("pinned vanilla id");
        registry
            .get_or_insert(&MaterialKey::Network(hash))
            .expect("room");
    }
    (blocks, Arc::new(RwLock::new(registry)))
}

fn material(index: u32) -> MaterialId {
    MaterialId::new(MaterialId::FIRST_BLOCK.raw() + index).expect("material")
}

fn assert_linear_near(colour: FaceColour, expected: [f32; 3]) {
    let linear = colour.linear();
    for (got, want) in linear.iter().zip(expected) {
        assert!((got - want).abs() < 0.01, "{linear:?} against {expected:?}");
    }
}

#[test]
fn materials_take_their_faces_average_texture_colour_and_tint_class() {
    let (blocks, registry) = registry_of(&[STONE, GRASS, LEAVES, WATER, FLOWER]);
    let mut builder = ColourTableBuilder::default();
    assert!(builder.refresh(&blocks, &registry));
    let table = Arc::clone(builder.table());
    assert_eq!(table.len(), 6, "air and five materials");

    let stone = table.get(material(0), Face::NegX);
    assert_eq!(stone.srgb(), STONE_RGB);
    assert_eq!(stone.tint(), TintClass::None);

    let grass_top = table.get(material(1), Face::PosY);
    assert_eq!(grass_top.srgb(), GRASS_TOP_RGB);
    assert_eq!(
        (grass_top.tint(), grass_top.tint_weight()),
        (TintClass::Grass, 1.0)
    );
    assert_eq!(table.get(material(1), Face::NegY).srgb(), DIRT_RGB);
    assert_eq!(table.get(material(1), Face::NegY).tint(), TintClass::None);
    // Half overlay, half dirt: every texel counts and only the overlay's share is tinted.
    let side = table.get(material(1), Face::PosZ);
    let half = |a: u8, b: u8| (decode_srgb(a) + decode_srgb(b)) / 2.0;
    assert_linear_near(
        side,
        std::array::from_fn(|channel| half(OVERLAY_RGB[channel], DIRT_RGB[channel])),
    );
    assert_eq!(side.tint(), TintClass::Grass);
    assert!(side.tint_weight() > 0.5 && side.tint_weight() < 1.0);

    // Cutout gaps never darken leaves.
    let leaves = table.get(material(2), Face::PosY);
    assert_eq!(leaves.srgb(), LEAF_RGB);
    assert_eq!(leaves.tint(), TintClass::Foliage);

    let water = table.get(material(3), Face::PosY);
    assert_eq!(water.srgb(), WATER_RGB);
    assert_eq!(water.tint(), TintClass::Water);

    // A model block without face materials takes its quads' texture.
    assert_eq!(table.get(material(4), Face::PosX).srgb(), STONE_RGB);
    assert_eq!(
        table.get(MaterialId::HIDDEN, Face::PosY),
        FaceColour::HIDDEN
    );
}

#[test]
fn tables_are_deterministic_and_grow_with_the_registry() {
    let (blocks, registry) = registry_of(&[GRASS, STONE]);
    let mut first = ColourTableBuilder::default();
    first.refresh(&blocks, &registry);
    let mut second = ColourTableBuilder::default();
    second.refresh(&blocks, &registry);
    assert_eq!(first.table(), second.table());
    let words: Vec<u32> = first
        .table()
        .entries()
        .iter()
        .flatten()
        .map(|colour| colour.bits())
        .collect();
    assert_eq!(words.len(), 3 * 6);

    assert!(!first.refresh(&blocks, &registry), "nothing new");
    let hash = assets::pinned_block_network_hash(LEAVES).expect("pinned");
    registry
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(hash))
        .expect("room");
    assert!(first.refresh(&blocks, &registry));
    assert_eq!(first.table().len(), 4);
    assert_eq!(&first.table().entries()[..3], second.table().entries());
}

#[test]
fn unknown_keys_take_the_unknown_colour() {
    let (blocks, registry) = registry_of(&[STONE]);
    registry
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(0x1234_5678))
        .expect("room");
    let mut builder = ColourTableBuilder::default();
    builder.refresh(&blocks, &registry);
    assert_eq!(
        builder.table().get(material(1), Face::PosY),
        FaceColour::UNKNOWN
    );
}

#[test]
fn section_faces_name_the_same_asset_faces_by_index() {
    for face in Face::ALL {
        assert_eq!(block_face(face) as usize, face.index());
    }
}

#[test]
fn biome_tints_follow_the_resolved_records() {
    let resolved = CompiledBiomeAssets::diagnostic()
        .resolve_live(&[])
        .expect("diagnostic biomes resolve");
    let tints = biome_tints(&resolved);
    assert_eq!(tints.records().len(), resolved.records.len().max(1));
    for raw in [0_u32, 1, 4, 300] {
        let record = &resolved.records[resolved.dense_index(raw) as usize];
        let rgb = |colour: [f32; 4]| [colour[0], colour[1], colour[2]];
        assert_eq!(tints.tint(raw, TintClass::Grass), rgb(record.grass));
        assert_eq!(tints.tint(raw, TintClass::Water), rgb(record.water));
        assert_eq!(tints.tint(raw, TintClass::None), [1.0; 3]);
    }
}

#[test]
fn tint_classes_follow_the_material_flags() {
    for (flags, class) in [
        (0, TintClass::None),
        (MATERIAL_FLAG_GRASS_TINT, TintClass::Grass),
        (MATERIAL_FLAG_FOLIAGE_TINT, TintClass::Foliage),
        (
            MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
            TintClass::Birch,
        ),
        (
            MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
            TintClass::Evergreen,
        ),
        (
            MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE,
            TintClass::DryFoliage,
        ),
        (MATERIAL_FLAG_WATER_TINT, TintClass::Water),
    ] {
        assert_eq!(tint_class(flags), class, "{flags:#x}");
    }
}
