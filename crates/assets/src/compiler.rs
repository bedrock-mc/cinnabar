use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    Animation, AnimationInventory, AssetError, BiomeRegistryRecord, BlockFace, BlockFlags,
    CompiledBiomeAssets, ContributorRole, MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_STAIR, ModelFamily, ModelQuad, ModelStateField, ModelTemplate,
    NO_ANIMATION, NO_MODEL_TEMPLATE, PackSources, RegistryRecord, TextureKey, TexturePage,
    TextureRef, VisualKind,
    animation::{
        AnimationLimits, AnimationPlan, DecodedImage, compile_animation_plan,
        compile_animation_plan_selected,
    },
    compile_biome_assets,
    image::{TextureArray, decode_static_texture, decode_texture},
    read_pack, resolve_texture_key,
};

pub const DIAGNOSTIC_MATERIAL: u32 = 0;
pub const MAX_TEXTURE_LAYERS: usize = 2_048;
pub const MAX_MATERIALS: usize = 65_536;
pub const MATERIAL_FLAG_ROTATE_UV: u32 = 1 << 0;
pub const MATERIAL_FLAG_UV_MASK: u32 = 0x0000_000f;
pub const MATERIAL_FLAG_TINT_MASK: u32 = 0x0000_0030;
pub const MATERIAL_FLAG_GRASS_TINT: u32 = 1 << 4;
pub const MATERIAL_FLAG_FOLIAGE_TINT: u32 = 1 << 5;
pub const MATERIAL_FLAG_WATER_TINT: u32 = MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_FOLIAGE_TINT;
pub const MATERIAL_FLAG_OVERLAY_MASK: u32 = 1 << 6;
pub const MATERIAL_FLAG_ALPHA_BLEND: u32 = 1 << 7;
pub const MATERIAL_FLAG_ALPHA_CUTOUT: u32 = 1 << 8;
pub const MATERIAL_FLAG_FOLIAGE_CLASS_MASK: u32 = 0x0000_0600;
pub const MATERIAL_FLAG_BIRCH_FOLIAGE: u32 = 1 << 9;
pub const MATERIAL_FLAG_EVERGREEN_FOLIAGE: u32 = 1 << 10;
pub const MATERIAL_FLAG_DRY_FOLIAGE: u32 = MATERIAL_FLAG_FOLIAGE_CLASS_MASK;
/// Selects the opaque, depth-writing liquid pipeline used by lava.
pub const MATERIAL_FLAG_LIQUID_DEPTH_WRITE: u32 = 1 << 11;
pub const MATERIAL_FLAGS_MASK: u32 = MATERIAL_FLAG_UV_MASK
    | MATERIAL_FLAG_TINT_MASK
    | MATERIAL_FLAG_OVERLAY_MASK
    | MATERIAL_FLAG_ALPHA_BLEND
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_FOLIAGE_CLASS_MASK
    | MATERIAL_FLAG_LIQUID_DEPTH_WRITE;

pub(crate) const fn material_flags_are_valid(flags: u32) -> bool {
    flags & !MATERIAL_FLAGS_MASK == 0
        && flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
            != MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT
        && (flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK == 0
            || flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT)
        && (flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE == 0
            || flags
                & (MATERIAL_FLAG_ALPHA_BLEND
                    | MATERIAL_FLAG_ALPHA_CUTOUT
                    | MATERIAL_FLAG_TINT_MASK)
                == 0)
}

const MAX_VISUALS: usize = 65_536;

/// One immutable GPU material-table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Material {
    pub texture: TextureRef,
    pub flags: u32,
    pub animation: u32,
}

const _: () = assert!(std::mem::size_of::<Material>() == 12);

/// Per-face material IDs and registry facts for one sequential block ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockVisual {
    pub faces: [u32; 6],
    pub flags: BlockFlags,
    pub kind: VisualKind,
    pub contributor_role: ContributorRole,
    pub model_template: u32,
    pub animation: u32,
    pub variant: u32,
}

impl BlockVisual {
    fn diagnostic(flags: BlockFlags, contributor_role: ContributorRole) -> Self {
        Self {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags,
            kind: VisualKind::Diagnostic,
            contributor_role,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }
    }
}

