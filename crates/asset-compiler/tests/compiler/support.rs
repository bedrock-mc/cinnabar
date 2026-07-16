pub(super) use std::{
    collections::HashSet,
    fmt::Write as FmtWrite,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) use asset_compiler::{
    BlockFace, compile_pack as compile_pack_with_lights, read_pack, resolve_texture_key,
};
pub(super) use assets::{
    AssetError, BlockFlags, CollisionBox, CollisionConfidence, CollisionSeed, CompiledAssets,
    ContributorRole, DIAGNOSTIC_MATERIAL, LightProperties, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV,
    MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK, MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK,
    MAX_TEXTURE_LAYERS, MODEL_QUAD_FLAG_CULL_FACE_MASK, MODEL_QUAD_FLAG_FACE_MASK,
    MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR,
    MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, Material, ModelFamily, ModelQuad, ModelState,
    ModelStateField, NetworkIdMode, RegistryProvenance, RegistryRecord, RuntimeAssets, VisualKind,
    encode_blob, read_registry,
};
pub(super) use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
pub(super) use sha2::{Digest, Sha256};
pub(super) use tempfile::TempDir;

pub(super) const TILE_SIZE: u32 = 16;

pub(super) fn compile_pack(
    root: &Path,
    records: &[RegistryRecord],
) -> Result<CompiledAssets, AssetError> {
    let synthetic_lights = vec![
        LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap_or(0)
    ];
    compile_pack_with_lights(root, records, &synthetic_lights)
}

pub(super) const HUGE_MUSHROOM_NAMES: [&str; 3] = [
    "minecraft:brown_mushroom_block",
    "minecraft:mushroom_stem",
    "minecraft:red_mushroom_block",
];

pub(super) const ORDINARY_STAINED_GLASS_NAMES: [&str; 16] = [
    "minecraft:black_stained_glass",
    "minecraft:blue_stained_glass",
    "minecraft:brown_stained_glass",
    "minecraft:cyan_stained_glass",
    "minecraft:gray_stained_glass",
    "minecraft:green_stained_glass",
    "minecraft:light_blue_stained_glass",
    "minecraft:light_gray_stained_glass",
    "minecraft:lime_stained_glass",
    "minecraft:magenta_stained_glass",
    "minecraft:orange_stained_glass",
    "minecraft:pink_stained_glass",
    "minecraft:purple_stained_glass",
    "minecraft:red_stained_glass",
    "minecraft:white_stained_glass",
    "minecraft:yellow_stained_glass",
];

pub(super) const COPPER_GRATE_NAMES: [&str; 8] = [
    "minecraft:copper_grate",
    "minecraft:exposed_copper_grate",
    "minecraft:oxidized_copper_grate",
    "minecraft:waxed_copper_grate",
    "minecraft:waxed_exposed_copper_grate",
    "minecraft:waxed_oxidized_copper_grate",
    "minecraft:waxed_weathered_copper_grate",
    "minecraft:weathered_copper_grate",
];

pub(super) const COPPER_GRATE_ALIAS_PAIRS: [(&str, &str); 4] = [
    ("minecraft:copper_grate", "minecraft:waxed_copper_grate"),
    (
        "minecraft:exposed_copper_grate",
        "minecraft:waxed_exposed_copper_grate",
    ),
    (
        "minecraft:weathered_copper_grate",
        "minecraft:waxed_weathered_copper_grate",
    ),
    (
        "minecraft:oxidized_copper_grate",
        "minecraft:waxed_oxidized_copper_grate",
    ),
];

pub(super) fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture");
}

pub(super) fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    write_file(root.join("blocks.json"), blocks);
    write_file(root.join("textures/terrain_texture.json"), terrain);
    write_file(root.join("textures/flipbook_textures.json"), flipbooks);
}

pub(super) fn png_bytes(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(pixels.len(), (width * height) as usize);
    let rgba = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect::<Vec<_>>();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .expect("encode synthetic PNG");
    png
}

