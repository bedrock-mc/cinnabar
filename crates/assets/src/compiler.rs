use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    AssetError, BiomeRegistryRecord, BlockFace, BlockFlags, CompiledBiomeAssets, PackSources,
    RegistryRecord, TextureKey, compile_biome_assets,
    image::{TextureArray, build_texture_array, decode_static_texture, diagnostic_pixels},
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
pub const MATERIAL_FLAG_ALPHA_CUTOUT: u32 = 1 << 8;
pub const MATERIAL_FLAG_FOLIAGE_CLASS_MASK: u32 = 0x0000_0600;
pub const MATERIAL_FLAG_BIRCH_FOLIAGE: u32 = 1 << 9;
pub const MATERIAL_FLAG_EVERGREEN_FOLIAGE: u32 = 1 << 10;
pub const MATERIAL_FLAG_DRY_FOLIAGE: u32 = MATERIAL_FLAG_FOLIAGE_CLASS_MASK;
pub const MATERIAL_FLAGS_MASK: u32 = MATERIAL_FLAG_UV_MASK
    | MATERIAL_FLAG_TINT_MASK
    | MATERIAL_FLAG_OVERLAY_MASK
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_FOLIAGE_CLASS_MASK;

pub(crate) const fn material_flags_are_valid(flags: u32) -> bool {
    flags & !MATERIAL_FLAGS_MASK == 0
        && (flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK == 0
            || flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT)
}

const MAX_VISUALS: usize = 65_536;

/// One immutable GPU material-table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Material {
    pub layer: u32,
    pub flags: u32,
}

const _: () = assert!(std::mem::size_of::<Material>() == 8);

/// Per-face material IDs and registry facts for one sequential block ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockVisual {
    pub faces: [u32; 6],
    pub flags: BlockFlags,
}

impl BlockVisual {
    fn diagnostic(flags: BlockFlags) -> Self {
        Self {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags,
        }
    }
}

/// Deterministic compiler output ready for checked blob serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledAssets {
    pub visuals: Box<[BlockVisual]>,
    pub hashed: Box<[(u32, u32)]>,
    pub materials: Box<[Material]>,
    pub textures: TextureArray,
    pub biomes: CompiledBiomeAssets,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Descriptor {
    path: Box<str>,
    flags: u32,
}

type CompiledLayers = (Vec<Box<[u8]>>, BTreeMap<Box<str>, u32>);
type CompiledMaterials = (Box<[Material]>, BTreeMap<Descriptor, u32>);
type CompiledVisuals = (Box<[BlockVisual]>, Box<[(u32, u32)]>);

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