pub(crate) fn visual_semantics_are_valid(
    kind: VisualKind,
    flags: BlockFlags,
    role: ContributorRole,
) -> bool {
    if flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        && !flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !matches!(kind, VisualKind::Model)
    {
        return false;
    }
    match kind {
        VisualKind::Diagnostic => true,
        VisualKind::Cube => {
            matches!(role, ContributorRole::Primary) && flags.contains(BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Cross | VisualKind::Model => {
            matches!(role, ContributorRole::Primary)
                && !flags.intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Liquid => {
            matches!(role, ContributorRole::LiquidAdditional)
                && !flags.intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Invisible => {
            !matches!(role, ContributorRole::LiquidAdditional)
                && !flags.contains(BlockFlags::CUBE_GEOMETRY)
                && (matches!(role, ContributorRole::Air) == flags.contains(BlockFlags::AIR))
        }
    }
}

/// Deterministic compiler output ready for checked blob serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledAssets {
    pub visuals: Box<[BlockVisual]>,
    pub hashed: Box<[(u32, u32)]>,
    pub materials: Box<[Material]>,
    pub model_templates: Box<[ModelTemplate]>,
    pub model_quads: Box<[ModelQuad]>,
    pub animations: Box<[Animation]>,
    pub animation_frames: Box<[TextureRef]>,
    pub texture_pages: Box<[TexturePage]>,
    pub biomes: CompiledBiomeAssets,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Descriptor {
    path: Box<str>,
    texture_key: Box<str>,
    flags: u32,
}

type CompiledMaterials = (Box<[Material]>, BTreeMap<Descriptor, u32>);
type CompiledVisuals = (
    Box<[BlockVisual]>,
    Box<[(u32, u32)]>,
    Box<[ModelTemplate]>,
    Box<[ModelQuad]>,
);
type CompiledAnimations = (Box<[Animation]>, Box<[TextureRef]>);

/// Compiles the cube-geometry subset of a bounded Bedrock resource pack.
pub fn compile_pack(root: &Path, records: &[RegistryRecord]) -> Result<CompiledAssets, AssetError> {
    compile_pack_inner(root, records, CompiledBiomeAssets::diagnostic())
}

/// Compiles the complete v3 block and biome asset set.
pub fn compile_pack_with_biomes(
    root: &Path,
    behavior_pack: &Path,
    records: &[RegistryRecord],
    biome_registry: &[BiomeRegistryRecord],
) -> Result<CompiledAssets, AssetError> {
    let biomes = compile_biome_assets(root, behavior_pack, biome_registry)?;
    compile_pack_inner(root, records, biomes)
}

/// Reads and compiles a bounded animation staging plan without changing the
/// runtime asset schema or installing animation layers into [`CompiledAssets`].
pub fn inspect_animation_inventory(
    root: &Path,
    max_layers_per_page: u32,
    max_pages: u32,
) -> Result<AnimationInventory, AssetError> {
    let pack = read_pack(root)?;
    let mut source_paths = pack
        .terrain
        .source_paths()
        .chain(
            pack.flipbooks
                .iter()
                .map(|flipbook| flipbook.texture_path.as_ref()),
        )
        .map(Box::<str>::from)
        .collect::<BTreeSet<_>>();
    let mut decoded_images = Vec::with_capacity(source_paths.len());
    for source_path in std::mem::take(&mut source_paths) {
        let path = static_texture_path(root, &source_path, &source_path)?;
        if !path.try_exists().map_err(|source| AssetError::TextureIo {
            key: source_path.clone(),
            path: path.clone(),
            source,
        })? {
            continue;
        }
        let decoded = decode_texture(&path, &source_path)?;
        decoded_images.push(DecodedImage {
            source_path,
            width: decoded.width,
            height: decoded.height,
            rgba8: decoded.rgba8,
        });
    }
    let plan = compile_animation_plan(
        &pack,
        &decoded_images,
        AnimationLimits {
            max_layers_per_page,
            max_pages,
        },
    )?;
    Ok(plan.inventory)
}

fn compile_pack_inner(
    root: &Path,
    records: &[RegistryRecord],
    biomes: CompiledBiomeAssets,
) -> Result<CompiledAssets, AssetError> {
    let pack = read_pack(root)?;
    validate_records(records)?;

    let mut descriptor_keys = BTreeMap::<Descriptor, Box<str>>::new();
    for record in records.iter().filter(|record| {
        (record.flags.contains(BlockFlags::CUBE_GEOMETRY)
            && !record_has_deferred_material(&pack, record))
            || is_model_visual(record)
            || is_liquid(record)
    }) {
        if is_flowerbed(record) {
            if let Some(descriptors) = flowerbed_material_descriptors(&pack, record) {
                for (descriptor, key) in descriptors {
                    descriptor_keys
                        .entry(descriptor)
                        .and_modify(|current| {
                            if key.as_ref() < current.as_ref() {
                                *current = key.clone();
                            }
                        })
                        .or_insert(key);
                }
            }
            continue;
        }
        let aquatic_faces;
        let faces: &[BlockFace] = if is_kelp(record) || is_liquid(record) {
            &BlockFace::ALL
        } else if is_aquatic_cross(record) {
            aquatic_faces = aquatic_cross_faces(record).unwrap_or([BlockFace::Up; 2]);
            &aquatic_faces
        } else if is_terrestrial_cross(record) {
            &[cross_texture_face(record)]
        } else {
            &BlockFace::ALL
        };
        for &face in faces {
            if let Some((descriptor, key)) = descriptor_for(&pack, record, face) {
                descriptor_keys
                    .entry(descriptor)
                    .and_modify(|current| {
                        if key.as_ref() < current.as_ref() {
                            *current = key.clone();
                        }
                    })
                    .or_insert(key);
            }
        }
    }

    let animation_plan = compile_runtime_animation_plan(root, &pack, &descriptor_keys)?;
    let texture_pages = animation_plan_pages(&animation_plan)?;
    let (animations, animation_frames) = runtime_animation_tables(&animation_plan)?;
    let (materials, material_by_descriptor) = compile_materials(&descriptor_keys, &animation_plan)?;
    let (visuals, hashed, model_templates, model_quads) =
        compile_visuals(records, &pack, &material_by_descriptor)?;

    Ok(CompiledAssets {
        visuals,
        hashed,
        materials,
        model_templates,
        model_quads,
        animations,
        animation_frames,
        texture_pages,
        biomes,
    })
}

fn validate_records(records: &[RegistryRecord]) -> Result<(), AssetError> {
    if records.len() > MAX_VISUALS {
        return Err(AssetError::TooManyRegistryRecords {
            count: records.len(),
            max: MAX_VISUALS,
        });
    }

    let mut sequential = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    for record in records {
        let flags_are_valid =
            BlockFlags::from_bits(record.flags.bits()).is_some_and(BlockFlags::has_valid_semantics);
        if !flags_are_valid {
            return Err(AssetError::InvalidCompiledAssets {
                detail: format!(
                    "registry record {} has invalid block flags {:#04x}",
                    record.sequential_id,
                    record.flags.bits()
                )
                .into(),
            });
        }
        if record.sequential_id as usize >= MAX_VISUALS {
            return Err(AssetError::SequentialIdOutOfRange {
                id: record.sequential_id,
                max: MAX_VISUALS - 1,
            });
        }
        if !sequential.insert(record.sequential_id) {
            return Err(AssetError::DuplicateSequentialId(record.sequential_id));
        }
        if !hashes.insert(record.network_hash) {
            return Err(AssetError::DuplicateNetworkHash(record.network_hash));
        }
    }
    Ok(())
}

fn descriptor_for(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    let path = if is_cutout_model_visual(record) {
        pack.terrain.get_for_model_record(&key, record)?.0
    } else {
        pack.terrain.get_for_record(&key, record)?
    };
    if !is_model_visual(record)
        && !is_liquid(record)
        && source_is_deferred(pack, record, &key, path)
    {
        return None;
    }
    let mut flags = if rotate_uv {
        MATERIAL_FLAG_ROTATE_UV
    } else {
        0
    };
    if is_cutout_model_visual(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT | cutout_model_tint_flags(&record.name);
    } else if is_liquid(record) {
        flags |= liquid_material_flags(&record.name);
    } else if record.flags.contains(BlockFlags::LEAF_MODEL) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
        flags |= leaf_tint_flags(&record.name);
    }
    if record.name.as_ref() == "minecraft:glass" {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    }
    if record.name.as_ref() == "minecraft:grass_block" {
        flags |= match face {
            BlockFace::Down => 0,
            BlockFace::Up => MATERIAL_FLAG_GRASS_TINT,
            BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South => {
                MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
            }
        };
    }
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            flags,
        },
        key,
    ))
}

fn flowerbed_material_descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<[(Descriptor, Box<str>); 2]> {
    let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, BlockFace::Down);
    let key = key?;
    let flags = (u32::from(rotate_uv) * MATERIAL_FLAG_ROTATE_UV) | MATERIAL_FLAG_ALPHA_CUTOUT;
    let paths = pack.terrain.get_exact_pair(&key)?;
    Some(paths.map(|path| {
        (
            Descriptor {
                path: path.into(),
                texture_key: key.clone(),
                flags,
            },
            key.clone(),
        )
    }))
}

const fn is_terrestrial_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Cross | ModelFamily::Crop)
}

fn is_aquatic_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Aquatic)
        && record.name.as_ref() == "minecraft:seagrass"
}

fn is_cross_visual(record: &RegistryRecord) -> bool {
    is_terrestrial_cross(record) || is_aquatic_cross(record)
}

fn is_kelp(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Aquatic) && record.name.as_ref() == "minecraft:kelp"
}

fn is_flowerbed(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::FlowerBed)
        && matches!(
            record.name.as_ref(),
            "minecraft:wildflowers" | "minecraft:pink_petals"
        )
}

fn is_vine(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Vine) && record.name.as_ref() == "minecraft:vine"
}

const fn is_door(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Door)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

const fn is_trapdoor(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Trapdoor)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

const fn is_slab(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Slab)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

const fn is_stair(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Stair)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

fn is_cutout_model_visual(record: &RegistryRecord) -> bool {
    is_cross_visual(record)
        || is_kelp(record)
        || is_flowerbed(record)
        || is_vine(record)
        || is_door(record)
        || is_trapdoor(record)
}

fn is_model_visual(record: &RegistryRecord) -> bool {
    is_cutout_model_visual(record) || is_slab(record) || is_stair(record)
}

const fn is_liquid(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Liquid)
        && matches!(record.contributor_role, ContributorRole::LiquidAdditional)
}

fn is_supported_liquid(record: &RegistryRecord) -> bool {
    is_liquid(record)
        && matches!(
            record.name.as_ref(),
            "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
        )
}

const fn liquid_material_flags(name: &str) -> u32 {
    match name.as_bytes() {
        b"minecraft:water" | b"minecraft:flowing_water" => {
            MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
        }
        b"minecraft:lava" | b"minecraft:flowing_lava" => MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
        _ => 0,
    }
}

fn cross_texture_face(record: &RegistryRecord) -> BlockFace {
    if canonical_state_u32(&record.canonical_state, "upper_block_bit") == Some(1) {
        BlockFace::Up
    } else {
        BlockFace::Down
    }
}

fn canonical_state_u32(state: &str, property: &str) -> Option<u32> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

fn canonical_state_str(state: &str, property: &str) -> Option<Box<str>> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_str()
        .map(Into::into)
}

fn aquatic_cross_faces(record: &RegistryRecord) -> Option<[BlockFace; 2]> {
    match record.name.as_ref() {
        "minecraft:seagrass" => {
            match canonical_state_str(&record.canonical_state, "sea_grass_type")?.as_ref() {
                "default" => Some([BlockFace::Up, BlockFace::Up]),
                "double_bot" => Some([BlockFace::Down, BlockFace::South]),
                "double_top" => Some([BlockFace::East, BlockFace::West]),
                _ => None,
            }
        }
        _ => None,
    }
}