pub(super) fn tga_bytes(width: u16, height: u16, pixels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
    let mut tga = vec![0; 18];
    tga[2] = 2;
    tga[12..14].copy_from_slice(&width.to_le_bytes());
    tga[14..16].copy_from_slice(&height.to_le_bytes());
    tga[16] = 32;
    tga[17] = 0x28;
    for &[red, green, blue, alpha] in pixels {
        tga.extend_from_slice(&[blue, green, red, alpha]);
    }
    tga
}

pub(super) fn solid(width: u32, height: u32, color: [u8; 4]) -> Vec<[u8; 4]> {
    vec![color; (width * height) as usize]
}

pub(super) fn write_png(
    root: &Path,
    source_path: &str,
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
) {
    write_file(
        root.join(format!("{source_path}.png")),
        png_bytes(width, height, pixels),
    );
}

pub(super) fn write_tga(
    root: &Path,
    source_path: &str,
    width: u16,
    height: u16,
    pixels: &[[u8; 4]],
) {
    write_file(
        root.join(format!("{source_path}.tga")),
        tga_bytes(width, height, pixels),
    );
}

pub(super) fn record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    state: &str,
    flags: BlockFlags,
) -> RegistryRecord {
    let model_family = if flags.contains(BlockFlags::AIR) {
        ModelFamily::Air
    } else if flags.contains(BlockFlags::LEAF_MODEL) {
        ModelFamily::Leaves
    } else if flags.contains(BlockFlags::CUBE_GEOMETRY) {
        ModelFamily::Cube
    } else {
        ModelFamily::Unknown
    };
    RegistryRecord {
        sequential_id,
        network_hash,
        name: name.into(),
        canonical_state: state.into(),
        flags,
        model_family,
        contributor_role: if flags.contains(BlockFlags::AIR) {
            ContributorRole::Air
        } else {
            ContributorRole::Primary
        },
        model_state: ModelState::default(),
        face_coverage: if flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
            0x3f
        } else {
            0
        },
        collision_seed: CollisionSeed::default(),
        provenance: RegistryProvenance::DRAGONFLY,
    }
}

pub(super) fn model_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    state: &str,
    model_family: ModelFamily,
) -> RegistryRecord {
    let mut record = record(
        sequential_id,
        network_hash,
        name,
        state,
        BlockFlags::empty(),
    );
    record.model_family = model_family;
    record
}

pub(super) fn encoded_model_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    family: ModelFamily,
    fields: &[(ModelStateField, u32)],
) -> RegistryRecord {
    let state = b"{}";
    let mut mask = 0_u8;
    let mut values = [0_u32; 8];
    for &(field, value) in fields {
        let index = field as usize - 1;
        mask |= 1 << index;
        values[index] = value;
    }
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    for count in [1_u32, 1, 0, 0, 1, 1] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes.extend_from_slice(&sequential_id.to_le_bytes());
    bytes.extend_from_slice(&network_hash.to_le_bytes());
    bytes.push(0); // block flags
    bytes.push(family as u8);
    bytes.push(ContributorRole::Primary as u8);
    bytes.push(mask);
    bytes.push(0); // face coverage
    bytes.push(CollisionConfidence::None as u8);
    bytes.push(RegistryProvenance::DRAGONFLY.bits());
    bytes.push(0); // collision box count
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(state);
    read_registry(&bytes)
        .expect("decode synthetic model-state record")
        .into_vec()
        .pop()
        .expect("one synthetic record")
}

pub(super) const MODEL_FLAG_UPPER: u32 = 1 << 7;

pub(super) fn generated_family_records(name: &str, family: ModelFamily) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == name && record.model_family == family)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Flags).unwrap_or(0),
            record.model_state.get(ModelStateField::Half).unwrap_or(0),
            record.model_state.get(ModelStateField::Open).unwrap_or(0),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap_or(0),
            record.model_state.get(ModelStateField::Hinge).unwrap_or(0),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 15_000 + id as u32;
    }
    records
}

pub(super) const MODEL_FLAG_ATTACHED: u32 = 1 << 2;
pub(super) const MODEL_FLAG_HANGING: u32 = 1 << 3;