fn compile_pack_inner(
    root: &Path,
    records: &[RegistryRecord],
    biomes: CompiledBiomeAssets,
) -> Result<CompiledAssets, AssetError> {
    let pack = read_pack(root)?;
    validate_records(records)?;

    let mut descriptor_keys = BTreeMap::<Descriptor, Box<str>>::new();
    for record in records.iter().filter(|record| {
        record.flags.contains(BlockFlags::CUBE_GEOMETRY)
            && !record_has_deferred_material(&pack, record)
    }) {
        for face in BlockFace::ALL {
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

    let (layers, layer_by_path) = compile_layers(root, &descriptor_keys)?;
    let cutout_layers = descriptor_keys
        .keys()
        .filter(|descriptor| descriptor.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0)
        .filter_map(|descriptor| layer_by_path.get(&descriptor.path).copied())
        .collect::<BTreeSet<_>>();
    let overlay_mask_layers = descriptor_keys
        .keys()
        .filter(|descriptor| descriptor.flags & MATERIAL_FLAG_OVERLAY_MASK != 0)
        .filter_map(|descriptor| layer_by_path.get(&descriptor.path).copied())
        .collect::<BTreeSet<_>>();
    let textures = build_texture_array(&layers, &cutout_layers, &overlay_mask_layers)?;
    let (materials, material_by_descriptor) = compile_materials(&descriptor_keys, &layer_by_path)?;
    let (visuals, hashed) = compile_visuals(records, &pack, &material_by_descriptor)?;

    Ok(CompiledAssets {
        visuals,
        hashed,
        materials,
        textures,
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
    let path = pack.terrain.get_for_record(&key, record)?;
    if source_is_deferred(pack, record, &key, path) {
        return None;
    }
    let mut flags = if rotate_uv {
        MATERIAL_FLAG_ROTATE_UV
    } else {
        0
    };
    if record.flags.contains(BlockFlags::LEAF_MODEL) {
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
            flags,
        },
        key,
    ))
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

fn source_is_deferred(pack: &PackSources, record: &RegistryRecord, key: &str, path: &str) -> bool {
    (record.name.as_ref() != "minecraft:grass_block" && pack.terrain.requires_tint(key))
        || pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key || flipbook.texture_path.as_ref() == path
        })
}

fn compile_layers(
    root: &Path,
    descriptor_keys: &BTreeMap<Descriptor, Box<str>>,
) -> Result<CompiledLayers, AssetError> {
    let cutout_paths = descriptor_keys
        .keys()
        .filter(|descriptor| descriptor.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0)
        .map(|descriptor| descriptor.path.clone())
        .collect::<BTreeSet<_>>();
    let overlay_mask_paths = descriptor_keys
        .keys()
        .filter(|descriptor| descriptor.flags & MATERIAL_FLAG_OVERLAY_MASK != 0)
        .map(|descriptor| descriptor.path.clone())
        .collect::<BTreeSet<_>>();
    let mut key_by_path = BTreeMap::<Box<str>, Box<str>>::new();
    for (descriptor, key) in descriptor_keys {
        key_by_path
            .entry(descriptor.path.clone())
            .and_modify(|current| {
                if key.as_ref() < current.as_ref() {
                    *current = key.clone();
                }
            })
            .or_insert_with(|| key.clone());
    }

    let mut layers = vec![diagnostic_pixels()];
    let mut layer_by_path = BTreeMap::new();
    let mut layers_by_digest = HashMap::<[u8; 32], Vec<u32>>::new();
    let diagnostic_digest: [u8; 32] = Sha256::digest(&layers[0]).into();
    layers_by_digest.insert(diagnostic_digest, vec![0]);

    for (path, key) in key_by_path {
        let source_path = static_texture_path(root, &path, &key)?;
        let pixels = decode_static_texture(&source_path, &key)?;
        if pixels.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX)
            && !cutout_paths.contains(&path)
            && !overlay_mask_paths.contains(&path)
        {
            continue;
        }
        let digest: [u8; 32] = Sha256::digest(&pixels).into();
        let existing = layers_by_digest.get(&digest).and_then(|candidates| {
            candidates
                .iter()
                .copied()
                .find(|&layer| layers[layer as usize].as_ref() == pixels.as_ref())
        });
        let layer = if let Some(layer) = existing {
            layer
        } else {
            if layers.len() >= MAX_TEXTURE_LAYERS {
                return Err(AssetError::TooManyTextureLayers {
                    count: layers.len() + 1,
                    max: MAX_TEXTURE_LAYERS,
                    key: Some(key),
                    path: Some(source_path),
                });
            }
            let layer = u32::try_from(layers.len()).map_err(|_| AssetError::BlobSizeOverflow {
                section: "texture layer",
            })?;
            layers.push(pixels);
            layers_by_digest.entry(digest).or_default().push(layer);
            layer
        };
        layer_by_path.insert(path, layer);
    }
    Ok((layers, layer_by_path))
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
    layer_by_path: &BTreeMap<Box<str>, u32>,
) -> Result<CompiledMaterials, AssetError> {
    let mut materials = vec![Material { layer: 0, flags: 0 }];
    let mut material_by_value = BTreeMap::<(u32, u32), u32>::new();
    material_by_value.insert((0, 0), DIAGNOSTIC_MATERIAL);
    let mut material_by_descriptor = BTreeMap::new();

    for descriptor in descriptor_keys.keys() {
        let Some(&layer) = layer_by_path.get(&descriptor.path) else {
            continue;
        };
        let value = (layer, descriptor.flags);
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
                layer,
                flags: descriptor.flags,
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
    let mut visuals = vec![BlockVisual::diagnostic(BlockFlags::empty()); visual_count];
    let mut hashed = Vec::with_capacity(records.len());

    for record in records {
        let mut visual = BlockVisual::diagnostic(record.flags);
        if record.flags.contains(BlockFlags::CUBE_GEOMETRY)
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
            }
        }
        visuals[record.sequential_id as usize] = visual;
        hashed.push((record.network_hash, record.sequential_id));
    }
    hashed.sort_unstable_by_key(|entry| entry.0);
    Ok((visuals.into_boxed_slice(), hashed.into_boxed_slice()))
}