fn cutout_model_tint_flags(name: &str) -> u32 {
    match name {
        "minecraft:short_grass"
        | "minecraft:tall_grass"
        | "minecraft:fern"
        | "minecraft:large_fern" => MATERIAL_FLAG_GRASS_TINT,
        "minecraft:vine" => MATERIAL_FLAG_FOLIAGE_TINT,
        _ => 0,
    }
}

fn leaf_tint_flags(name: &str) -> u32 {
    match name {
        "minecraft:oak_leaves"
        | "minecraft:dark_oak_leaves"
        | "minecraft:jungle_leaves"
        | "minecraft:acacia_leaves"
        | "minecraft:mangrove_leaves" => MATERIAL_FLAG_FOLIAGE_TINT,
        "minecraft:birch_leaves" => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
        "minecraft:spruce_leaves" => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        _ => 0,
    }
}

fn record_has_deferred_material(pack: &PackSources, record: &RegistryRecord) -> bool {
    BlockFace::ALL.into_iter().any(|face| {
        let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
        let Some(key) = key else {
            return false;
        };
        let Some(path) = pack.terrain.get_for_record(&key, record) else {
            return false;
        };
        source_is_deferred(pack, record, &key, path)
    })
}

fn source_is_deferred(pack: &PackSources, record: &RegistryRecord, key: &str, _path: &str) -> bool {
    record.name.as_ref() != "minecraft:grass_block" && pack.terrain.requires_tint(key)
}

fn compile_runtime_animation_plan(
    root: &Path,
    pack: &PackSources,
    descriptor_keys: &BTreeMap<Descriptor, Box<str>>,
) -> Result<AnimationPlan, AssetError> {
    let referenced_paths = descriptor_keys
        .keys()
        .map(|descriptor| descriptor.path.clone())
        .collect::<BTreeSet<_>>();
    let selected_atlas_tiles = pack
        .flipbooks
        .iter()
        .filter(|flipbook| {
            descriptor_keys
                .values()
                .any(|key| flipbook.atlas_tile.as_ref() == key.as_ref())
                || referenced_paths.contains(&flipbook.texture_path)
        })
        .map(|flipbook| flipbook.atlas_tile.clone())
        .collect::<BTreeSet<_>>();
    let animation_paths = pack
        .flipbooks
        .iter()
        .filter(|flipbook| selected_atlas_tiles.contains(&flipbook.atlas_tile))
        .map(|flipbook| flipbook.texture_path.clone())
        .collect::<BTreeSet<_>>();
    let mut decoded_images = Vec::new();
    let mut decoded_paths = BTreeSet::new();
    for descriptor in descriptor_keys
        .keys()
        .filter(|descriptor| !animation_paths.contains(&descriptor.path))
    {
        if !decoded_paths.insert(descriptor.path.clone()) {
            continue;
        }
        let source_path = descriptor.path.clone();
        let key = descriptor_keys
            .iter()
            .filter(|(candidate, _)| candidate.path == source_path)
            .map(|(_, key)| key)
            .min()
            .expect("descriptor path has a source key");
        let path = static_texture_path(root, &source_path, key)?;
        let rgba8 = decode_static_texture(&path, key)?;
        let has_alpha = rgba8.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX);
        let supports_alpha = descriptor_keys
            .keys()
            .filter(|candidate| candidate.path == source_path)
            .any(|candidate| {
                candidate.flags & (MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_OVERLAY_MASK) != 0
            });
        if has_alpha && !supports_alpha {
            continue;
        }
        decoded_images.push(DecodedImage {
            source_path,
            width: crate::TILE_SIZE,
            height: crate::TILE_SIZE,
            rgba8,
        });
    }
    for source_path in animation_paths {
        if !decoded_paths.insert(source_path.clone()) {
            continue;
        }
        let path = static_texture_path(root, &source_path, &source_path)?;
        let decoded = decode_texture(&path, &source_path)?;
        decoded_images.push(DecodedImage {
            source_path,
            width: decoded.width,
            height: decoded.height,
            rgba8: decoded.rgba8,
        });
    }
    compile_animation_plan_selected(
        pack,
        &decoded_images,
        AnimationLimits {
            max_layers_per_page: MAX_TEXTURE_LAYERS as u32,
            max_pages: 2,
        },
        Some(&selected_atlas_tiles),
    )
}

fn animation_plan_pages(plan: &AnimationPlan) -> Result<Box<[TexturePage]>, AssetError> {
    let mut pages = Vec::new();
    for chunk in plan.layers.chunks(MAX_TEXTURE_LAYERS) {
        let mut mips = Vec::with_capacity(crate::MIP_COUNT as usize);
        for level in 0..crate::MIP_COUNT as usize {
            let size = crate::TILE_SIZE >> level;
            let mut rgba8 = Vec::new();
            for layer in chunk {
                let mip =
                    layer
                        .mips
                        .get(level)
                        .ok_or_else(|| AssetError::InvalidCompiledAssets {
                            detail: "animation layer has a noncanonical mip count".into(),
                        })?;
                if mip.size != size {
                    return Err(AssetError::InvalidCompiledAssets {
                        detail: "animation layer has a noncanonical mip size".into(),
                    });
                }
                rgba8.extend_from_slice(&mip.rgba8);
            }
            mips.push(crate::TextureMip {
                size,
                rgba8: rgba8.into_boxed_slice(),
            });
        }
        pages.push(TexturePage::new(TextureArray {
            layers: u32::try_from(chunk.len()).map_err(|_| AssetError::BlobSizeOverflow {
                section: "texture page layers",
            })?,
            mips: mips.into_boxed_slice(),
        }));
    }
    Ok(pages.into_boxed_slice())
}

fn runtime_animation_tables(plan: &AnimationPlan) -> Result<CompiledAnimations, AssetError> {
    let animations = plan
        .animations
        .iter()
        .map(|source| Animation {
            frame_start: source.frame_start,
            frame_count: source.frame_count,
            ticks_per_frame: source.ticks_per_frame,
            atlas_index: source.atlas_index,
            atlas_tile_variant: source.atlas_tile_variant,
            replicate: source.replicate,
            flags: u32::from(source.blend_frames),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok((animations, plan.frames.clone()))
}

fn static_texture_path(root: &Path, source: &str, key: &str) -> Result<PathBuf, AssetError> {
    let source_path = Path::new(source);
    if source_path.extension().is_some() {
        return Ok(root.join(source_path));
    }

    let png = root.join(format!("{source}.png"));
    if png.try_exists().map_err(|source| AssetError::TextureIo {
        key: key.into(),
        path: png.clone(),
        source,
    })? {
        return Ok(png);
    }
    let tga = root.join(format!("{source}.tga"));
    if tga.try_exists().map_err(|source| AssetError::TextureIo {
        key: key.into(),
        path: tga.clone(),
        source,
    })? {
        return Ok(tga);
    }
    Ok(png)
}

fn compile_materials(
    descriptor_keys: &BTreeMap<Descriptor, Box<str>>,
    plan: &AnimationPlan,
) -> Result<CompiledMaterials, AssetError> {
    let mut materials = vec![Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: NO_ANIMATION,
    }];
    let mut material_by_value = BTreeMap::<(TextureRef, u32, u32), u32>::new();
    material_by_value.insert(
        (TextureRef::DIAGNOSTIC, 0, NO_ANIMATION),
        DIAGNOSTIC_MATERIAL,
    );
    let mut material_by_descriptor = BTreeMap::new();

    for descriptor in descriptor_keys.keys() {
        let key = &descriptor.texture_key;
        let candidates = plan
            .animations
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                source.atlas_tile.as_ref() == key.as_ref() && source.source_path == descriptor.path
            })
            .collect::<Vec<_>>();
        let animation = match candidates.as_slice() {
            [] => None,
            [(index, _)] => Some(*index),
            _ => Some(
                candidates
                    .iter()
                    .find(|(_, source)| source.atlas_index == 0 && source.atlas_tile_variant == 0)
                    .map(|(index, _)| *index)
                    .ok_or_else(|| AssetError::InvalidCompiledAssets {
                        detail: format!(
                            "texture {} has multiple flipbook selectors and no canonical selector (0, 0)",
                            descriptor.path
                        )
                        .into(),
                    })?,
            ),
        }
        .map(|index| u32::try_from(index).expect("bounded flipbook count"));
        let texture = if let Some(animation) = animation {
            let source = &plan.animations[animation as usize];
            plan.frames[source.frame_start as usize]
        } else if let Some(&texture) = plan
            .static_refs
            .get(&descriptor.path)
            .or_else(|| plan.strip_first_refs.get(&descriptor.path))
        {
            texture
        } else {
            continue;
        };
        let animation = animation.unwrap_or(NO_ANIMATION);
        let value = (texture, descriptor.flags, animation);
        let material = if let Some(&material) = material_by_value.get(&value) {
            material
        } else {
            if materials.len() >= MAX_MATERIALS {
                return Err(AssetError::TooManyMaterials {
                    count: materials.len() + 1,
                    max: MAX_MATERIALS,
                });
            }
            let material =
                u32::try_from(materials.len()).map_err(|_| AssetError::BlobSizeOverflow {
                    section: "material",
                })?;
            materials.push(Material {
                texture,
                flags: descriptor.flags,
                animation,
            });
            material_by_value.insert(value, material);
            material
        };
        material_by_descriptor.insert(descriptor.clone(), material);
    }
    Ok((materials.into_boxed_slice(), material_by_descriptor))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CuboidTemplateKey {
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
}