pub(super) fn material_for_face(
    compiled: &CompiledAssets,
    sequential_id: usize,
    face: BlockFace,
) -> Material {
    compiled.materials[compiled.visuals[sequential_id].faces[face as usize] as usize]
}

pub(super) fn leaf_material_fixture() -> (TempDir, PathBuf, Vec<RegistryRecord>) {
    let directory = tempfile::tempdir().expect("create leaf fixture");
    let resource_pack = directory.path().join("resource_pack");
    write_pack(
        &resource_pack,
        r#"{
            "stone": {"textures": "shared"},
            "cherry_leaves": {"textures": "shared"},
            "azalea_leaves": {"textures": "azalea"},
            "azalea_leaves_flowered": {"textures": "flowered"}
        }"#,
        r#"{"texture_data": {
            "shared": {"textures": "textures/blocks/a_shared"},
            "azalea": {"textures": "textures/blocks/b_azalea"},
            "flowered": {"textures": "textures/blocks/c_flowered"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("textures/blocks/a_shared", [220, 80, 90, 255]),
        ("textures/blocks/b_azalea", [40, 180, 80, 255]),
        ("textures/blocks/c_flowered", [220, 120, 180, 255]),
    ] {
        write_png(
            &resource_pack,
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    let leaf = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = vec![
        record(
            0,
            100,
            "minecraft:stone",
            "{}",
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        ),
        record(1, 101, "minecraft:cherry_leaves", "{}", leaf),
        record(2, 102, "minecraft:azalea_leaves", "{}", leaf),
        record(3, 103, "minecraft:azalea_leaves_flowered", "{}", leaf),
    ];
    (directory, resource_pack, records)
}

pub(super) fn biome_registry_bytes(id: u32, name: &str) -> Vec<u8> {
    let mut bytes = b"BIOREG01".to_vec();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(
        &u16::try_from(name.len())
            .expect("small fixture name")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

pub(super) fn write_biome_fixture(resource_pack: &Path) {
    write_file(
        resource_pack.join("biomes/plains.client_biome.json"),
        r#"{
            "format_version":"1.21.0",
            "minecraft:client_biome":{
                "description":{"identifier":"minecraft:plains"},
                "components":{}
            }
        }"#,
    );
    let behavior_pack = resource_pack
        .parent()
        .expect("resource pack has fixture parent")
        .join("behavior_pack");
    write_file(
        behavior_pack.join("biomes/plains.biome.json"),
        r#"{
            "format_version":"1.21.0",
            "minecraft:biome":{
                "description":{"identifier":"minecraft:plains"},
                "components":{"minecraft:climate":{"temperature":0.8,"downfall":0.4}}
            }
        }"#,
    );
    for name in [
        "grass",
        "foliage",
        "birch",
        "evergreen",
        "swamp_grass",
        "swamp_foliage",
        "mangrove_swamp_foliage",
        "dry_foliage",
    ] {
        write_png(
            resource_pack,
            &format!("textures/colormap/{name}"),
            256,
            256,
            &solid(256, 256, [80, 160, 40, 255]),
        );
    }
}

pub(super) fn registry_bytes(records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(record.flags.bits());
        bytes.push(record.model_family as u8);
        bytes.push(record.contributor_role as u8);
        bytes.push(record.model_state.mask());
        bytes.push(record.face_coverage);
        bytes.push(record.collision_seed.confidence as u8);
        bytes.push(record.provenance.bits());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).expect("small collision seed"));
        bytes.extend_from_slice(&record.collision_seed.shape_id.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(record.name.len())
                .expect("small fixture name")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(
            &u32::try_from(record.canonical_state.len())
                .expect("small fixture state")
                .to_le_bytes(),
        );
        for field in [
            assets::ModelStateField::Orientation,
            assets::ModelStateField::Half,
            assets::ModelStateField::Open,
            assets::ModelStateField::Hinge,
            assets::ModelStateField::Connections,
            assets::ModelStateField::Growth,
            assets::ModelStateField::LiquidDepth,
            assets::ModelStateField::Flags,
        ] {
            bytes.extend_from_slice(&record.model_state.get(field).unwrap_or(0).to_le_bytes());
        }
        for collision_box in &record.collision_seed.boxes {
            for coordinate in [
                collision_box.min_x,
                collision_box.min_y,
                collision_box.min_z,
                collision_box.max_x,
                collision_box.max_y,
                collision_box.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        bytes.extend_from_slice(record.name.as_bytes());
        bytes.extend_from_slice(record.canonical_state.as_bytes());
    }
    bytes
}

pub(super) fn light_registry_bytes(breg: &[u8], count: usize) -> Vec<u8> {
    let mut bytes = b"LREG1001".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    bytes.extend(std::iter::repeat_n(0xf0, count));
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

pub(super) fn shuffled_records(records: &[RegistryRecord], mut state: u64) -> Vec<RegistryRecord> {
    let mut shuffled = records.to_vec();
    for upper in (1..shuffled.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let bound = u64::try_from(upper + 1).expect("fixture bound fits u64");
        let index = usize::try_from(state % bound).expect("shuffle index fits usize");
        shuffled.swap(upper, index);
    }
    shuffled
}

pub(super) fn mip_layer(compiled: &CompiledAssets, mip_index: usize, layer: u32) -> &[u8] {
    let mip = &compiled.texture_pages[0].texture.mips[mip_index];
    let layer_bytes = usize::try_from(mip.size * mip.size * 4).expect("small mip");
    let start = usize::try_from(layer).expect("small layer") * layer_bytes;
    &mip.rgba8[start..start + layer_bytes]
}

pub(super) fn alpha_survivors(rgba: &[u8]) -> usize {
    assert_eq!(rgba.len() % 4, 0);
    rgba.chunks_exact(4).filter(|pixel| pixel[3] >= 128).count()
}

pub(super) fn scaled_survivors(raw_rgba: &[u8], scale: u32) -> usize {
    raw_rgba
        .chunks_exact(4)
        .filter(|pixel| {
            let alpha = ((u32::from(pixel[3]) * scale + 0x8000) >> 16).min(255) as u8;
            alpha >= 128
        })
        .count()
}

pub(super) fn reference_nearest_scale(raw_rgba: &[u8], target: usize) -> u32 {
    const SCALE_MAX: u32 = 16 << 16;
    const SURVIVOR_NUMERATOR: u32 = (128 << 16) - 0x8000;
    let mut candidates = vec![0];
    for alpha in raw_rgba.chunks_exact(4).map(|pixel| pixel[3]) {
        if alpha == 0 {
            continue;
        }
        let alpha = u32::from(alpha);
        let threshold = SURVIVOR_NUMERATOR.div_ceil(alpha);
        if threshold <= SCALE_MAX {
            candidates.push(threshold.saturating_sub(1));
            candidates.push(threshold);
        }
    }
    assert!(candidates.len() <= raw_rgba.len() / 2 + 1);
    candidates.sort_unstable();
    candidates.dedup();
    candidates
        .into_iter()
        .min_by_key(|&scale| (scaled_survivors(raw_rgba, scale).abs_diff(target), scale))
        .expect("scale zero is always present")
}

pub(super) fn reference_nearest_survivors(raw_rgba: &[u8], target: usize) -> usize {
    scaled_survivors(raw_rgba, reference_nearest_scale(raw_rgba, target))
}

pub(super) fn cutout_pattern(colour: [u8; 3], threshold: u32) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let alpha = if ((x * 17 + y * 29 + x * y * 7) & 255) < threshold {
                255
            } else {
                0
            };
            pixels.push([colour[0], colour[1], colour[2], alpha]);
        }
    }
    pixels
}

