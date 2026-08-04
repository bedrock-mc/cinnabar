use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use assets::{
    Animation, AssetError, BiomeRegistryRecord, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, ContributorRole, DIAGNOSTIC_MATERIAL, LightProperties,
    MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV,
    MATERIAL_FLAG_WATER_TINT, MAX_MATERIALS, MAX_TEXTURE_LAYERS, MODEL_QUAD_FLAG_FACE_MASK,
    MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, MODEL_TEMPLATE_FLAG_FENCE_NETHER,
    MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_GATE_AXIS_X,
    MODEL_TEMPLATE_FLAG_GATE_AXIS_Z, MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE,
    MODEL_TEMPLATE_FLAG_STAIR, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, MODEL_TEMPLATE_FLAG_WALL,
    Material, ModelFamily, ModelQuad, ModelStateField, ModelTemplate, NO_ANIMATION, RegistryRecord,
    TextureArray, TexturePage, TextureRef, TextureSourceRoute, VisualKind, VisualSupport,
};

use crate::{
    AnimationInventory, BlockFace, PackSources, TextureKey,
    animation::{
        AnimationLimits, AnimationPlan, DecodedImage, compile_animation_plan,
        compile_animation_plan_selected,
    },
    compile_biome_assets,
    image::{decode_static_texture, decode_texture, normalize_texture_tile},
    pack::{read_pack, resolve_texture_key},
};

mod classification;
mod visuals;

use classification::{
    aquatic_cross_faces, canonical_state_u32, cross_texture_face, cutout_model_tint_flags,
    is_aquatic_cross, is_button, is_carpet, is_copper_grate, is_copper_grate_name, is_cross_visual,
    is_cutout_model_visual, is_door, is_fence, is_flowerbed, is_gate, is_kelp, is_liquid,
    is_model_visual, is_multiface, is_ordinary_stained_glass_name, is_pale_moss_carpet, is_pane,
    is_pressure_plate, is_sign, is_slab, is_stained_glass_cube, is_stair, is_supported_liquid,
    is_terrestrial_cross, is_trapdoor, is_vine, is_wall, leaf_tint_flags, liquid_material_flags,
    record_has_deferred_material, source_is_deferred,
};

use visuals::{
    bee_housing::{
        bee_housing_inventory_is_exact, bee_housing_material_descriptors, exact_bee_housing_state,
        is_bee_housing_name, is_bee_housing_record,
    },
    bookshelf::{
        chiseled_bookshelf_inventory_is_exact, chiseled_bookshelf_material_descriptors,
        chiseled_bookshelf_quads, exact_chiseled_bookshelf_state, is_chiseled_bookshelf_name,
        is_chiseled_bookshelf_record,
    },
    cactus::{
        cactus_inventory_is_exact, cactus_material_descriptors, is_cactus_name, is_cactus_record,
    },
    cake::{
        cake_inventory_is_exact, cake_material_descriptors, cake_source_alpha_is_exact,
        exact_cake_bite, is_cake_name, is_cake_record,
    },
    dispatcher::{CompileRuleResult, ExactAdmissions, compile_visuals},
    farmland::{
        exact_farmland_moisture, farmland_inventory_is_exact, farmland_material_descriptors,
        farmland_source_alpha_is_exact, is_farmland_name, is_farmland_record,
    },
    flowerbed::flowerbed_quads,
    geometry::cuboid_quads,
    mineral_cubes::{
        is_mineral_cube_name, is_mineral_cube_record, mineral_cube_inventory_is_exact,
        mineral_cube_material_descriptor, mineral_cube_sources_are_exact,
    },
    multiface::multiface_quads,
    resin_clump::{
        is_resin_clump, is_resin_clump_name, is_resin_clump_record, resin_clump_inventory_is_exact,
        resin_clump_material_descriptor,
    },
    selector_alias::{
        is_selector_alias_cube_name, is_selector_alias_cube_record,
        selector_alias_cube_inventory_is_exact, selector_alias_cube_material_descriptors,
    },
    vine::vine_quads,
};

const MAX_VISUALS: usize = 65_536;

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
pub fn compile_pack(
    root: &Path,
    records: &[RegistryRecord],
    light_properties: &[LightProperties],
) -> Result<CompiledAssets, AssetError> {
    compile_pack_inner(
        root,
        records,
        light_properties,
        CompiledBiomeAssets::diagnostic(),
    )
}