fn intern_cuboid_template(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
    template_by_key: &mut BTreeMap<CuboidTemplateKey, u32>,
    model_templates: &mut Vec<ModelTemplate>,
    model_quads: &mut Vec<ModelQuad>,
) -> Result<u32, AssetError> {
    let key = CuboidTemplateKey {
        materials,
        min,
        max,
    };
    if let Some(&template) = template_by_key.get(&key) {
        return Ok(template);
    }
    let template =
        u32::try_from(model_templates.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model template",
        })?;
    let quad_start =
        u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model quad",
        })?;
    model_templates.push(ModelTemplate {
        quad_start,
        quad_count: 6,
        flags: 0,
    });
    model_quads.extend(cuboid_quads(materials, min, max));
    template_by_key.insert(key, template);
    Ok(template)
}

fn compile_visuals(
    records: &[RegistryRecord],
    pack: &PackSources,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
) -> Result<CompiledVisuals, AssetError> {
    let visual_count = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap_or(0);
    let mut visuals =
        vec![BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary); visual_count];
    let mut hashed = Vec::with_capacity(records.len());
    let mut model_templates = Vec::new();
    let mut model_quads = Vec::new();
    let mut template_by_material = BTreeMap::<[u32; 2], u32>::new();
    let mut kelp_template_by_material = BTreeMap::<[u32; 6], u32>::new();
    let mut flowerbed_template_by_key = BTreeMap::<[u32; 4], u32>::new();
    let mut slab_template_by_key = BTreeMap::<[u32; 7], u32>::new();
    let mut stair_template_by_key = BTreeMap::<[u32; 7], u32>::new();
    let mut vine_template_by_key = BTreeMap::<[u32; 2], u32>::new();
    let mut cuboid_template_by_key = BTreeMap::<CuboidTemplateKey, u32>::new();

    let mut ordered_records = records.iter().collect::<Vec<_>>();
    ordered_records.sort_unstable_by_key(|record| record.sequential_id);
    for record in ordered_records {
        let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
        if is_supported_liquid(record) {
            let materials = BlockFace::ALL.map(|face| {
                descriptor_for(pack, record, face)
                    .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
            });
            let liquid_depth = record
                .model_state
                .get(ModelStateField::LiquidDepth)
                .or_else(|| canonical_state_u32(&record.canonical_state, "liquid_depth"));
            if let [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ] = materials
                && let Some(liquid_depth) = liquid_depth.filter(|depth| *depth <= 15)
            {
                visual.flags.remove(
                    BlockFlags::AIR
                        | BlockFlags::CUBE_GEOMETRY
                        | BlockFlags::OCCLUDES_FULL_FACE
                        | BlockFlags::LEAF_MODEL,
                );
                visual.faces = [west, east, down, up, north, south];
                visual.kind = VisualKind::Liquid;
                visual.variant = liquid_depth;
            }
        } else if is_flowerbed(record) {
            let growth = record.model_state.get(ModelStateField::Growth);
            let orientation = record.model_state.get(ModelStateField::Orientation);
            let materials = flowerbed_material_descriptors(pack, record).map(|descriptors| {
                descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
            });
            if let (
                Some([Some(flower), Some(stem)]),
                Some(growth @ 0..=7),
                Some(orientation @ 0..=3),
            ) = (materials, growth, orientation)
            {
                const LAYOUT_BY_GROWTH: [u32; 8] = [0, 1, 2, 3, 3, 3, 3, 3];
                let layout = LAYOUT_BY_GROWTH[growth as usize];
                let key = [flower, stem, layout, orientation];
                let template = if let Some(&template) = flowerbed_template_by_key.get(&key) {
                    template
                } else {
                    let quads = flowerbed_quads([flower, stem], layout, orientation)?;
                    let template = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model quad",
                        }
                    })?;
                    let quad_count =
                        u32::try_from(quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
                            section: "model quad count",
                        })?;
                    model_templates.push(ModelTemplate {
                        quad_start,
                        quad_count,
                        flags: 0,
                    });
                    model_quads.extend(quads);
                    flowerbed_template_by_key.insert(key, template);
                    template
                };
                visual.faces = [flower; 6];
                visual.kind = VisualKind::Model;
                visual.model_template = template;
            }
        } else if is_vine(record) {
            let material = descriptor_for(pack, record, BlockFace::South)
                .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied());
            let connections = record.model_state.get(ModelStateField::Connections);
            if let (Some(material), Some(connections @ 0..=15)) = (material, connections) {
                let key = [material, connections];
                let template = if let Some(&template) = vine_template_by_key.get(&key) {
                    template
                } else {
                    let quads = vine_quads(material, connections);
                    let template = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model quad",
                        }
                    })?;
                    model_templates.push(ModelTemplate {
                        quad_start,
                        quad_count: connections.count_ones(),
                        flags: 0,
                    });
                    model_quads.extend(quads);
                    vine_template_by_key.insert(key, template);
                    template
                };
                visual.flags.remove(
                    BlockFlags::AIR
                        | BlockFlags::CUBE_GEOMETRY
                        | BlockFlags::OCCLUDES_FULL_FACE
                        | BlockFlags::LEAF_MODEL,
                );
                visual.faces = [material; 6];
                visual.kind = VisualKind::Model;
                visual.model_template = template;
            }
        } else if is_door(record) {
            const UPPER: u32 = 1 << 7;
            let orientation = record.model_state.get(ModelStateField::Orientation);
            let open = record.model_state.get(ModelStateField::Open);
            let hinge = record.model_state.get(ModelStateField::Hinge);
            let flags = record.model_state.get(ModelStateField::Flags);
            if let (Some(orientation @ 0..=3), Some(open @ 0..=1), Some(hinge @ 0..=1), Some(flags)) =
                (orientation, open, hinge, flags)
                && flags & !UPPER == 0
            {
                let texture_face = if flags & UPPER == 0 {
                    BlockFace::Down
                } else {
                    BlockFace::South
                };
                let material = descriptor_for(pack, record, texture_face)
                    .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied());
                if let Some(material) = material {
                    let materials = [material; 6];
                    let (min, max) = door_bounds(orientation, open, hinge);
                    let template = intern_cuboid_template(
                        materials,
                        min,
                        max,
                        &mut cuboid_template_by_key,
                        &mut model_templates,
                        &mut model_quads,
                    )?;
                    visual.flags.remove(
                        BlockFlags::AIR
                            | BlockFlags::CUBE_GEOMETRY
                            | BlockFlags::OCCLUDES_FULL_FACE
                            | BlockFlags::LEAF_MODEL,
                    );
                    visual.faces = materials;
                    visual.kind = VisualKind::Model;
                    visual.model_template = template;
                }
            }
        } else if is_trapdoor(record) {
            let materials = BlockFace::ALL.map(|face| {
                descriptor_for(pack, record, face)
                    .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
            });
            let orientation = record.model_state.get(ModelStateField::Orientation);
            let open = record.model_state.get(ModelStateField::Open);
            let half = record.model_state.get(ModelStateField::Half);
            if let [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ] = materials
                && let (Some(orientation @ 0..=3), Some(open @ 0..=1), Some(half @ 0..=1)) =
                    (orientation, open, half)
            {
                let materials = [west, east, down, up, north, south];
                let (min, max) = trapdoor_bounds(orientation, open, half);
                let template = intern_cuboid_template(
                    materials,
                    min,
                    max,
                    &mut cuboid_template_by_key,
                    &mut model_templates,
                    &mut model_quads,
                )?;
                visual.flags.remove(
                    BlockFlags::AIR
                        | BlockFlags::CUBE_GEOMETRY
                        | BlockFlags::OCCLUDES_FULL_FACE
                        | BlockFlags::LEAF_MODEL,
                );
                visual.faces = materials;
                visual.kind = VisualKind::Model;
                visual.model_template = template;
            }
        } else if is_slab(record) {
            let materials = BlockFace::ALL.map(|face| {
                descriptor_for(pack, record, face)
                    .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
            });
            if let [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ] = materials
                && let Some(half @ 0..=2) = record.model_state.get(ModelStateField::Half)
            {
                let faces = [west, east, down, up, north, south];
                let key = [west, east, down, up, north, south, half];
                let template = if let Some(&template) = slab_template_by_key.get(&key) {
                    template
                } else {
                    let template = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model quad",
                        }
                    })?;
                    model_templates.push(ModelTemplate {
                        quad_start,
                        quad_count: 6,
                        flags: 0,
                    });
                    model_quads.extend(slab_quads(faces, half));
                    slab_template_by_key.insert(key, template);
                    template
                };
                visual
                    .flags
                    .remove(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL);
                visual.flags.set(BlockFlags::OCCLUDES_FULL_FACE, half == 2);
                visual.faces = faces;
                visual.kind = VisualKind::Model;
                visual.model_template = template;
            }
        } else if is_stair(record) {
            let materials = BlockFace::ALL.map(|face| {
                descriptor_for(pack, record, face)
                    .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
            });
            if let [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ] = materials
                && let Some(orientation @ 0..=3) =
                    record.model_state.get(ModelStateField::Orientation)
                && let Some(upside @ 0..=1) = record.model_state.get(ModelStateField::Half)
            {
                let faces = [west, east, down, up, north, south];
                let rotation = (orientation + 2) & 3;
                let canonical_faces = canonical_stair_materials(faces, rotation);
                let key = [
                    canonical_faces[0],
                    canonical_faces[1],
                    canonical_faces[2],
                    canonical_faces[3],
                    canonical_faces[4],
                    canonical_faces[5],
                    upside,
                ];
                let base = if let Some(&base) = stair_template_by_key.get(&key) {
                    base
                } else {
                    let base = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    for shape in 0..5 {
                        let quads = stair_quads(canonical_faces, 2, upside != 0, shape);
                        let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                            AssetError::BlobSizeOverflow {
                                section: "model quad",
                            }
                        })?;
                        let quad_count = u32::try_from(quads.len()).map_err(|_| {
                            AssetError::BlobSizeOverflow {
                                section: "model quad count",
                            }
                        })?;
                        model_templates.push(ModelTemplate {
                            quad_start,
                            quad_count,
                            flags: MODEL_TEMPLATE_FLAG_STAIR,
                        });
                        model_quads.extend(quads);
                    }
                    stair_template_by_key.insert(key, base);
                    base
                };
                visual.flags.remove(
                    BlockFlags::AIR
                        | BlockFlags::CUBE_GEOMETRY
                        | BlockFlags::OCCLUDES_FULL_FACE
                        | BlockFlags::LEAF_MODEL,
                );
                visual.faces = faces;
                visual.kind = VisualKind::Model;
                visual.model_template = base;
                visual.variant = rotation | (upside << 2);
            }
        } else if is_kelp(record) {
            let descriptors = BlockFace::ALL.map(|face| descriptor_for(pack, record, face));
            let materials = descriptors.each_ref().map(|descriptor| {
                descriptor
                    .as_ref()
                    .and_then(|(descriptor, _)| material_by_descriptor.get(descriptor))
                    .copied()
            });
            if let [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ] = materials
            {
                let ordered = [north, south, up, down, east, west];
                let template = if let Some(&template) = kelp_template_by_material.get(&ordered) {
                    template
                } else {
                    let template = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model quad",
                        }
                    })?;
                    model_templates.push(ModelTemplate {
                        quad_start,
                        quad_count: 6,
                        flags: MODEL_TEMPLATE_FLAG_KELP,
                    });
                    model_quads.extend(kelp_quads(ordered));
                    kelp_template_by_material.insert(ordered, template);
                    template
                };
                visual.faces = [west, east, down, up, north, south];
                visual.kind = VisualKind::Model;
                visual.model_template = template;
            }
        } else if is_cross_visual(record) {
            let faces = if is_aquatic_cross(record) {
                aquatic_cross_faces(record)
            } else {
                Some([cross_texture_face(record); 2])
            };
            if let Some(faces) = faces
                && let Some((descriptor_a, _)) = descriptor_for(pack, record, faces[0])
                && let Some((descriptor_b, _)) = descriptor_for(pack, record, faces[1])
                && let Some(&material_a) = material_by_descriptor.get(&descriptor_a)
                && let Some(&material_b) = material_by_descriptor.get(&descriptor_b)
                && let Some(variant) = model_variant(pack, record, faces[0])
            {
                let materials = [material_a, material_b];
                let template = if let Some(&template) = template_by_material.get(&materials) {
                    template
                } else {
                    let template = u32::try_from(model_templates.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model template",
                        }
                    })?;
                    let quad_start = u32::try_from(model_quads.len()).map_err(|_| {
                        AssetError::BlobSizeOverflow {
                            section: "model quad",
                        }
                    })?;
                    model_templates.push(ModelTemplate {
                        quad_start,
                        quad_count: 2,
                        flags: 0,
                    });
                    model_quads.extend(crossed_quads(materials));
                    template_by_material.insert(materials, template);
                    template
                };
                visual.faces = [material_a; 6];
                visual.kind = VisualKind::Cross;
                visual.model_template = template;
                visual.variant = variant;
            }
        } else if record.flags.contains(BlockFlags::CUBE_GEOMETRY)
            && !record_has_deferred_material(pack, record)
        {
            let mut faces = [DIAGNOSTIC_MATERIAL; 6];
            let mut supported = true;
            for face in BlockFace::ALL {
                let Some((descriptor, _)) = descriptor_for(pack, record, face) else {
                    supported = false;
                    break;
                };
                let Some(&material) = material_by_descriptor.get(&descriptor) else {
                    supported = false;
                    break;
                };
                faces[face as usize] = material;
            }
            if supported {
                visual.faces = faces;
                visual.kind = VisualKind::Cube;
            }
        }
        visuals[record.sequential_id as usize] = visual;
        hashed.push((record.network_hash, record.sequential_id));
    }
    hashed.sort_unstable_by_key(|entry| entry.0);
    Ok((
        visuals.into_boxed_slice(),
        hashed.into_boxed_slice(),
        model_templates.into_boxed_slice(),
        model_quads.into_boxed_slice(),
    ))
}

