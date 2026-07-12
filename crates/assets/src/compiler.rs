use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    Animation, AnimationInventory, AssetError, BiomeRegistryRecord, BlockFace, BlockFlags,
    CompiledBiomeAssets, ContributorRole, MODEL_QUAD_FLAG_TWO_SIDED, ModelFamily, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, PackSources, RegistryRecord, TextureKey,
    TexturePage, TextureRef, VisualKind,
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
pub const MATERIAL_FLAGS_MASK: u32 = MATERIAL_FLAG_UV_MASK
    | MATERIAL_FLAG_TINT_MASK
    | MATERIAL_FLAG_OVERLAY_MASK
    | MATERIAL_FLAG_ALPHA_BLEND
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_FOLIAGE_CLASS_MASK;

pub(crate) const fn material_flags_are_valid(flags: u32) -> bool {
    flags & !MATERIAL_FLAGS_MASK == 0
        && flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
            != MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT
        && (flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK == 0
            || flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT)
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
            || is_terrestrial_cross(record)
            || is_liquid(record)
    }) {
        let faces: &[BlockFace] = if is_terrestrial_cross(record) {
            &[cross_texture_face(record)]
        } else if is_liquid(record) {
            &[BlockFace::Up]
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
    let path = if is_terrestrial_cross(record) {
        pack.terrain.get_for_model_record(&key, record)?.0
    } else {
        pack.terrain.get_for_record(&key, record)?
    };
    if !is_terrestrial_cross(record)
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
    if is_terrestrial_cross(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT | cross_tint_flags(&record.name);
    } else if is_liquid(record) {
        flags |= liquid_material_flags(&record.name);
    } else if record.flags.contains(BlockFlags::LEAF_MODEL) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
        flags |= leaf_tint_flags(&record.name);
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

const fn is_terrestrial_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Cross | ModelFamily::Crop)
}

const fn is_liquid(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Liquid)
        && matches!(record.contributor_role, ContributorRole::LiquidAdditional)
}

const fn liquid_material_flags(name: &str) -> u32 {
    match name.as_bytes() {
        b"minecraft:water" | b"minecraft:flowing_water" => {
            MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
        }
        b"minecraft:lava" | b"minecraft:flowing_lava" => 0,
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

fn cross_tint_flags(name: &str) -> u32 {
    match name {
        "minecraft:short_grass"
        | "minecraft:tall_grass"
        | "minecraft:fern"
        | "minecraft:large_fern" => MATERIAL_FLAG_GRASS_TINT,
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
    let mut template_by_material = BTreeMap::<u32, u32>::new();

    for record in records {
        let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
        if is_liquid(record) {
            if let Some((descriptor, _)) = descriptor_for(pack, record, BlockFace::Up)
                && let Some(&material) = material_by_descriptor.get(&descriptor)
            {
                visual.flags.remove(
                    BlockFlags::AIR
                        | BlockFlags::CUBE_GEOMETRY
                        | BlockFlags::OCCLUDES_FULL_FACE
                        | BlockFlags::LEAF_MODEL,
                );
                visual.faces = [material; 6];
                visual.kind = VisualKind::Liquid;
            }
        } else if is_terrestrial_cross(record) {
            let face = cross_texture_face(record);
            if let Some((descriptor, _)) = descriptor_for(pack, record, face)
                && let Some(&material) = material_by_descriptor.get(&descriptor)
                && let Some(variant) = model_variant(pack, record, face)
            {
                let template = if let Some(&template) = template_by_material.get(&material) {
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
                    model_quads.extend(crossed_quads(material));
                    template_by_material.insert(material, template);
                    template
                };
                visual.faces = [material; 6];
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

fn model_variant(pack: &PackSources, record: &RegistryRecord, face: BlockFace) -> Option<u32> {
    let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    pack.terrain
        .get_for_model_record(&key, record)
        .map(|(_, variant)| variant)
}

fn crossed_quads(material: u32) -> [ModelQuad; 2] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    [
        ModelQuad {
            positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
            uvs,
            material,
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
            uvs,
            material,
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