/// Compiles the complete v3 block and biome asset set.
pub fn compile_pack_with_biomes(
    root: &Path,
    behavior_pack: &Path,
    records: &[RegistryRecord],
    biome_registry: &[BiomeRegistryRecord],
    light_properties: &[LightProperties],
) -> Result<CompiledAssets, AssetError> {
    let biomes = compile_biome_assets(root, behavior_pack, biome_registry)?;
    compile_pack_inner(root, records, light_properties, biomes)
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

/// Compiles the immutable terrain-key to texture-reference map used to apply
/// server texture overrides to the existing runtime atlas.
pub fn compile_texture_catalog(
    root: &Path,
    source_manifest_sha256: [u8; 32],
) -> Result<Box<[u8]>, AssetError> {
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
            max_layers_per_page: MAX_TEXTURE_LAYERS as u32,
            max_pages: 2,
        },
    )?;
    let mut references_by_path = BTreeMap::<Box<str>, BTreeSet<TextureRef>>::new();
    for (path, reference) in plan.static_refs.iter().chain(plan.strip_first_refs.iter()) {
        references_by_path
            .entry(path.clone())
            .or_default()
            .insert(*reference);
    }
    for animation in &plan.animations {
        let start = animation.frame_start as usize;
        let end = start.checked_add(animation.frame_count as usize).ok_or(
            AssetError::BlobSizeOverflow {
                section: "texture catalog animation route",
            },
        )?;
        let frames = plan
            .frames
            .get(start..end)
            .ok_or(AssetError::InvalidCompiledAssets {
                detail: "texture catalog animation route exceeds the frame table".into(),
            })?;
        let references = references_by_path
            .entry(animation.source_path.clone())
            .or_default();
        references.extend(frames.iter().copied());
    }
    let mut routes = Vec::new();
    for (key, variant, path) in pack.terrain.source_routes() {
        let Some(references) = references_by_path.get(path) else {
            continue;
        };
        let animation_frames = pack
            .flipbooks
            .iter()
            .filter(|flipbook| {
                flipbook.atlas_tile.as_ref() == key
                    && flipbook.atlas_tile_variant == variant
                    && flipbook.texture_path.as_ref() == path
            })
            .filter(|flipbook| flipbook.atlas_index == 0)
            .find_map(|flipbook| {
                let animation = plan.animations.iter().find(|animation| {
                    animation.atlas_tile.as_ref() == key
                        && animation.atlas_tile_variant == variant
                        && animation.atlas_index == 0
                        && animation.source_path.as_ref() == path
                })?;
                let start = animation.frame_start as usize;
                let end = start.checked_add(animation.frame_count as usize)?;
                plan.frames
                    .get(start..end)
                    .map(|frames| frames.to_vec().into_boxed_slice())
            })
            .unwrap_or_default();
        routes.push(TextureSourceRoute::new(
            key,
            variant,
            path,
            references.iter().copied().collect::<Vec<_>>(),
        )
        .with_animation_frames(animation_frames));
    }
    routes.sort_by(|left, right| {
        (left.key(), left.variant(), left.path()).cmp(&(right.key(), right.variant(), right.path()))
    });
    assets::encode_texture_catalog(source_manifest_sha256, &routes).map_err(|error| {
        AssetError::InvalidCompiledAssets {
            detail: format!("encode texture catalog: {error}").into_boxed_str(),
        }
    })
}