/// Emits only the four horizontal attachment planes represented by Bedrock's
/// `vine_direction_bits`. The pinned Dragonfly codec defines bit order as
/// south, west, north, east; protocol 1001 carries no up/down attachment bit.
fn vine_quads(material: u32, connections: u32) -> Vec<ModelQuad> {
    debug_assert!(connections <= 15);
    const PLANES: [(u32, u32, [[i16; 3]; 4]); 4] = [
        (
            1,
            6,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (2, 3, [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]]),
        (4, 5, [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]]),
        (
            8,
            4,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    PLANES
        .into_iter()
        .filter(|(bit, _, _)| connections & bit != 0)
        .map(|(_, face, positions)| ModelQuad {
            positions,
            uvs: positions.map(|[x, y, z]| {
                let tangent = if matches!(face, 5 | 6) { x } else { z };
                [(tangent as u16) * 16, ((256 - y) as u16) * 16]
            }),
            material,
            // Vines remain visible from either side. Deliberately omit the
            // cull-face field: the support block is not a reason to drop the
            // attachment plane before alpha testing.
            flags: MODEL_QUAD_FLAG_TWO_SIDED | face,
        })
        .collect()
}

fn door_bounds(orientation: u32, open: u32, hinge: u32) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = 3 * 16;
    const HIGH: i16 = 256 - THICKNESS;
    // Dragonfly writes `Door.Facing.RotateRight()` into the Bedrock cardinal
    // state. Decode that stored orientation back to the logical closed facing
    // before applying model.Door's open/hinge rotations.
    const NORTH: u32 = 0;
    const SOUTH: u32 = 1;
    const WEST: u32 = 2;
    const EAST: u32 = 3;
    let facing = match orientation {
        0 => EAST,  // encoded south
        1 => SOUTH, // encoded west
        2 => WEST,  // encoded north
        3 => NORTH, // encoded east
        _ => unreachable!("door selectors are checked before geometry generation"),
    };
    let rotate_right = |facing| match facing {
        NORTH => EAST,
        EAST => SOUTH,
        SOUTH => WEST,
        WEST => NORTH,
        _ => unreachable!(),
    };
    let rotate_left = |facing| match facing {
        NORTH => WEST,
        WEST => SOUTH,
        SOUTH => EAST,
        EAST => NORTH,
        _ => unreachable!(),
    };
    let effective = match (open, hinge) {
        (0, 0 | 1) => facing,
        (1, 0) => rotate_right(facing),
        (1, 1) => rotate_left(facing),
        _ => unreachable!("door selectors are checked before geometry generation"),
    };
    match effective {
        NORTH => ([0, 0, HIGH], [256, 256, 256]),
        SOUTH => ([0, 0, 0], [256, 256, THICKNESS]),
        WEST => ([HIGH, 0, 0], [256, 256, 256]),
        EAST => ([0, 0, 0], [THICKNESS, 256, 256]),
        _ => unreachable!(),
    }
}