pub(super) fn aligned_half_pattern(colour: [u8; 3]) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for _y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            pixels.push([colour[0], colour[1], colour[2], u8::MAX * u8::from(x < 8)]);
        }
    }
    pixels
}

pub(super) fn reference_raw_mips(base: &[[u8; 4]], colour: [u8; 3]) -> Vec<Vec<u8>> {
    let mut size = TILE_SIZE;
    let mut current = base.to_vec();
    let mut mips = vec![current.iter().flatten().copied().collect::<Vec<_>>()];
    while size > 1 {
        let target_size = size / 2;
        let mut target = Vec::with_capacity((target_size * target_size) as usize);
        for y in 0..target_size {
            for x in 0..target_size {
                let mut alpha_sum = 0_u32;
                for offset_y in 0..2 {
                    for offset_x in 0..2 {
                        let source = ((y * 2 + offset_y) * size + x * 2 + offset_x) as usize;
                        alpha_sum += u32::from(current[source][3]);
                    }
                }
                let rgb = if alpha_sum == 0 { [0; 3] } else { colour };
                target.push([rgb[0], rgb[1], rgb[2], ((alpha_sum + 2) / 4) as u8]);
            }
        }
        mips.push(target.iter().flatten().copied().collect());
        current = target;
        size = target_size;
    }
    mips
}