fn compile_pack_inner(
    root: &Path,
    records: &[RegistryRecord],
    light_properties: &[LightProperties],
    biomes: CompiledBiomeAssets,
) -> Result<CompiledAssets, AssetError> {
    let pack = read_pack(root)?;
    validate_records(records)?;

    let admit_chiseled_bookshelves = chiseled_bookshelf_inventory_is_exact(records);
    let admit_mineral_cubes = mineral_cube_inventory_is_exact(records)
        && mineral_cube_sources_are_exact(root, &pack, records);
    let admit_resin_clumps = resin_clump_inventory_is_exact(records);
    let admit_selector_alias_cubes = selector_alias_cube_inventory_is_exact(records);
    let admit_cacti = cactus_inventory_is_exact(records);
    let admit_cakes = cake_inventory_is_exact(records) && cake_source_alpha_is_exact(root, &pack);
    let admit_farmland =
        farmland_inventory_is_exact(records) && farmland_source_alpha_is_exact(root, &pack);
    let admit_bee_housing = bee_housing_inventory_is_exact(records);

    let mut descriptor_keys = BTreeMap::<Descriptor, Box<str>>::new();
    let mut fallback_descriptors = BTreeSet::<Descriptor>::new();
    for record in records.iter().filter(|record| {
        if is_mineral_cube_name(&record.name) {
            return (admit_mineral_cubes && is_mineral_cube_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_selector_alias_cube_name(&record.name) {
            return (admit_selector_alias_cubes && is_selector_alias_cube_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_resin_clump_name(&record.name) {
            return (admit_resin_clumps && is_resin_clump_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_chiseled_bookshelf_name(&record.name) {
            return (admit_chiseled_bookshelves && is_chiseled_bookshelf_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_cactus_name(&record.name) {
            return (admit_cacti && is_cactus_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_cake_name(&record.name) {
            return (admit_cakes && is_cake_record(record)) || visuals::fallback::is_record(record);
        }
        if is_farmland_name(&record.name) {
            return (admit_farmland && is_farmland_record(record))
                || visuals::fallback::is_record(record);
        }
        if is_bee_housing_name(&record.name) {
            return (admit_bee_housing && is_bee_housing_record(record))
                || visuals::fallback::is_record(record);
        }
        (record.flags.contains(BlockFlags::CUBE_GEOMETRY)
            && !record_has_deferred_material(&pack, record))
            || is_model_visual(record)
            || is_liquid(record)
            || visuals::fallback::is_record(record)
    }) {
        if visuals::fallback::is_record(record) {
            for face in BlockFace::ALL {
                if let Some((descriptor, key)) = descriptor_for(&pack, record, face) {
                    fallback_descriptors.insert(descriptor.clone());
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
        if admit_mineral_cubes && is_mineral_cube_record(record) {
            if let Some((descriptor, key)) = mineral_cube_material_descriptor(&pack, record) {
                descriptor_keys.insert(descriptor, key);
            }
            continue;
        }
        if admit_selector_alias_cubes && is_selector_alias_cube_record(record) {
            if let Some(descriptors) = selector_alias_cube_material_descriptors(&pack, record) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
        if admit_resin_clumps && is_resin_clump_record(record) {
            if let Some((descriptor, key)) = resin_clump_material_descriptor(&pack) {
                descriptor_keys.insert(descriptor, key);
            }
            continue;
        }
        if admit_chiseled_bookshelves && is_chiseled_bookshelf_record(record) {
            if let Some(descriptors) = chiseled_bookshelf_material_descriptors(&pack) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
        if admit_cacti && is_cactus_record(record) {
            if let Some(descriptors) = cactus_material_descriptors(&pack) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
        if admit_cakes && is_cake_record(record) {
            if let Some(descriptors) = cake_material_descriptors(&pack) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
        if admit_farmland && is_farmland_record(record) {
            if let Some(descriptors) = farmland_material_descriptors(&pack) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
        if admit_bee_housing && is_bee_housing_record(record) {
            if let Some(descriptors) = bee_housing_material_descriptors(&pack) {
                for (descriptor, key) in descriptors {
                    descriptor_keys.insert(descriptor, key);
                }
            }
            continue;
        }
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
        if is_pale_moss_carpet(record)
            && let Some(descriptors) = pale_moss_carpet_side_material_descriptors(&pack)
        {
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

    let (animation_plan, alpha_paths) =
        compile_runtime_animation_plan(root, &pack, &descriptor_keys, &fallback_descriptors)?;
    let texture_pages = animation_plan_pages(&animation_plan)?;
    let (animations, animation_frames) = runtime_animation_tables(&animation_plan)?;
    let (materials, material_by_descriptor) =
        compile_materials(&descriptor_keys, &animation_plan, &alpha_paths)?;
    let vanilla_fallback_material =
        visuals::fallback::neutral_material(records, &pack, &material_by_descriptor)?;
    let (visuals, hashed, model_templates, model_quads) = compile_visuals(
        records,
        &pack,
        &material_by_descriptor,
        vanilla_fallback_material,
        ExactAdmissions {
            mineral_cubes: admit_mineral_cubes,
            chiseled_bookshelves: admit_chiseled_bookshelves,
            resin_clumps: admit_resin_clumps,
            selector_alias_cubes: admit_selector_alias_cubes,
            cacti: admit_cacti,
            cakes: admit_cakes,
            farmland: admit_farmland,
            bee_housing: admit_bee_housing,
        },
    )?;
    if light_properties.len() != visuals.len() {
        return Err(AssetError::InvalidCompiledAssets {
            detail: "light-property count does not match sequential visual span".into(),
        });
    }

    Ok(CompiledAssets {
        visuals,
        light_properties: light_properties.into(),
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
    let path = if is_model_visual(record) {
        pack.terrain.get_for_model_record(&key, record)?.0
    } else {
        pack.terrain.get_for_record(&key, record)?
    };
    if !is_model_visual(record)
        && !is_liquid(record)
        && !visuals::fallback::is_record(record)
        && source_is_deferred(pack, record, &key, path)
    {
        return None;
    }
    let mut flags = if rotate_uv {
        MATERIAL_FLAG_ROTATE_UV
    } else {
        0
    };
    if let Some(fallback_flags) = visuals::fallback::material_flags(record) {
        flags |= fallback_flags;
    } else if is_stained_glass_cube(record) {
        flags |= MATERIAL_FLAG_ALPHA_BLEND;
    } else if is_copper_grate(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_pane(record) {
        flags |= if record.name.contains("stained_glass_pane") {
            MATERIAL_FLAG_ALPHA_BLEND
        } else {
            MATERIAL_FLAG_ALPHA_CUTOUT
        };
    } else if is_fence(record) && record.name.contains("bamboo") {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_cutout_model_visual(record) {
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

fn pale_moss_carpet_side_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 2]> {
    let key: Box<str> = "pale_moss_carpet_side".into();
    let paths = pack.terrain.get_exact_pair(&key)?;
    Some(paths.map(|path| {
        (
            Descriptor {
                path: path.into(),
                texture_key: key.clone(),
                flags: MATERIAL_FLAG_ALPHA_CUTOUT,
            },
            key.clone(),
        )
    }))
}

fn compile_runtime_animation_plan(
    root: &Path,
    pack: &PackSources,
    descriptor_keys: &BTreeMap<Descriptor, Box<str>>,
    fallback_descriptors: &BTreeSet<Descriptor>,
) -> Result<(AnimationPlan, BTreeSet<Box<str>>), AssetError> {
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
    let mut alpha_paths = BTreeSet::new();
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
        let fallback_only = descriptor_keys
            .keys()
            .filter(|candidate| candidate.path == source_path)
            .all(|candidate| fallback_descriptors.contains(candidate));
        let path = static_texture_path(root, &source_path, key)?;
        let decoded = decode_texture(&path, key)?;
        if decoded.width != decoded.height {
            if fallback_only {
                continue;
            }
            return Err(AssetError::WrongTextureDimensions {
                key: key.clone(),
                path,
                width: decoded.width,
                height: decoded.height,
            });
        }
        let rgba8 = match normalize_texture_tile(decoded.rgba8, decoded.width, &source_path) {
            Ok(rgba8) => rgba8,
            Err(_) if fallback_only => continue,
            Err(error) => return Err(error),
        };
        let has_alpha = rgba8.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX);
        let supports_alpha = descriptor_keys
            .keys()
            .filter(|candidate| candidate.path == source_path)
            .any(descriptor_supports_alpha);
        if has_alpha && !supports_alpha {
            continue;
        }
        if has_alpha {
            alpha_paths.insert(source_path.clone());
        }
        decoded_images.push(DecodedImage {
            source_path,
            width: assets::TILE_SIZE,
            height: assets::TILE_SIZE,
            rgba8,
        });
    }
    for source_path in animation_paths {
        if !decoded_paths.insert(source_path.clone()) {
            continue;
        }
        let path = static_texture_path(root, &source_path, &source_path)?;
        let decoded = decode_texture(&path, &source_path)?;
        if decoded
            .rgba8
            .chunks_exact(4)
            .any(|pixel| pixel[3] != u8::MAX)
        {
            alpha_paths.insert(source_path.clone());
        }
        decoded_images.push(DecodedImage {
            source_path,
            width: decoded.width,
            height: decoded.height,
            rgba8: decoded.rgba8,
        });
    }
    let plan = compile_animation_plan_selected(
        pack,
        &decoded_images,
        AnimationLimits {
            max_layers_per_page: MAX_TEXTURE_LAYERS as u32,
            max_pages: 2,
        },
        Some(&selected_atlas_tiles),
    )?;
    Ok((plan, alpha_paths))
}

fn descriptor_supports_alpha(descriptor: &Descriptor) -> bool {
    // Bamboo gates and the beacon's shell/core are reviewed vanilla records
    // whose source alpha is consumed by their existing generated routes.
    descriptor.flags
        & (MATERIAL_FLAG_ALPHA_BLEND
            | MATERIAL_FLAG_ALPHA_CUTOUT
            | MATERIAL_FLAG_OVERLAY_MASK
            | MATERIAL_FLAG_LIQUID_DEPTH_WRITE)
        != 0
        || matches!(
            descriptor.texture_key.as_ref(),
            "bamboo_fence_gate" | "beacon_core" | "beacon_shell"
        )
}

fn animation_plan_pages(plan: &AnimationPlan) -> Result<Box<[TexturePage]>, AssetError> {
    let mut pages = Vec::new();
    for chunk in plan.layers.chunks(MAX_TEXTURE_LAYERS) {
        let mut mips = Vec::with_capacity(assets::MIP_COUNT as usize);
        for level in 0..assets::MIP_COUNT as usize {
            let size = assets::TILE_SIZE >> level;
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
            mips.push(assets::TextureMip {
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
    alpha_paths: &BTreeSet<Box<str>>,
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
        if alpha_paths.contains(&descriptor.path) && !descriptor_supports_alpha(descriptor) {
            continue;
        }
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

#[cfg(test)]
#[path = "compiler/tests.rs"]
mod tests;