fn trapdoor_bounds(orientation: u32, open: u32, half: u32) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = 3 * 16;
    const HIGH: i16 = 256 - THICKNESS;
    match (open, orientation, half) {
        (0, _, 0) => ([0, 0, 0], [256, THICKNESS, 256]),
        (0, _, 1) => ([0, HIGH, 0], [256, 256, 256]),
        (1, 0, _) => ([0, 0, 0], [THICKNESS, 256, 256]),
        (1, 1, _) => ([HIGH, 0, 0], [256, 256, 256]),
        (1, 2, _) => ([0, 0, 0], [256, 256, THICKNESS]),
        (1, 3, _) => ([0, 0, HIGH], [256, 256, 256]),
        _ => unreachable!("trapdoor selectors are checked before geometry generation"),
    }
}

fn cuboid_quads(materials: [u32; 6], min: [i16; 3], max: [i16; 3]) -> [ModelQuad; 6] {
    debug_assert!(
        min.iter().zip(max).all(|(&min, max)| min < max),
        "cuboid bounds must have positive volume"
    );
    let [min_x, min_y, min_z] = min;
    let [max_x, max_y, max_z] = max;
    let make = |face: BlockFace, positions: [[i16; 3]; 4], face_id: u32| ModelQuad {
        uvs: positions.map(|[x, y, z]| match face {
            BlockFace::West | BlockFace::East => {
                [(z as u16) * 16, (4096 - i32::from(y) * 16) as u16]
            }
            BlockFace::North | BlockFace::South => {
                [(x as u16) * 16, (4096 - i32::from(y) * 16) as u16]
            }
            BlockFace::Down | BlockFace::Up => [(x as u16) * 16, (z as u16) * 16],
        }),
        positions,
        material: materials[face as usize],
        // Thin model cuboids deliberately never advertise a full-face cull
        // boundary. Their registry coverage remains conservative too.
        flags: face_id,
    };
    [
        make(
            BlockFace::West,
            [
                [min_x, min_y, min_z],
                [min_x, min_y, max_z],
                [min_x, max_y, max_z],
                [min_x, max_y, min_z],
            ],
            3,
        ),
        make(
            BlockFace::East,
            [
                [max_x, min_y, min_z],
                [max_x, max_y, min_z],
                [max_x, max_y, max_z],
                [max_x, min_y, max_z],
            ],
            4,
        ),
        make(
            BlockFace::Down,
            [
                [min_x, min_y, min_z],
                [max_x, min_y, min_z],
                [max_x, min_y, max_z],
                [min_x, min_y, max_z],
            ],
            1,
        ),
        make(
            BlockFace::Up,
            [
                [min_x, max_y, min_z],
                [min_x, max_y, max_z],
                [max_x, max_y, max_z],
                [max_x, max_y, min_z],
            ],
            2,
        ),
        make(
            BlockFace::North,
            [
                [min_x, min_y, min_z],
                [min_x, max_y, min_z],
                [max_x, max_y, min_z],
                [max_x, min_y, min_z],
            ],
            5,
        ),
        make(
            BlockFace::South,
            [
                [min_x, min_y, max_z],
                [max_x, min_y, max_z],
                [max_x, max_y, max_z],
                [min_x, max_y, max_z],
            ],
            6,
        ),
    ]
}

fn slab_quads(materials: [u32; 6], half: u32) -> [ModelQuad; 6] {
    let (min_y, max_y) = match half {
        0 => (0, 128),
        1 => (128, 256),
        2 => (0, 256),
        _ => unreachable!("slab half is checked before template generation"),
    };
    let min_v = (4096 - min_y * 16) as u16;
    let max_v = (4096 - max_y * 16) as u16;
    let vertical_standard = [[0, min_v], [4096, min_v], [4096, max_v], [0, max_v]];
    let vertical_transposed = [[0, min_v], [0, max_v], [4096, max_v], [4096, min_v]];
    let horizontal_standard = [[0, 0], [4096, 0], [4096, 4096], [0, 4096]];
    let horizontal_transposed = [[0, 0], [0, 4096], [4096, 4096], [4096, 0]];
    let flagged = |face: u32, boundary: bool| face | (u32::from(boundary) * (face << 4));
    [
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [0, min_y, 256],
                [0, max_y, 256],
                [0, max_y, 0],
            ],
            uvs: vertical_standard,
            material: materials[BlockFace::West as usize],
            flags: flagged(3, true),
        },
        ModelQuad {
            positions: [
                [256, min_y, 0],
                [256, max_y, 0],
                [256, max_y, 256],
                [256, min_y, 256],
            ],
            uvs: vertical_transposed,
            material: materials[BlockFace::East as usize],
            flags: flagged(4, true),
        },
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [256, min_y, 0],
                [256, min_y, 256],
                [0, min_y, 256],
            ],
            uvs: horizontal_standard,
            material: materials[BlockFace::Down as usize],
            flags: flagged(1, min_y == 0),
        },
        ModelQuad {
            positions: [
                [0, max_y, 0],
                [0, max_y, 256],
                [256, max_y, 256],
                [256, max_y, 0],
            ],
            uvs: horizontal_transposed,
            material: materials[BlockFace::Up as usize],
            flags: flagged(2, max_y == 256),
        },
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [0, max_y, 0],
                [256, max_y, 0],
                [256, min_y, 0],
            ],
            uvs: vertical_transposed,
            material: materials[BlockFace::North as usize],
            flags: flagged(5, true),
        },
        ModelQuad {
            positions: [
                [0, min_y, 256],
                [256, min_y, 256],
                [256, max_y, 256],
                [0, max_y, 256],
            ],
            uvs: vertical_standard,
            material: materials[BlockFace::South as usize],
            flags: flagged(6, true),
        },
    ]
}