pub(super) fn slab_geometry_digest(quads: &[ModelQuad]) -> String {
    let mut digest = Sha256::new();
    for quad in quads {
        for coordinate in quad.positions.iter().flatten() {
            digest.update(coordinate.to_le_bytes());
        }
        for coordinate in quad.uvs.iter().flatten() {
            digest.update(coordinate.to_le_bytes());
        }
        digest.update(quad.flags.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub(super) fn compiled_model_quads(
    compiled: &CompiledAssets,
    sequential_id: usize,
) -> &[ModelQuad] {
    let visual = compiled.visuals[sequential_id];
    assert_eq!(visual.kind, VisualKind::Model);
    let template = compiled.model_templates[visual.model_template as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

pub(super) fn compiled_compound_model_quads(
    compiled: &CompiledAssets,
    sequential_id: usize,
) -> Vec<ModelQuad> {
    let visual = compiled.visuals[sequential_id];
    assert_eq!(visual.kind, VisualKind::Model);
    let head_id = visual.model_template as usize;
    let head = compiled.model_templates[head_id];
    assert_eq!(
        head.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
        assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
    );
    [head, compiled.model_templates[head_id + 1]]
        .into_iter()
        .flat_map(|template| {
            compiled.model_quads
                [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
                .iter()
                .copied()
        })
        .collect()
}

pub(super) fn mip_pixel(
    compiled: &CompiledAssets,
    mip_index: usize,
    layer: u32,
    x: usize,
    y: usize,
) -> [u8; 4] {
    let mip = &compiled.texture_pages[0].texture.mips[mip_index];
    let size = mip.size as usize;
    let layer_bytes = size * size * 4;
    let offset = layer as usize * layer_bytes + (y * size + x) * 4;
    mip.rgba8[offset..offset + 4]
        .try_into()
        .expect("RGBA pixel")
}

pub(super) fn bee_housing_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:bee_nest" | "minecraft:beehive"
        )
    })
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    records
}

pub(super) fn model_bounds(quads: &[ModelQuad]) -> ([i16; 3], [i16; 3]) {
    let mut min = [i16::MAX; 3];
    let mut max = [i16::MIN; 3];
    for position in quads.iter().flat_map(|quad| quad.positions) {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}

pub(super) fn template_quads(compiled: &CompiledAssets, template: u32) -> &[ModelQuad] {
    let template = compiled.model_templates[template as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

pub(super) fn carpet_state_value<'a>(
    state: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
    expected_type: &str,
) -> &'a serde_json::Value {
    let value = state[name].as_object().expect("typed carpet selector");
    assert_eq!(value["type"], expected_type);
    &value["value"]
}

pub(super) fn generated_flowerbed_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    growth: u32,
    orientation: u32,
) -> RegistryRecord {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let mut record = records
        .into_iter()
        .find(|record| {
            record.name.as_ref() == name
                && record.model_state.get(ModelStateField::Growth) == Some(growth)
                && record.model_state.get(ModelStateField::Orientation) == Some(orientation)
        })
        .unwrap_or_else(|| panic!("missing {name} growth={growth} orientation={orientation}"));
    record.sequential_id = sequential_id;
    record.network_hash = network_hash;
    record
}