fn stair_quads(
    materials: [u32; 6],
    orientation: u32,
    upside_down: bool,
    shape: u32,
) -> Vec<ModelQuad> {
    debug_assert!(orientation < 4 && shape < 5);
    let mut occupied = [false; 8];
    let base_y = usize::from(upside_down);
    let step_y = 1 - base_y;
    for x in 0..2 {
        for z in 0..2 {
            occupied[cell_index(x, base_y, z)] = true;
            let facing = toward(orientation, x, z);
            let right = toward((orientation + 1) & 3, x, z);
            let left = toward((orientation + 3) & 3, x, z);
            let opposite = toward((orientation + 2) & 3, x, z);
            let step = match shape {
                0 => facing,
                1 => facing || (opposite && right),
                2 => facing || (opposite && left),
                3 => facing && left,
                4 => facing && right,
                _ => false,
            };
            if step {
                occupied[cell_index(x, step_y, z)] = true;
            }
        }
    }
    let mut quads = Vec::with_capacity(32);
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                if !occupied[cell_index(x, y, z)] {
                    continue;
                }
                for face in BlockFace::ALL {
                    let neighbour = match face {
                        BlockFace::West => x.checked_sub(1).map(|nx| [nx, y, z]),
                        BlockFace::East => (x + 1 < 2).then_some([x + 1, y, z]),
                        BlockFace::Down => y.checked_sub(1).map(|ny| [x, ny, z]),
                        BlockFace::Up => (y + 1 < 2).then_some([x, y + 1, z]),
                        BlockFace::North => z.checked_sub(1).map(|nz| [x, y, nz]),
                        BlockFace::South => (z + 1 < 2).then_some([x, y, z + 1]),
                    };
                    if neighbour.is_none_or(|[nx, ny, nz]| !occupied[cell_index(nx, ny, nz)]) {
                        quads.push(stair_cell_quad(materials, face, x, y, z));
                    }
                }
            }
        }
    }
    debug_assert!(!quads.is_empty() && quads.len() <= 32);
    quads
}

const fn canonical_stair_materials(materials: [u32; 6], rotation: u32) -> [u32; 6] {
    let mut canonical = materials;
    match rotation {
        0 => {}
        1 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::North as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::East as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::West as usize];
        }
        2 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::East as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::West as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::North as usize];
        }
        3 => {
            canonical[BlockFace::West as usize] = materials[BlockFace::South as usize];
            canonical[BlockFace::East as usize] = materials[BlockFace::North as usize];
            canonical[BlockFace::North as usize] = materials[BlockFace::West as usize];
            canonical[BlockFace::South as usize] = materials[BlockFace::East as usize];
        }
        _ => {}
    }
    canonical
}

const fn cell_index(x: usize, y: usize, z: usize) -> usize {
    x | (y << 1) | (z << 2)
}

const fn toward(orientation: u32, x: usize, z: usize) -> bool {
    match orientation {
        0 => z == 1, // south
        1 => x == 0, // west
        2 => z == 0, // north
        3 => x == 1, // east
        _ => false,
    }
}

fn stair_cell_quad(
    materials: [u32; 6],
    face: BlockFace,
    x: usize,
    y: usize,
    z: usize,
) -> ModelQuad {
    let x0 = (x * 128) as i16;
    let x1 = x0 + 128;
    let y0 = (y * 128) as i16;
    let y1 = y0 + 128;
    let z0 = (z * 128) as i16;
    let z1 = z0 + 128;
    let (positions, face_id, boundary) = match face {
        BlockFace::West => (
            [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
            3,
            x == 0,
        ),
        BlockFace::East => (
            [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
            4,
            x == 1,
        ),
        BlockFace::Down => (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            1,
            y == 0,
        ),
        BlockFace::Up => (
            [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
            2,
            y == 1,
        ),
        BlockFace::North => (
            [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
            5,
            z == 0,
        ),
        BlockFace::South => (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            6,
            z == 1,
        ),
    };
    let uvs = positions.map(|[px, py, pz]| match face {
        BlockFace::West | BlockFace::East => [(pz as u16) * 16, (4096 - i32::from(py) * 16) as u16],
        BlockFace::North | BlockFace::South => {
            [(px as u16) * 16, (4096 - i32::from(py) * 16) as u16]
        }
        BlockFace::Down | BlockFace::Up => [(px as u16) * 16, (pz as u16) * 16],
    });
    ModelQuad {
        positions,
        uvs,
        material: materials[face as usize],
        flags: face_id | (u32::from(boundary) * (face_id << 4)),
    }
}

fn model_variant(pack: &PackSources, record: &RegistryRecord, face: BlockFace) -> Option<u32> {
    let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    pack.terrain
        .get_for_model_record(&key, record)
        .map(|(_, variant)| variant)
}

fn crossed_quads(materials: [u32; 2]) -> [ModelQuad; 2] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    [
        ModelQuad {
            positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
            uvs,
            material: materials[0],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
            uvs,
            material: materials[1],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}

#[derive(Clone, Copy)]
struct FlowerBedQuad {
    positions: [[i16; 3]; 4],
    uvs: [[u16; 2]; 4],
    stem: bool,
}

#[derive(Clone, Copy)]
struct FlowerBedPatch {
    quads: &'static [FlowerBedQuad],
}

const FLOWERBED_PATCH_1: [FlowerBedQuad; 7] = [
    flowerbed_quad(
        [[0, 48, 0], [128, 48, 0], [128, 48, 128], [0, 48, 128]],
        [[0, 0], [2048, 0], [2048, 2048], [0, 2048]],
        false,
    ),
    flowerbed_quad(
        [[77, 0, 19], [66, 0, 30], [66, 48, 30], [77, 48, 19]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[66, 0, 19], [77, 0, 30], [77, 48, 30], [66, 48, 19]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[29, 0, 81], [18, 0, 93], [18, 48, 93], [29, 48, 81]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[18, 0, 81], [29, 0, 93], [29, 48, 93], [18, 48, 81]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[109, 0, 98], [97, 0, 110], [97, 48, 110], [109, 48, 98]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[97, 0, 98], [109, 0, 110], [109, 48, 110], [97, 48, 98]],
        stem_uv(1024),
        true,
    ),
];

const FLOWERBED_PATCH_2: [FlowerBedQuad; 3] = [
    flowerbed_quad(
        [[0, 16, 128], [128, 16, 128], [128, 16, 256], [0, 16, 256]],
        [[0, 2048], [2048, 2048], [2048, 4096], [0, 4096]],
        false,
    ),
    flowerbed_quad(
        [[67, 0, 179], [78, 0, 190], [78, 16, 190], [67, 16, 179]],
        stem_uv(1536),
        true,
    ),
    flowerbed_quad(
        [[78, 0, 179], [67, 0, 190], [67, 16, 190], [78, 16, 179]],
        stem_uv(1536),
        true,
    ),
];

const FLOWERBED_PATCH_3: [FlowerBedQuad; 7] = [
    flowerbed_quad(
        [
            [128, 32, 128],
            [256, 32, 128],
            [256, 32, 256],
            [128, 32, 256],
        ],
        [[2048, 2048], [4096, 2048], [4096, 4096], [2048, 4096]],
        false,
    ),
    flowerbed_quad(
        [[186, 0, 218], [198, 0, 229], [198, 32, 229], [186, 32, 218]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[198, 0, 218], [186, 0, 229], [186, 32, 229], [198, 32, 218]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[238, 0, 162], [226, 0, 173], [226, 32, 173], [238, 32, 162]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[226, 0, 162], [238, 0, 173], [238, 32, 173], [226, 32, 162]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[157, 0, 146], [146, 0, 157], [146, 32, 157], [157, 32, 146]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[146, 0, 146], [157, 0, 157], [157, 32, 157], [146, 32, 146]],
        stem_uv(1280),
        true,
    ),
];

const FLOWERBED_PATCH_4: [FlowerBedQuad; 3] = [
    flowerbed_quad(
        [[128, 32, 0], [256, 32, 0], [256, 32, 128], [128, 32, 128]],
        [[2048, 0], [4096, 0], [4096, 2048], [2048, 2048]],
        false,
    ),
    flowerbed_quad(
        [[189, 0, 50], [177, 0, 62], [177, 32, 62], [189, 32, 50]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[177, 0, 50], [189, 0, 62], [189, 32, 62], [177, 32, 50]],
        stem_uv(1280),
        true,
    ),
];

const FLOWERBED_PATCHES: [FlowerBedPatch; 4] = [
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_1,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_2,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_3,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_4,
    },
];

const fn flowerbed_quad(positions: [[i16; 3]; 4], uvs: [[u16; 2]; 4], stem: bool) -> FlowerBedQuad {
    FlowerBedQuad {
        positions,
        uvs,
        stem,
    }
}

const fn stem_uv(min_v: u16) -> [[u16; 2]; 4] {
    [[0, 1792], [256, 1792], [256, min_v], [0, min_v]]
}

fn flowerbed_quads(
    materials: [u32; 2],
    growth: u32,
    orientation: u32,
) -> Result<Vec<ModelQuad>, AssetError> {
    let patch_count = usize::try_from(growth + 1).map_err(|_| AssetError::BlobSizeOverflow {
        section: "flowerbed patch count",
    })?;
    let patches =
        FLOWERBED_PATCHES
            .get(..patch_count)
            .ok_or_else(|| AssetError::InvalidCompiledAssets {
                detail: format!("flowerbed growth {growth} is not a normal state").into(),
            })?;
    if orientation > 3 {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed orientation {orientation} is not cardinal").into(),
        });
    }
    let quad_count = patches.iter().map(|patch| patch.quads.len()).sum();
    if quad_count > 32 {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed template has {quad_count} quads").into(),
        });
    }
    let mut quads = Vec::with_capacity(quad_count);
    for source in patches.iter().flat_map(|patch| patch.quads) {
        let mut positions = source.positions;
        for position in &mut positions {
            *position = rotate_flowerbed_position(*position, orientation)?;
            if position[1] >= 64 {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: "flowerbed template exceeded the near-ground bound".into(),
                });
            }
        }
        quads.push(ModelQuad {
            positions,
            uvs: source.uvs,
            material: materials[usize::from(source.stem)],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        });
    }
    Ok(quads)
}

fn rotate_flowerbed_position(
    [x, y, z]: [i16; 3],
    orientation: u32,
) -> Result<[i16; 3], AssetError> {
    if !(0..=256).contains(&x) || !(0..=256).contains(&z) {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed source position ({x}, {z}) is outside one block").into(),
        });
    }
    let complement = |value: i16| {
        i16::try_from(256_i32 - i32::from(value)).map_err(|_| AssetError::BlobSizeOverflow {
            section: "flowerbed rotated position",
        })
    };
    match orientation {
        0 => Ok([complement(x)?, y, complement(z)?]),
        1 => Ok([z, y, complement(x)?]),
        2 => Ok([x, y, z]),
        3 => Ok([complement(z)?, y, x]),
        _ => Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed orientation {orientation} is not cardinal").into(),
        }),
    }
}

fn kelp_quads(materials: [u32; 6]) -> [ModelQuad; 6] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    let diagonal_a = [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]];
    let diagonal_b = [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]];
    let reverse_a = [diagonal_a[1], diagonal_a[0], diagonal_a[3], diagonal_a[2]];
    let reverse_b = [diagonal_b[1], diagonal_b[0], diagonal_b[3], diagonal_b[2]];
    [
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[0],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[1],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_a,
            uvs,
            material: materials[2],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_b,
            uvs,
            material: materials[3],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[4],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[5],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use ::image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    use super::inspect_animation_inventory;
    use crate::TILE_SIZE;

    fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture");
    }

    fn write_png(path: impl AsRef<Path>, width: u32, height: u32, rgba8: &[u8]) {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(rgba8, width, height, ExtendedColorType::Rgba8)
            .expect("encode synthetic PNG");
        write(path, png);
    }

    #[test]
    fn animation_inventory_inspects_a_bounded_pack_without_installing_it() {
        let directory = tempfile::tempdir().expect("create inventory fixture");
        write(directory.path().join("blocks.json"), "{}");
        write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{
                "still":{"textures":"textures/blocks/still"},
                "animated":{"textures":"textures/blocks/animated"}
            }}"#,
        );
        write(
            directory.path().join("textures/flipbook_textures.json"),
            r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated"}]"#,
        );
        write_png(
            directory.path().join("textures/blocks/still.png"),
            TILE_SIZE,
            TILE_SIZE,
            &vec![7; (TILE_SIZE * TILE_SIZE * 4) as usize],
        );
        let mut strip = vec![0; (TILE_SIZE * TILE_SIZE * 2 * 4) as usize];
        for pixel in strip
            .chunks_exact_mut(4)
            .take((TILE_SIZE * TILE_SIZE) as usize)
        {
            pixel.copy_from_slice(&[10, 20, 30, 255]);
        }
        for pixel in strip
            .chunks_exact_mut(4)
            .skip((TILE_SIZE * TILE_SIZE) as usize)
        {
            pixel.copy_from_slice(&[40, 50, 60, 255]);
        }
        write_png(
            directory.path().join("textures/blocks/animated.png"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );

        let inventory = inspect_animation_inventory(directory.path(), 3, 2)
            .expect("inspect synthetic animation inventory");

        assert_eq!(inventory.static_sources, 1);
        assert_eq!(inventory.reachable_animations, 1);
        assert_eq!(inventory.physical_animation_frames, 2);
        assert_eq!(inventory.deduplicated_layers, 4);
        assert_eq!(inventory.page_layers.as_ref(), [3, 1]);
    }

    #[test]
    fn animation_inventory_counts_catalog_only_missing_static_aliases() {
        let directory = tempfile::tempdir().expect("create missing-static fixture");
        write(directory.path().join("blocks.json"), "{}");
        write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{
                "virtual":{"textures":"textures/blocks/not_a_physical_file"}
            }}"#,
        );
        write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        );

        let inventory = inspect_animation_inventory(directory.path(), 8, 2)
            .expect("catalog-only static aliases are measurable, not animation failures");

        assert_eq!(inventory.catalog_static_sources, 1);
        assert_eq!(inventory.static_sources, 0);
        assert_eq!(inventory.missing_static_sources, 1);
        assert_eq!(inventory.deduplicated_layers, 1, "diagnostic only");
    }

    #[test]
    fn animation_inventory_counts_non_tile_static_uv_sheets_without_paging_them() {
        let directory = tempfile::tempdir().expect("create non-tile fixture");
        write(directory.path().join("blocks.json"), "{}");
        write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{
                "model_uv":{"textures":"textures/blocks/model_uv"}
            }}"#,
        );
        write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        );
        write_png(
            directory.path().join("textures/blocks/model_uv.png"),
            24,
            12,
            &vec![255; 24 * 12 * 4],
        );

        let inventory = inspect_animation_inventory(directory.path(), 8, 2)
            .expect("non-tile model sheets remain outside texture pages");

        assert_eq!(inventory.catalog_static_sources, 1);
        assert_eq!(inventory.static_sources, 0);
        assert_eq!(inventory.missing_static_sources, 0);
        assert_eq!(inventory.non_tile_static_sources, 1);
        assert_eq!(inventory.deduplicated_layers, 1, "diagnostic only");
    }

    #[test]
    fn animation_inventory_rejects_a_missing_flipbook_strip() {
        let directory = tempfile::tempdir().expect("create missing-animation fixture");
        write(directory.path().join("blocks.json"), "{}");
        write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{
                "animated":{"textures":"textures/blocks/missing_strip"}
            }}"#,
        );
        write(
            directory.path().join("textures/flipbook_textures.json"),
            r#"[{
                "flipbook_texture":"textures/blocks/missing_strip",
                "atlas_tile":"animated"
            }]"#,
        );

        let error = inspect_animation_inventory(directory.path(), 8, 2)
            .expect_err("a missing physical animation strip must fail closed");
        assert!(matches!(
            error,
            crate::AssetError::MissingAnimationTexture { ref source_path }
                if source_path.as_ref() == "textures/blocks/missing_strip"
        ));
    }
}
