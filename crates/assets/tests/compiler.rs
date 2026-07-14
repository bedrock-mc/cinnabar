use std::{
    collections::HashSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assets::{
    AssetError, BlockFace, BlockFlags, CollisionBox, CollisionConfidence, CollisionSeed,
    CompiledAssets, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV,
    MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK, MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK,
    MAX_TEXTURE_LAYERS, MODEL_QUAD_FLAG_CULL_FACE_MASK, MODEL_QUAD_FLAG_FACE_MASK,
    MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, Material,
    ModelFamily, ModelQuad, ModelState, ModelStateField, NetworkIdMode, RegistryProvenance,
    RegistryRecord, RuntimeAssets, VisualKind, compile_pack, encode_blob, read_pack, read_registry,
    resolve_texture_key,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const TILE_SIZE: u32 = 16;

#[test]
fn flowerbed_generated_registry_has_exact_canonical_state_matrix() {
    let bytes = if let Ok(revision) = std::env::var("FLOWERBED_REGISTRY_GIT_REV") {
        let output = Command::new("git")
            .args([
                "show",
                &format!("{revision}:crates/assets/data/block-registry-v1001.bin"),
            ])
            .output()
            .expect("read requested registry revision");
        assert!(output.status.success(), "git show failed: {output:?}");
        output.stdout
    } else {
        include_bytes!("../data/block-registry-v1001.bin").to_vec()
    };
    let records = read_registry(&bytes).expect("decode committed generated registry");

    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 32, "{name} record count");

        let mut growths = [false; 8];
        let mut orientations = [false; 4];
        let mut selectors = HashSet::with_capacity(32);
        let mut canonical_states = HashSet::with_capacity(32);
        for record in selected {
            assert_eq!(record.model_family as u8, 31, "{name} raw family");
            assert_ne!(record.model_family, ModelFamily::Cross, "{name} is Cross");
            assert_ne!(
                record.model_family,
                ModelFamily::Unknown,
                "{name} is Unknown"
            );
            let growth = record
                .model_state
                .get(ModelStateField::Growth)
                .expect("flowerbed growth") as usize;
            let orientation = record
                .model_state
                .get(ModelStateField::Orientation)
                .expect("flowerbed orientation") as usize;
            assert!(
                growth < growths.len(),
                "{name} growth {growth} out of range"
            );
            assert!(
                orientation < orientations.len(),
                "{name} orientation {orientation} out of range"
            );
            growths[growth] = true;
            orientations[orientation] = true;
            assert!(
                selectors.insert((growth, orientation)),
                "{name} duplicate growth/orientation pair {growth}/{orientation}"
            );
            assert!(
                canonical_states.insert(record.canonical_state.as_ref()),
                "{name} duplicate canonical state"
            );
        }

        assert!(
            growths.into_iter().all(|present| present),
            "{name} growth coverage"
        );
        assert!(
            orientations.into_iter().all(|present| present),
            "{name} orientation coverage"
        );
        assert_eq!(selectors.len(), 32, "{name} selector uniqueness");
        assert_eq!(
            canonical_states.len(),
            32,
            "{name} canonical-state uniqueness"
        );
    }
}

#[test]
fn assetc_root_help_documents_all_compile_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .arg("--help")
        .output()
        .expect("run assetc help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    for required in [
        "compile",
        "animation-inventory",
        "--pack",
        "--registry",
        "--out",
    ] {
        assert!(help.contains(required), "help omitted {required}:\n{help}");
    }
}

#[test]
fn assetc_animation_inventory_records_the_full_deterministic_contract() {
    let directory = tempfile::tempdir().expect("create CLI inventory fixture");
    let pack = directory.path().join("resource pack");
    write_pack(
        &pack,
        "{}",
        r#"{"texture_data":{
            "still":{"textures":"textures/blocks/still"},
            "animated":{"textures":"textures/blocks/animated"}
        }}"#,
        r#"[{
            "flipbook_texture":"textures/blocks/animated",
            "atlas_tile":"animated",
            "ticks_per_frame":2,
            "blend_frames":true
        }]"#,
    );
    write_png(
        &pack,
        "textures/blocks/still",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [5, 10, 15, 255]),
    );
    let mut animation = solid(TILE_SIZE, TILE_SIZE, [20, 30, 40, 255]);
    animation.extend(solid(TILE_SIZE, TILE_SIZE, [50, 60, 70, 255]));
    write_png(
        &pack,
        "textures/blocks/animated",
        TILE_SIZE,
        TILE_SIZE * 2,
        &animation,
    );
    let manifest = directory.path().join("vanilla-source.json");
    let manifest_bytes = br#"{"schema":1,"commit":"synthetic"}
"#;
    write_file(&manifest, manifest_bytes);
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");

    for out in [&first, &second] {
        let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
            .args([
                "animation-inventory",
                "--pack",
                pack.to_str().expect("UTF-8 pack path"),
                "--source-manifest",
                manifest.to_str().expect("UTF-8 manifest path"),
                "--max-layers-per-page",
                "3",
                "--max-pages",
                "2",
                "--out",
                out.to_str().expect("UTF-8 output path"),
            ])
            .output()
            .expect("run animation inventory CLI");
        assert!(
            output.status.success(),
            "assetc failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first_bytes = fs::read(&first).expect("read first report");
    let second_bytes = fs::read(&second).expect("read second report");
    assert_eq!(
        first_bytes, second_bytes,
        "report bytes must be deterministic"
    );
    assert_eq!(first_bytes.last(), Some(&b'\n'));
    let report: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("parse inventory report");
    let manifest_sha = format!("{:x}", Sha256::digest(manifest_bytes));
    let canonical_pack = fs::canonicalize(&pack).expect("canonical pack path");

    assert_eq!(report["schema"], 1);
    assert_eq!(report["source_manifest_sha256"], manifest_sha);
    assert_eq!(
        report["canonical_pack_path"],
        canonical_pack.to_string_lossy().as_ref()
    );
    assert_eq!(report["limits"]["max_layers_per_page"], 3);
    assert_eq!(report["limits"]["max_pages"], 2);
    assert_eq!(report["inventory"]["static_sources"], 1);
    assert_eq!(report["inventory"]["reachable_animations"], 1);
    assert_eq!(report["inventory"]["physical_animation_frames"], 2);
    assert_eq!(report["inventory"]["deduplicated_layers"], 4);
    assert_eq!(
        report["inventory"]["page_layers"],
        serde_json::json!([3, 1])
    );
}

#[test]
fn compile_pack_installs_flipbook_pages_frames_and_material_animation() {
    let directory = tempfile::tempdir().expect("create runtime animation fixture");
    write_pack(
        directory.path(),
        r#"{"animated_block":{"textures":"animated"}}"#,
        r#"{"texture_data":{"animated":{"textures":"textures/blocks/animated"}}}"#,
        r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated","ticks_per_frame":3,"blend_frames":true}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [40, 50, 60, 255]));
    write_png(
        directory.path(),
        "textures/blocks/animated",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            1,
            "minecraft:animated_block",
            "minecraft:animated_block[]",
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        )],
    )
    .expect("compile flipbook into MCBEAS04 tables");

    assert_eq!(compiled.animations.len(), 1);
    assert_eq!(compiled.animations[0].frame_count, 2);
    assert_eq!(compiled.animations[0].ticks_per_frame, 3);
    assert_eq!(compiled.animation_frames.len(), 2);
    assert_ne!(compiled.animation_frames[0], compiled.animation_frames[1]);
    assert_eq!(compiled.materials.len(), 2);
    assert_eq!(compiled.materials[1].animation, 0);
    assert_eq!(compiled.materials[1].texture, compiled.animation_frames[0]);
    assert_eq!(compiled.texture_pages.len(), 1);
    let runtime = assets::RuntimeAssets::decode(&encode_blob(&compiled).unwrap())
        .expect("decode installed animation tables");
    assert_eq!(runtime.animations(), compiled.animations.as_ref());
    assert_eq!(
        runtime.animation_frames(),
        compiled.animation_frames.as_ref()
    );
}

#[test]
fn compile_pack_uses_compact_exact_flipbook_selector_and_preserves_metadata() {
    let directory = tempfile::tempdir().expect("create selector fixture");
    write_pack(
        directory.path(),
        r#"{"chosen_block":{"textures":"chosen"}}"#,
        r#"{"texture_data":{
            "unselected":{"textures":"textures/blocks/unselected"},
            "chosen":{"textures":"textures/blocks/chosen"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/unselected","atlas_tile":"unselected"},
            {"flipbook_texture":"textures/blocks/chosen","atlas_tile":"chosen","atlas_index":4,"atlas_tile_variant":5,"replicate":2},
            {"flipbook_texture":"textures/blocks/chosen","atlas_tile":"chosen","ticks_per_frame":7,"atlas_index":0,"atlas_tile_variant":0,"replicate":3}
        ]"#,
    );
    let mut chosen = solid(TILE_SIZE, TILE_SIZE, [1, 2, 3, 255]);
    chosen.extend(solid(TILE_SIZE, TILE_SIZE, [4, 5, 6, 255]));
    write_png(
        directory.path(),
        "textures/blocks/chosen",
        TILE_SIZE,
        TILE_SIZE * 2,
        &chosen,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            2,
            "minecraft:chosen_block",
            "minecraft:chosen_block[]",
            BlockFlags::CUBE_GEOMETRY,
        )],
    )
    .expect("compile selected flipbooks without loading unselected strip");

    assert_eq!(compiled.animations.len(), 2);
    assert_eq!(
        compiled.materials[1].animation, 1,
        "default selector is compact index one"
    );
    assert_eq!(compiled.animations[0].atlas_index, 4);
    assert_eq!(compiled.animations[0].atlas_tile_variant, 5);
    assert_eq!(compiled.animations[0].replicate, 2);
    assert_eq!(compiled.animations[1].atlas_index, 0);
    assert_eq!(compiled.animations[1].atlas_tile_variant, 0);
    assert_eq!(compiled.animations[1].replicate, 3);
    assert_eq!(compiled.animations[1].ticks_per_frame, 7);
}

#[test]
fn compile_pack_uses_first_strip_frame_for_non_flipbook_path_alias() {
    let directory = tempfile::tempdir().expect("create animated path alias fixture");
    write_pack(
        directory.path(),
        r#"{"flattened_prismarine":{"textures":"flattened_prismarine"}}"#,
        r#"{"texture_data":{
            "prismarine":{"textures":"textures/blocks/prismarine_rough"},
            "flattened_prismarine":{"textures":"textures/blocks/prismarine_rough"}
        }}"#,
        r#"[{"flipbook_texture":"textures/blocks/prismarine_rough","atlas_tile":"prismarine","ticks_per_frame":2}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [11, 22, 33, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [44, 55, 66, 255]));
    write_png(
        directory.path(),
        "textures/blocks/prismarine_rough",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[record(
            0,
            3,
            "minecraft:flattened_prismarine",
            "minecraft:flattened_prismarine[]",
            BlockFlags::CUBE_GEOMETRY,
        )],
    )
    .expect("compile alias without decoding the strip as a static 16x64 image");

    assert_eq!(compiled.animations.len(), 1, "strip remains compiled once");
    assert_eq!(compiled.materials.len(), 2);
    assert_eq!(compiled.materials[1].animation, assets::NO_ANIMATION);
    assert_eq!(compiled.materials[1].texture, compiled.animation_frames[0]);
    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material == 1)
    );
}

#[test]
fn compile_pack_keeps_static_and_animated_keys_distinct_on_one_strip_path() {
    let directory = tempfile::tempdir().expect("create shared strip-key fixture");
    write_pack(
        directory.path(),
        r#"{
            "static_block":{"textures":"a_static"},
            "animated_block":{"textures":"z_anim"}
        }"#,
        r#"{"texture_data":{
            "a_static":{"textures":"textures/blocks/shared_strip"},
            "z_anim":{"textures":"textures/blocks/shared_strip"}
        }}"#,
        r#"[{"flipbook_texture":"textures/blocks/shared_strip","atlas_tile":"z_anim"}]"#,
    );
    let mut strip = solid(TILE_SIZE, TILE_SIZE, [1, 10, 100, 255]);
    strip.extend(solid(TILE_SIZE, TILE_SIZE, [2, 20, 200, 255]));
    write_png(
        directory.path(),
        "textures/blocks/shared_strip",
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );
    let compiled = compile_pack(
        directory.path(),
        &[
            record(
                0,
                4,
                "minecraft:static_block",
                "{}",
                BlockFlags::CUBE_GEOMETRY,
            ),
            record(
                1,
                5,
                "minecraft:animated_block",
                "{}",
                BlockFlags::CUBE_GEOMETRY,
            ),
        ],
    )
    .expect("compile distinct atlas-key semantics on one source strip");

    let static_material = compiled.visuals[0].faces[0] as usize;
    let animated_material = compiled.visuals[1].faces[0] as usize;
    assert_ne!(static_material, animated_material);
    assert_eq!(
        compiled.materials[static_material].animation,
        assets::NO_ANIMATION
    );
    assert_eq!(compiled.materials[animated_material].animation, 0);
    assert_eq!(
        compiled.materials[static_material].texture, compiled.materials[animated_material].texture,
        "both begin on the same deduplicated physical frame"
    );
}

fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture");
}

fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    write_file(root.join("blocks.json"), blocks);
    write_file(root.join("textures/terrain_texture.json"), terrain);
    write_file(root.join("textures/flipbook_textures.json"), flipbooks);
}

fn png_bytes(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
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

fn tga_bytes(width: u16, height: u16, pixels: &[[u8; 4]]) -> Vec<u8> {
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

fn solid(width: u32, height: u32, color: [u8; 4]) -> Vec<[u8; 4]> {
    vec![color; (width * height) as usize]
}

fn write_png(root: &Path, source_path: &str, width: u32, height: u32, pixels: &[[u8; 4]]) {
    write_file(
        root.join(format!("{source_path}.png")),
        png_bytes(width, height, pixels),
    );
}

fn write_tga(root: &Path, source_path: &str, width: u16, height: u16, pixels: &[[u8; 4]]) {
    write_file(
        root.join(format!("{source_path}.tga")),
        tga_bytes(width, height, pixels),
    );
}

fn record(
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

fn model_record(
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

fn encoded_model_record(
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

const MODEL_FLAG_UPPER: u32 = 1 << 7;

fn generated_family_records(name: &str, family: ModelFamily) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
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

fn write_door_trapdoor_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "wooden_door":{"textures":{"down":"door_lower","side":"door_upper","up":"door_lower"}},
            "trapdoor":{"textures":"trapdoor"}
        }"#,
        r#"{"texture_data":{
            "door_lower":{"textures":"textures/blocks/door_lower"},
            "door_upper":{"textures":"textures/blocks/door_upper"},
            "trapdoor":{"textures":"textures/blocks/trapdoor"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("door_lower", [40, 80, 120, 0]),
        ("door_upper", [80, 120, 160, 127]),
        ("trapdoor", [120, 160, 200, 200]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn model_bounds(quads: &[ModelQuad]) -> ([i16; 3], [i16; 3]) {
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

fn pinned_collision_bounds(record: &RegistryRecord) -> ([i16; 3], [i16; 3]) {
    assert_eq!(
        record.collision_seed.confidence,
        CollisionConfidence::CollisionOnly,
        "{} {} collision authority",
        record.name,
        record.canonical_state
    );
    let [collision] = record.collision_seed.boxes.as_ref() else {
        panic!(
            "{} {} must have one pinned collision cuboid, got {:?}",
            record.name, record.canonical_state, record.collision_seed.boxes
        );
    };
    let convert = |value: i32| {
        let scaled = i64::from(value) * 256;
        let rounded = if scaled >= 0 {
            (scaled + 50_000_000) / 100_000_000
        } else {
            (scaled - 50_000_000) / 100_000_000
        };
        i16::try_from(rounded).expect("bounded collision coordinate")
    };
    (
        [
            convert(collision.min_x),
            convert(collision.min_y),
            convert(collision.min_z),
        ],
        [
            convert(collision.max_x),
            convert(collision.max_y),
            convert(collision.max_z),
        ],
    )
}

fn assert_bounds_within(
    rendered: ([i16; 3], [i16; 3]),
    collision: ([i16; 3], [i16; 3]),
    tolerance: i16,
) {
    for (rendered, collision) in rendered
        .0
        .into_iter()
        .chain(rendered.1)
        .zip(collision.0.into_iter().chain(collision.1))
    {
        assert!(
            (rendered - collision).abs() <= tolerance,
            "render/collision selector bounds differ: rendered={rendered} collision={collision} tolerance={tolerance}"
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DoorFacing {
    North,
    South,
    West,
    East,
}

impl DoorFacing {
    fn rotate_right(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    fn rotate_left(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
        }
    }
}

fn decoded_door_facing(encoded_orientation: u32) -> DoorFacing {
    let encoded = match encoded_orientation {
        0 => DoorFacing::South,
        1 => DoorFacing::West,
        2 => DoorFacing::North,
        3 => DoorFacing::East,
        _ => panic!("invalid encoded door orientation {encoded_orientation}"),
    };
    // Dragonfly encodes Door.Facing.RotateRight(), so recover the logical
    // closed facing by applying the inverse rotation.
    encoded.rotate_left()
}

fn expected_door_bounds(orientation: u32, open: u32, hinge: u32) -> ([i16; 3], [i16; 3]) {
    const T: i16 = 48;
    const H: i16 = 256 - T;
    let facing = decoded_door_facing(orientation);
    let effective = match (open, hinge) {
        (0, 0 | 1) => facing,
        (1, 0) => facing.rotate_right(),
        (1, 1) => facing.rotate_left(),
        _ => panic!("invalid door selector {orientation}/{open}/{hinge}"),
    };
    match effective {
        DoorFacing::North => ([0, 0, H], [256, 256, 256]),
        DoorFacing::South => ([0, 0, 0], [256, 256, T]),
        DoorFacing::West => ([H, 0, 0], [256, 256, 256]),
        DoorFacing::East => ([0, 0, 0], [T, 256, 256]),
    }
}

fn expected_trapdoor_bounds(orientation: u32, open: u32, half: u32) -> ([i16; 3], [i16; 3]) {
    const T: i16 = 48;
    const H: i16 = 256 - T;
    match (open, orientation, half) {
        (0, _, 0) => ([0, 0, 0], [256, T, 256]),
        (0, _, 1) => ([0, H, 0], [256, 256, 256]),
        (1, 0, _) => ([0, 0, 0], [T, 256, 256]),
        (1, 1, _) => ([H, 0, 0], [256, 256, 256]),
        (1, 2, _) => ([0, 0, 0], [256, 256, T]),
        (1, 3, _) => ([0, 0, H], [256, 256, 256]),
        _ => panic!("invalid trapdoor selector {orientation}/{open}/{half}"),
    }
}

fn assert_cutout_cuboid(compiled: &CompiledAssets, visual_id: usize) {
    let visual = compiled.visuals[visual_id];
    assert_eq!(visual.kind, VisualKind::Model);
    assert!(!visual.flags.intersects(
        BlockFlags::AIR
            | BlockFlags::CUBE_GEOMETRY
            | BlockFlags::OCCLUDES_FULL_FACE
            | BlockFlags::LEAF_MODEL
    ));
    let template = compiled.model_templates[visual.model_template as usize];
    assert_eq!(template.quad_count, 6);
    assert_eq!(template.flags, 0);
    let quads = compiled_model_quads(compiled, visual_id);
    for quad in quads {
        assert!(
            quad.positions
                .iter()
                .flatten()
                .all(|value| (0..=256).contains(value))
        );
        assert!(quad.uvs.iter().flatten().all(|value| *value <= 4096));
        assert!((1..=6).contains(&(quad.flags & MODEL_QUAD_FLAG_FACE_MASK)));
        assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
        assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        assert_ne!(quad.material, DIAGNOSTIC_MATERIAL);
        assert_eq!(
            compiled.materials[quad.material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
}

#[test]
fn compiler_routes_every_generated_door_and_trapdoor_selector_to_exact_cutout_cuboids() {
    let directory = tempfile::tempdir().expect("create door/trapdoor fixture");
    write_door_trapdoor_pack(directory.path());
    let doors = generated_family_records("minecraft:wooden_door", ModelFamily::Door);
    let trapdoors = generated_family_records("minecraft:trapdoor", ModelFamily::Trapdoor);
    assert_eq!(doors.len(), 32);
    assert_eq!(trapdoors.len(), 16);

    let compiled_doors = compile_pack(directory.path(), &doors).expect("compile all door states");
    let compiled_trapdoors =
        compile_pack(directory.path(), &trapdoors).expect("compile all trapdoor states");
    assert_eq!(
        compiled_doors.materials.len(),
        3,
        "diagnostic + lower + upper"
    );
    assert_eq!(
        compiled_trapdoors.materials.len(),
        2,
        "diagnostic + trapdoor"
    );
    assert_eq!(
        compiled_doors.model_templates.len(),
        8,
        "four spatial bounds times lower/upper materials"
    );
    assert_eq!(compiled_trapdoors.model_templates.len(), 6);
    let door_collision_bounds = doors
        .iter()
        .map(pinned_collision_bounds)
        .collect::<HashSet<_>>();
    let trapdoor_collision_bounds = trapdoors
        .iter()
        .map(pinned_collision_bounds)
        .collect::<HashSet<_>>();
    assert_eq!(
        door_collision_bounds,
        HashSet::from([([0, 0, 0], [47, 256, 256])]),
        "Prismarine exposes one uniform collision-only door seed for all typed states"
    );
    assert_eq!(
        trapdoor_collision_bounds,
        HashSet::from([
            ([0, 0, 0], [256, 47, 256]),
            ([0, 209, 0], [256, 256, 256]),
            ([0, 0, 209], [256, 256, 256]),
            ([0, 0, 0], [256, 256, 47]),
            ([209, 0, 0], [256, 256, 256]),
            ([0, 0, 0], [47, 256, 256]),
        ]),
        "trapdoor collision-only seeds must cover both halves and all four open boundaries"
    );
    // The pinned collision slabs are 0.1825 blocks thick (47/256 after
    // rounding) and contain no state transition. They therefore audit the
    // source limitation only; render geometry remains the exact typed 3/16
    // contract below and never reads CollisionSeed.

    for (id, record) in doors.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let hinge = record.model_state.get(ModelStateField::Hinge).unwrap();
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        assert!(orientation <= 3 && open <= 1 && hinge <= 1 && flags <= MODEL_FLAG_UPPER);
        assert_cutout_cuboid(&compiled_doors, id);
        let bounds = model_bounds(compiled_model_quads(&compiled_doors, id));
        assert_eq!(
            bounds,
            expected_door_bounds(orientation, open, hinge),
            "{}",
            record.canonical_state
        );
        let expected_material = usize::from(flags & MODEL_FLAG_UPPER != 0) + 1;
        assert!(
            compiled_model_quads(&compiled_doors, id)
                .iter()
                .all(|quad| quad.material as usize == expected_material)
        );
        if open == 0 {
            let peer = doors
                .iter()
                .position(|candidate| {
                    candidate.model_state.get(ModelStateField::Orientation) == Some(orientation)
                        && candidate.model_state.get(ModelStateField::Open) == Some(open)
                        && candidate.model_state.get(ModelStateField::Flags) == Some(flags)
                        && candidate.model_state.get(ModelStateField::Hinge) == Some(hinge ^ 1)
                })
                .unwrap();
            assert_eq!(
                compiled_doors.visuals[id].model_template,
                compiled_doors.visuals[peer].model_template,
                "closed door hinge must deduplicate"
            );
        }
    }

    for (id, record) in trapdoors.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let half = record.model_state.get(ModelStateField::Half).unwrap();
        assert!(orientation <= 3 && open <= 1 && half <= 1);
        assert_cutout_cuboid(&compiled_trapdoors, id);
        let bounds = model_bounds(compiled_model_quads(&compiled_trapdoors, id));
        assert_eq!(
            bounds,
            expected_trapdoor_bounds(orientation, open, half),
            "{}",
            record.canonical_state
        );
        assert_bounds_within(bounds, pinned_collision_bounds(record), 1);
        let peer = trapdoors
            .iter()
            .position(|candidate| {
                let same_open = candidate.model_state.get(ModelStateField::Open) == Some(open);
                if open == 0 {
                    same_open
                        && candidate.model_state.get(ModelStateField::Half) == Some(half)
                        && candidate.model_state.get(ModelStateField::Orientation)
                            == Some(orientation ^ 1)
                } else {
                    same_open
                        && candidate.model_state.get(ModelStateField::Orientation)
                            == Some(orientation)
                        && candidate.model_state.get(ModelStateField::Half) == Some(half ^ 1)
                }
            })
            .unwrap();
        assert_eq!(
            compiled_trapdoors.visuals[id].model_template,
            compiled_trapdoors.visuals[peer].model_template,
            "inactive trapdoor selector must deduplicate"
        );
    }
}

#[test]
fn compiler_door_and_trapdoor_selectors_fail_closed_when_required_fields_are_missing() {
    let directory = tempfile::tempdir().expect("create fail-closed fixture");
    write_door_trapdoor_pack(directory.path());
    let mut door = model_record(0, 16_000, "minecraft:wooden_door", "{}", ModelFamily::Door);
    let mut trapdoor = model_record(1, 16_001, "minecraft:trapdoor", "{}", ModelFamily::Trapdoor);
    door.collision_seed = CollisionSeed {
        shape_id: 99,
        confidence: CollisionConfidence::CollisionOnly,
        boxes: vec![CollisionBox {
            max_x: 100_000_000,
            max_y: 1,
            max_z: 100_000_000,
            ..CollisionBox::default()
        }]
        .into_boxed_slice(),
    };
    trapdoor.collision_seed = door.collision_seed.clone();
    let compiled = compile_pack(directory.path(), &[door, trapdoor]).expect("fail closed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_door_and_trapdoor_selectors_fail_closed_for_every_out_of_range_field() {
    let directory = tempfile::tempdir().expect("create invalid-selector fixture");
    write_door_trapdoor_pack(directory.path());
    let mut records = vec![
        encoded_model_record(
            0,
            16_100,
            "minecraft:wooden_door",
            ModelFamily::Door,
            &[
                (ModelStateField::Orientation, 0),
                (ModelStateField::Open, 0),
                (ModelStateField::Hinge, 0),
                (ModelStateField::Flags, 0),
            ],
        ),
        encoded_model_record(
            1,
            16_101,
            "minecraft:trapdoor",
            ModelFamily::Trapdoor,
            &[
                (ModelStateField::Orientation, 0),
                (ModelStateField::Open, 0),
                (ModelStateField::Half, 0),
            ],
        ),
    ];
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Hinge, 2),
        (ModelStateField::Flags, 1),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Hinge, 0),
            (ModelStateField::Flags, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            16_100 + id,
            "minecraft:wooden_door",
            ModelFamily::Door,
            &fields,
        ));
    }
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Half, 2),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Half, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            16_100 + id,
            "minecraft:trapdoor",
            ModelFamily::Trapdoor,
            &fields,
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile bounded selectors");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Model);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Model);
    assert!(
        compiled.visuals[2..]
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
}

#[test]
fn compiler_selects_the_exact_legacy_door_terrain_variant_for_each_material_family() {
    let directory = tempfile::tempdir().expect("create legacy door-array fixture");
    let names = [
        "wooden_door",
        "spruce_door",
        "birch_door",
        "jungle_door",
        "acacia_door",
        "dark_oak_door",
        "iron_door",
    ];
    let blocks = serde_json::Value::Object(
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    serde_json::json!({"textures":{"down":"door_lower","side":"door_upper","up":"door_lower"}}),
                )
            })
            .collect(),
    );
    let lower_paths = (0..7)
        .map(|index| format!("textures/blocks/lower_{index}"))
        .collect::<Vec<_>>();
    let upper_paths = (0..7)
        .map(|index| format!("textures/blocks/upper_{index}"))
        .collect::<Vec<_>>();
    let terrain = serde_json::json!({"texture_data":{
        "door_lower":{"textures":lower_paths},
        "door_upper":{"textures":upper_paths}
    }});
    write_pack(
        directory.path(),
        &serde_json::to_string(&blocks).unwrap(),
        &serde_json::to_string(&terrain).unwrap(),
        "[]",
    );
    for index in 0..7_u8 {
        for half in ["lower", "upper"] {
            write_png(
                directory.path(),
                &format!("textures/blocks/{half}_{index}"),
                TILE_SIZE,
                TILE_SIZE,
                &solid(TILE_SIZE, TILE_SIZE, [index * 31 + 1, 2, 3, 127]),
            );
        }
    }
    let all = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed registry");
    let mut records = names
        .iter()
        .enumerate()
        .map(|(id, name)| {
            let mut record = all
                .iter()
                .find(|record| {
                    record.name.as_ref() == format!("minecraft:{name}")
                        && record.model_state.get(ModelStateField::Flags) == Some(0)
                        && record.model_state.get(ModelStateField::Open) == Some(0)
                        && record.model_state.get(ModelStateField::Hinge) == Some(0)
                        && record.model_state.get(ModelStateField::Orientation) == Some(0)
                })
                .unwrap_or_else(|| panic!("missing lower {name}"))
                .clone();
            record.sequential_id = id as u32;
            record.network_hash = 17_000 + id as u32;
            record
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile legacy door variants");
    for (id, name) in names.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{name}");
        let material = compiled.materials[compiled_model_quads(&compiled, id)[0].material as usize];
        assert_eq!(
            &mip_layer(&compiled, 0, material.texture.layer())[0..4],
            &[id as u8 * 31 + 1, 2, 3, 127],
            "{name} selected the wrong legacy terrain-array entry"
        );
    }
    let baseline = encode_blob(&compiled).expect("encode legacy door variants");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed door records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_door_and_trapdoor_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| {
            matches!(
                record.model_family,
                ModelFamily::Door | ModelFamily::Trapdoor
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Door)
            .count(),
        672
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Trapdoor)
            .count(),
        336
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 60_000 + id as u32;
    }

    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned doors");
    for (id, record) in records.iter().enumerate() {
        assert_cutout_cuboid(&compiled, id);
        assert_eq!(record.face_coverage, 0, "{}", record.name);
    }
    for modern in [
        "minecraft:bamboo_door",
        "minecraft:cherry_door",
        "minecraft:mangrove_door",
        "minecraft:pale_oak_door",
        "minecraft:crimson_door",
        "minecraft:warped_door",
        "minecraft:copper_door",
        "minecraft:exposed_copper_door",
        "minecraft:weathered_copper_door",
        "minecraft:oxidized_copper_door",
        "minecraft:waxed_copper_door",
        "minecraft:waxed_exposed_copper_door",
        "minecraft:waxed_weathered_copper_door",
        "minecraft:waxed_oxidized_copper_door",
    ] {
        let matching = records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.name.as_ref() == modern)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 32, "{modern} state count");
        assert!(matching.into_iter().all(|(id, _)| {
            compiled.visuals[id].kind == VisualKind::Model
                && compiled_model_quads(&compiled, id).iter().all(|quad| {
                    compiled.materials[quad.material as usize].flags == MATERIAL_FLAG_ALPHA_CUTOUT
                })
        }));
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive doors");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed doors");
    assert_eq!(
        encode_blob(&reversed).expect("encode reversed exhaustive doors"),
        baseline,
        "door/trapdoor compiler output depends on registry order"
    );
}

fn generated_wall_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.name.as_ref() == name && record.model_family == ModelFamily::Wall)
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("wall connections")
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 70_000 + id as u32;
    }
    records
}

fn write_wall_pack(root: &Path) {
    write_pack(
        root,
        r#"{"cobblestone_wall":{"textures":{
            "west":"wall_west","east":"wall_east","down":"wall_down",
            "up":"wall_up","north":"wall_north","south":"wall_south"
        }}}"#,
        r#"{"texture_data":{
            "wall_west":{"textures":"textures/blocks/wall_west"},
            "wall_east":{"textures":"textures/blocks/wall_east"},
            "wall_down":{"textures":"textures/blocks/wall_down"},
            "wall_up":{"textures":"textures/blocks/wall_up"},
            "wall_north":{"textures":"textures/blocks/wall_north"},
            "wall_south":{"textures":"textures/blocks/wall_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/wall_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 31 + 1, 50, 90, 255]),
        );
    }
}

fn expected_wall_boxes(connections: u32) -> Vec<([i16; 3], [i16; 3])> {
    assert_eq!(connections & !0x1ff, 0);
    let north = connections & 3;
    let east = (connections >> 2) & 3;
    let south = (connections >> 4) & 3;
    let west = (connections >> 6) & 3;
    let post = (connections >> 8) & 1;
    assert!(
        [north, east, south, west]
            .into_iter()
            .all(|connection| connection <= 2)
    );
    let height = |connection| match connection {
        1 => 224,
        2 => 256,
        _ => unreachable!(),
    };
    // The local vanilla template_wall_{post,side,side_tall}.json files are the
    // visible render oracle. Dragonfly collision BBoxes are intentionally not
    // used for these extents.
    let mut boxes = Vec::with_capacity(5);
    if post != 0 {
        boxes.push(([64, 0, 64], [192, 256, 192]));
    }
    if north != 0 {
        boxes.push(([80, 0, 0], [176, height(north), 128]));
    }
    if east != 0 {
        boxes.push(([128, 0, 80], [256, height(east), 176]));
    }
    if south != 0 {
        boxes.push(([80, 0, 128], [176, height(south), 256]));
    }
    if west != 0 {
        boxes.push(([0, 0, 80], [128, height(west), 176]));
    }
    boxes
}

#[test]
fn compiler_routes_all_generated_wall_connections_to_exact_compact_cuboids() {
    let directory = tempfile::tempdir().expect("create wall fixture");
    write_wall_pack(directory.path());
    let records = generated_wall_records("minecraft:cobblestone_wall");
    assert_eq!(records.len(), 162, "3^4 connection heights times post bit");
    let selectors = records
        .iter()
        .map(|record| {
            record
                .model_state
                .get(ModelStateField::Connections)
                .expect("typed wall connections")
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors.len(), 162);

    let compiled = compile_pack(directory.path(), &records).expect("compile all wall states");
    assert_eq!(compiled.materials.len(), 7, "diagnostic plus six faces");
    assert_eq!(compiled.model_templates.len(), 162);
    for (id, record) in records.iter().enumerate() {
        let connections = record
            .model_state
            .get(ModelStateField::Connections)
            .unwrap();
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let template = compiled.model_templates[visual.model_template as usize];
        let expected = expected_wall_boxes(connections);
        assert_eq!(template.quad_count as usize, expected.len() * 6);
        assert!(template.quad_count <= 30);
        let quads = compiled_model_quads(&compiled, id);
        for (cuboid, bounds) in quads.chunks_exact(6).zip(expected) {
            assert_eq!(model_bounds(cuboid), bounds, "{}", record.canonical_state);
            for (face, quad) in cuboid.iter().enumerate() {
                assert_eq!(quad.material, visual.faces[face]);
                assert_eq!(quad.flags, [3, 4, 1, 2, 5, 6][face]);
                assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
                assert!(quad.uvs.iter().flatten().all(|value| *value <= 4096));
            }
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive walls");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "wall compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed wall render geometry"
    );
}

#[test]
fn compiler_wall_connections_fail_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid wall fixture");
    write_wall_pack(directory.path());
    let mut records = vec![model_record(
        0,
        71_000,
        "minecraft:cobblestone_wall",
        "{}",
        ModelFamily::Wall,
    )];
    for connections in [3, 3 << 2, 3 << 4, 3 << 6, 1 << 9] {
        let id = records.len() as u32;
        records.push(encoded_model_record(
            id,
            71_000 + id,
            "minecraft:cobblestone_wall",
            ModelFamily::Wall,
            &[(ModelStateField::Connections, connections)],
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid wall states");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

fn write_connected_model_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "glass_pane":{"textures":{
                "west":"pane_body","east":"pane_edge","down":"pane_edge",
                "up":"pane_edge","north":"pane_body","south":"pane_body"
            }},
            "oak_fence":{"textures":"oak_fence"},
            "nether_brick_fence":{"textures":"nether_fence"}
        }"#,
        r#"{"texture_data":{
            "pane_body":{"textures":"textures/blocks/pane_body"},
            "pane_edge":{"textures":"textures/blocks/pane_edge"},
            "oak_fence":{"textures":"textures/blocks/oak_fence"},
            "nether_fence":{"textures":"textures/blocks/nether_fence"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("pane_body", [30, 60, 90, 0]),
        ("pane_edge", [60, 90, 120, 255]),
        ("oak_fence", [100, 70, 30, 255]),
        ("nether_fence", [70, 10, 20, 255]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn template_quads(compiled: &CompiledAssets, template: u32) -> &[ModelQuad] {
    let template = compiled.model_templates[template as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

#[test]
fn compiler_emits_all_sixteen_exact_pane_connection_templates() {
    let directory = tempfile::tempdir().expect("create pane fixture");
    write_connected_model_pack(directory.path());
    let record = model_record(0, 72_000, "minecraft:glass_pane", "{}", ModelFamily::Pane);
    let compiled = compile_pack(directory.path(), &[record]).expect("compile pane");
    let visual = compiled.visuals[0];
    assert_eq!(visual.kind, VisualKind::Model);
    assert!(!visual.flags.intersects(
        BlockFlags::AIR
            | BlockFlags::CUBE_GEOMETRY
            | BlockFlags::OCCLUDES_FULL_FACE
            | BlockFlags::LEAF_MODEL
    ));
    let base = visual.model_template;
    assert_eq!(compiled.model_templates.len(), 16);
    for mask in 0_u32..16 {
        let template = compiled.model_templates[(base + mask) as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_PANE, "mask={mask:#06b}");
        assert_eq!(
            template.quad_count,
            6 + mask.count_ones() * 4,
            "post and arms omit both faces at every internal join, mask={mask:#06b}"
        );
        assert!(template.quad_count <= 26);
        let quads = template_quads(&compiled, base + mask);
        if mask == 0 {
            assert_eq!(model_bounds(quads), ([112, 0, 112], [144, 256, 144]));
        }
        for (bit, axis, coordinate) in [(1, 2, 112), (2, 0, 144), (4, 2, 144), (8, 0, 112)] {
            if mask & bit != 0 {
                let span_axis = if axis == 0 { 2 } else { 0 };
                assert!(
                    quads.iter().all(|quad| {
                        !quad.positions.iter().all(|position| {
                            position[axis] == coordinate
                                && (112..=144).contains(&position[span_axis])
                        })
                    }),
                    "internal pane join remains for mask={mask:#06b} bit={bit:#06b}"
                );
            }
        }
        assert!(
            quads
                .iter()
                .all(|quad| quad.uvs.iter().flatten().all(|uv| *uv <= 4096))
        );
    }
    assert!(compiled.materials.iter().skip(1).all(|material| {
        material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
            && material.flags & MATERIAL_FLAG_ALPHA_BLEND == 0
    }));
}

#[test]
fn compiler_emits_bounded_seventeen_template_fence_groups_by_connection_class() {
    let directory = tempfile::tempdir().expect("create fence fixture");
    write_connected_model_pack(directory.path());
    let records = [
        model_record(0, 73_000, "minecraft:oak_fence", "{}", ModelFamily::Fence),
        model_record(
            1,
            73_001,
            "minecraft:nether_brick_fence",
            "{}",
            ModelFamily::Fence,
        ),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile fences");
    assert_eq!(compiled.model_templates.len(), 34);
    for (id, expected_flag) in [
        (0, MODEL_TEMPLATE_FLAG_FENCE_WOOD),
        (1, MODEL_TEMPLATE_FLAG_FENCE_NETHER),
    ] {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model);
        let base = visual.model_template;
        let post = compiled.model_templates[base as usize];
        assert_eq!(post.flags, expected_flag);
        assert_eq!(post.quad_count, 6);
        assert_eq!(
            model_bounds(template_quads(&compiled, base)),
            ([96, 0, 96], [160, 256, 160])
        );
        for mask in 0_u32..16 {
            let arms = compiled.model_templates[(base + 1 + mask) as usize];
            assert_eq!(arms.flags, expected_flag, "id={id} mask={mask:#06b}");
            assert_eq!(arms.quad_count, mask.count_ones() * 8);
            assert!(arms.quad_count <= 32);
            for rail in template_quads(&compiled, base + 1 + mask).chunks_exact(4) {
                let (min, max) = model_bounds(rail);
                assert!(matches!((min[1], max[1]), (96, 144) | (192, 240)));
            }
        }
    }
    assert!(compiled.materials.iter().all(|material| {
        material.flags & (MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_ALPHA_BLEND) == 0
    }));
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_pane_and_fence_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode connected-family registry")
        .into_iter()
        .filter(|record| matches!(record.model_family, ModelFamily::Pane | ModelFamily::Fence))
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Pane)
            .count(),
        43
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Fence)
            .count(),
        13
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 74_000 + id as u32;
    }
    let sources = read_pack(Path::new(&pack)).expect("read pinned connected pack");
    for record in records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Pane)
    {
        for face in [BlockFace::North, BlockFace::East] {
            let key = resolve_texture_key(&sources.blocks, record, face)
                .key
                .unwrap_or_else(|| panic!("{} missing {face:?} key", record.name));
            assert!(
                sources.terrain.get(&key).is_some(),
                "{} {face:?} terrain key {key} is missing",
                record.name
            );
        }
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned panes/fences");
    let diagnostics = records
        .iter()
        .enumerate()
        .filter(|(id, _)| compiled.visuals[*id].kind == VisualKind::Diagnostic)
        .map(|(_, record)| record.name.as_ref())
        .collect::<Vec<_>>();
    assert!(
        diagnostics.is_empty(),
        "diagnostic connected states: {diagnostics:?}"
    );
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        let template = compiled.model_templates[visual.model_template as usize];
        match record.model_family {
            ModelFamily::Pane => {
                assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_PANE, "{}", record.name);
                let expected_alpha = if record.name.contains("stained_glass_pane") {
                    MATERIAL_FLAG_ALPHA_BLEND
                } else {
                    MATERIAL_FLAG_ALPHA_CUTOUT
                };
                for template in &compiled.model_templates
                    [visual.model_template as usize..visual.model_template as usize + 16]
                {
                    let quads = &compiled.model_quads[template.quad_start as usize
                        ..(template.quad_start + template.quad_count) as usize];
                    assert!(quads.iter().all(|quad| {
                        compiled.materials[quad.material as usize].flags & expected_alpha != 0
                    }));
                }
            }
            ModelFamily::Fence => {
                assert!(matches!(
                    template.flags,
                    MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER
                ));
                let expected_alpha =
                    u32::from(record.name.contains("bamboo")) * MATERIAL_FLAG_ALPHA_CUTOUT;
                for template in &compiled.model_templates
                    [visual.model_template as usize..visual.model_template as usize + 17]
                {
                    let quads = &compiled.model_quads[template.quad_start as usize
                        ..(template.quad_start + template.quad_count) as usize];
                    assert!(quads.iter().all(|quad| {
                        compiled.materials[quad.material as usize].flags
                            & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
                            == expected_alpha
                    }));
                }
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_wall_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Wall)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 5_184);
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 80_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned walls");
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert_eq!(record.face_coverage, 0);
        let template = compiled.model_templates[visual.model_template as usize];
        assert!(template.quad_count <= 30);
        assert_eq!(template.quad_count % 6, 0);
        assert!(compiled_model_quads(&compiled, id).iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned walls");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed pinned walls");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn generated_pressure_plate_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| {
            record.name.as_ref() == name && record.model_family == ModelFamily::PressurePlate
        })
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.canonical_state.clone());
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 90_000 + id as u32;
    }
    records
}

fn write_pressure_plate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"wooden_pressure_plate":{"textures":{
            "west":"plate_west","east":"plate_east","down":"plate_down",
            "up":"plate_up","north":"plate_north","south":"plate_south"
        }}}"#,
        r#"{"texture_data":{
            "plate_west":{"textures":"textures/blocks/plate_west"},
            "plate_east":{"textures":"textures/blocks/plate_east"},
            "plate_down":{"textures":"textures/blocks/plate_down"},
            "plate_up":{"textures":"textures/blocks/plate_up"},
            "plate_north":{"textures":"textures/blocks/plate_north"},
            "plate_south":{"textures":"textures/blocks/plate_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/plate_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 31 + 1, 70, 110, 255]),
        );
    }
}

#[test]
fn compiler_routes_all_generated_pressure_plate_states_to_exact_opaque_cuboids() {
    const PRESSED: u32 = 1 << 1;
    let directory = tempfile::tempdir().expect("create pressure-plate fixture");
    write_pressure_plate_pack(directory.path());
    let records = generated_pressure_plate_records("minecraft:wooden_pressure_plate");
    assert_eq!(records.len(), 16, "redstone signal 0..15");
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_state.get(ModelStateField::Flags) == Some(0))
            .count(),
        1,
        "only signal zero is unpressed"
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_state.get(ModelStateField::Flags) == Some(PRESSED))
            .count(),
        15,
        "signals 1..15 are pressed"
    );

    let compiled = compile_pack(directory.path(), &records).expect("compile all pressure plates");
    assert_eq!(
        compiled.materials.len(),
        7,
        "diagnostic plus six opaque faces"
    );
    assert_eq!(compiled.model_templates.len(), 2, "up and down templates");
    for (id, record) in records.iter().enumerate() {
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        assert_eq!(
            model_bounds(quads),
            if flags == 0 {
                ([16, 0, 16], [240, 16, 240])
            } else {
                ([16, 0, 16], [240, 8, 240])
            },
            "{}",
            record.canonical_state
        );
        let side_bottom_v = if flags == 0 { 4096 } else { 3968 };
        let side_top_v = 3840;
        let expected_uvs = [
            [
                [256, side_bottom_v],
                [3840, side_bottom_v],
                [3840, side_top_v],
                [256, side_top_v],
            ],
            [
                [256, side_bottom_v],
                [256, side_top_v],
                [3840, side_top_v],
                [3840, side_bottom_v],
            ],
            [[256, 256], [3840, 256], [3840, 3840], [256, 3840]],
            [[256, 256], [256, 3840], [3840, 3840], [3840, 256]],
            [
                [256, side_bottom_v],
                [256, side_top_v],
                [3840, side_top_v],
                [3840, side_bottom_v],
            ],
            [
                [256, side_bottom_v],
                [3840, side_bottom_v],
                [3840, side_top_v],
                [256, side_top_v],
            ],
        ];
        for (face, quad) in quads.iter().enumerate() {
            assert_eq!(quad.material, visual.faces[face]);
            assert_eq!(quad.flags, [3, 4, 1, 2, 5, 6][face]);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.uvs, expected_uvs[face]);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive pressure plates");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "pressure-plate compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed pressure-plate render geometry"
    );
}

#[test]
fn compiler_pressure_plate_selector_fails_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid pressure-plate fixture");
    write_pressure_plate_pack(directory.path());
    let mut records = vec![model_record(
        0,
        91_000,
        "minecraft:wooden_pressure_plate",
        "{}",
        ModelFamily::PressurePlate,
    )];
    for flags in [1, 3, 4, u32::MAX] {
        let id = records.len() as u32;
        records.push(encoded_model_record(
            id,
            91_000 + id,
            "minecraft:wooden_pressure_plate",
            ModelFamily::PressurePlate,
            &[(ModelStateField::Flags, flags)],
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid selectors");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_pressure_plate_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::PressurePlate)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 256);
    assert_eq!(
        records
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        16
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 92_000 + id as u32;
    }
    let compiled =
        compile_pack(Path::new(&pack), &records).expect("compile pinned pressure plates");
    for (id, record) in records.iter().enumerate() {
        assert_eq!(
            compiled.visuals[id].kind,
            VisualKind::Model,
            "{}",
            record.name
        );
        assert_eq!(record.face_coverage, 0);
        assert!(compiled_model_quads(&compiled, id).iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL
                && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
                && compiled.materials[quad.material as usize].flags == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned pressure plates");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed plates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn generated_carpet_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Carpet)
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_000 + id as u32;
    }
    records
}

fn write_carpet_pack(root: &Path) {
    let colours = [
        "black",
        "blue",
        "brown",
        "cyan",
        "gray",
        "green",
        "light_blue",
        "silver",
        "lime",
        "magenta",
        "orange",
        "pink",
        "purple",
        "red",
        "white",
        "yellow",
    ];
    let blocks = [
        ("black_carpet", "wool_colored_black"),
        ("blue_carpet", "wool_colored_blue"),
        ("brown_carpet", "wool_colored_brown"),
        ("cyan_carpet", "wool_colored_cyan"),
        ("gray_carpet", "wool_colored_gray"),
        ("green_carpet", "wool_colored_green"),
        ("light_blue_carpet", "wool_colored_light_blue"),
        ("light_gray_carpet", "wool_colored_silver"),
        ("lime_carpet", "wool_colored_lime"),
        ("magenta_carpet", "wool_colored_magenta"),
        ("moss_carpet", "moss_block"),
        ("orange_carpet", "wool_colored_orange"),
        ("pale_moss_carpet", "pale_moss_block"),
        ("pink_carpet", "wool_colored_pink"),
        ("purple_carpet", "wool_colored_purple"),
        ("red_carpet", "wool_colored_red"),
        ("white_carpet", "wool_colored_white"),
        ("yellow_carpet", "wool_colored_yellow"),
    ];
    let block_json = format!(
        "{{{}}}",
        blocks
            .into_iter()
            .map(|(name, key)| format!(r#""{name}":{{"textures":"{key}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut terrain = colours
        .into_iter()
        .map(|colour| {
            format!(
                r#""wool_colored_{colour}":{{"textures":"textures/blocks/wool_colored_{colour}"}}"#
            )
        })
        .collect::<Vec<_>>();
    terrain.extend([
        r#""moss_block":{"textures":"textures/blocks/moss_block"}"#.to_owned(),
        r#""pale_moss_block":{"textures":"textures/blocks/pale_moss_block"}"#.to_owned(),
        r#""pale_moss_carpet_side":{"textures":["textures/blocks/pale_moss_carpet_side_base","textures/blocks/pale_moss_carpet_side_tip"]}"#.to_owned(),
    ]);
    write_pack(
        root,
        &block_json,
        &format!(r#"{{"texture_data":{{{}}}}}"#, terrain.join(",")),
        "[]",
    );
    for (index, colour) in colours.into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/wool_colored_{colour}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 50, 90, 255]),
        );
    }
    for (index, path) in ["moss_block", "pale_moss_block"].into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [80 + index as u8, 120, 40, 255]),
        );
    }
    for (index, path) in ["pale_moss_carpet_side_base", "pale_moss_carpet_side_tip"]
        .into_iter()
        .enumerate()
    {
        let mut pixels = solid(TILE_SIZE, TILE_SIZE, [30 + index as u8, 90, 20, 255]);
        for (pixel_index, pixel) in pixels.iter_mut().enumerate() {
            if pixel_index % (index + 2) == 0 {
                pixel[3] = 0;
            }
        }
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
}

fn carpet_state_value<'a>(
    state: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
    expected_type: &str,
) -> &'a serde_json::Value {
    let value = state[name].as_object().expect("typed carpet selector");
    assert_eq!(value["type"], expected_type);
    &value["value"]
}

fn pale_carpet_selector(record: &RegistryRecord) -> ([u8; 4], bool) {
    assert_eq!(
        record.model_state.mask(),
        1 << (ModelStateField::Flags as u8 - 1)
    );
    let flags = record
        .model_state
        .get(ModelStateField::Flags)
        .expect("pale carpet flags");
    assert!(matches!(flags, 0 | MODEL_FLAG_UPPER));
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .expect("parse pale carpet state");
    assert_eq!(state.len(), 5);
    let side = |direction| match carpet_state_value(
        &state,
        &format!("pale_moss_carpet_side_{direction}"),
        "string",
    )
    .as_str()
    .expect("string side")
    {
        "none" => 0,
        "short" => 1,
        "tall" => 2,
        value => panic!("invalid side {value}"),
    };
    let upper = carpet_state_value(&state, "upper_block_bit", "byte")
        .as_u64()
        .expect("byte upper bit")
        != 0;
    assert_eq!(upper, flags == MODEL_FLAG_UPPER);
    (
        [side("east"), side("north"), side("south"), side("west")],
        upper,
    )
}

#[test]
fn generated_carpet_registry_has_exact_ordinary_and_pale_selector_contract() {
    let records = generated_carpet_records();
    assert_eq!(records.len(), 179);
    let ordinary = records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:pale_moss_carpet")
        .collect::<Vec<_>>();
    assert_eq!(ordinary.len(), 17);
    assert!(ordinary.iter().all(|record| {
        record.canonical_state.as_ref() == "{}" && record.model_state.mask() == 0
    }));
    let pale = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:pale_moss_carpet")
        .collect::<Vec<_>>();
    assert_eq!(pale.len(), 162);
    let selectors = pale
        .into_iter()
        .map(pale_carpet_selector)
        .collect::<HashSet<_>>();
    let expected = (0..3)
        .flat_map(|east| {
            (0..3).flat_map(move |north| {
                (0..3).flat_map(move |south| {
                    (0..3).flat_map(move |west| {
                        [false, true]
                            .into_iter()
                            .map(move |upper| ([east, north, south, west], upper))
                    })
                })
            })
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors, expected);
}

#[test]
fn compiler_covers_all_carpet_states_with_exact_geometry_materials_and_determinism() {
    let directory = tempfile::tempdir().expect("create carpet fixture");
    write_carpet_pack(directory.path());
    let records = generated_carpet_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile all carpets");
    assert_eq!(
        compiled.materials.len(),
        21,
        "diagnostic, 18 opaque, two cutout"
    );
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert_eq!(record.face_coverage, 0);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let quads = compiled_model_quads(&compiled, id);
        if record.name.as_ref() != "minecraft:pale_moss_carpet" {
            assert_eq!(quads.len(), 6);
            assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 16, 256]));
            assert!(quads.iter().all(|quad| {
                quad.material != DIAGNOSTIC_MATERIAL
                    && compiled.materials[quad.material as usize].flags == 0
                    && quad.flags & (MODEL_QUAD_FLAG_CULL_FACE_MASK | MODEL_QUAD_FLAG_TWO_SIDED)
                        == 0
            }));
            continue;
        }
        let (sides, upper) = pale_carpet_selector(record);
        let isolated_upper = upper && sides == [0; 4];
        let has_base = !upper || isolated_upper;
        let side_count = if isolated_upper {
            4
        } else {
            sides.into_iter().filter(|side| *side != 0).count()
        };
        assert_eq!(quads.len(), usize::from(has_base) * 6 + side_count * 2);
        let base_count = usize::from(has_base) * 6;
        if has_base {
            assert_eq!(model_bounds(&quads[..6]), ([0, 0, 0], [256, 16, 256]));
            assert!(quads[..6].iter().all(|quad| {
                compiled.materials[quad.material as usize].flags == 0
                    && quad.flags & MODEL_QUAD_FLAG_TWO_SIDED == 0
            }));
        }
        for quad in &quads[base_count..] {
            assert_eq!(
                compiled.materials[quad.material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert!(
                quad.uvs
                    .iter()
                    .flatten()
                    .all(|value| matches!(*value, 0 | 4096))
            );
            let face = quad.flags & MODEL_QUAD_FLAG_FACE_MASK;
            let bounds = model_bounds(std::slice::from_ref(quad));
            assert!(matches!(
                (face, bounds),
                (3 | 4, ([2, 0, 0], [2, 256, 256]))
                    | (3 | 4, ([254, 0, 0], [254, 256, 256]))
                    | (5 | 6, ([0, 0, 2], [256, 256, 2]))
                    | (5 | 6, ([0, 0, 254], [256, 256, 254]))
            ));
        }
    }
    let first_material_pixel = |name: &str, selector: Option<([u8; 4], bool)>| {
        let id = records
            .iter()
            .position(|record| {
                record.name.as_ref() == name
                    && selector.is_none_or(|selector| pale_carpet_selector(record) == selector)
            })
            .expect("requested carpet state");
        let quad = compiled_model_quads(&compiled, id)
            .last()
            .expect("carpet template quad");
        let material = compiled.materials[quad.material as usize];
        assert_eq!(material.texture.page(), 0);
        mip_pixel(&compiled, 0, material.texture.layer(), 0, 0)
    };
    assert_eq!(
        first_material_pixel("minecraft:light_gray_carpet", None),
        [8, 50, 90, 255],
        "light gray must select wool_colored_silver"
    );
    assert_eq!(
        first_material_pixel("minecraft:moss_carpet", None),
        [80, 120, 40, 255],
        "moss carpet must select moss_block"
    );
    assert_eq!(
        first_material_pixel("minecraft:pale_moss_carpet", Some(([1, 0, 0, 0], false))),
        [31, 90, 20, 0],
        "short must select pair index 1 / side_tip / Java small"
    );
    assert_eq!(
        first_material_pixel("minecraft:pale_moss_carpet", Some(([2, 0, 0, 0], false))),
        [30, 90, 20, 0],
        "tall must select pair index 0 / side_base / Java tall"
    );
    let baseline = encode_blob(&compiled).expect("encode carpets");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "carpet compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed carpet render geometry"
    );
}

#[test]
fn compiler_pale_moss_side_planes_preserve_both_java_face_uv_orders() {
    let directory = tempfile::tempdir().expect("create pale moss UV fixture");
    write_carpet_pack(directory.path());
    let generated = generated_carpet_records();
    let selectors = [
        ([2, 0, 0, 0], true),
        ([0, 2, 0, 0], true),
        ([0, 0, 2, 0], true),
        ([0, 0, 0, 2], true),
    ];
    let mut records = selectors
        .into_iter()
        .map(|selector| {
            generated
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:pale_moss_carpet"
                        && pale_carpet_selector(record) == selector
                })
                .expect("requested pale moss direction")
                .clone()
        })
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_500 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile pale moss UV fixture");

    let expected = [
        (
            "east",
            4,
            3,
            [[254, 0, 0], [254, 256, 0], [254, 256, 256], [254, 0, 256]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[254, 0, 0], [254, 0, 256], [254, 256, 256], [254, 256, 0]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            "north",
            5,
            6,
            [[0, 0, 2], [0, 256, 2], [256, 256, 2], [256, 0, 2]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[0, 0, 2], [256, 0, 2], [256, 256, 2], [0, 256, 2]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            "south",
            6,
            5,
            [[0, 0, 254], [256, 0, 254], [256, 256, 254], [0, 256, 254]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[0, 0, 254], [0, 256, 254], [256, 256, 254], [256, 0, 254]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
        (
            "west",
            3,
            4,
            [[2, 0, 0], [2, 0, 256], [2, 256, 256], [2, 256, 0]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[2, 0, 0], [2, 256, 0], [2, 256, 256], [2, 0, 256]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
    ];
    for (
        id,
        (
            direction,
            outward_face,
            inward_face,
            outward_positions,
            outward_uvs,
            inward_positions,
            inward_uvs,
        ),
    ) in expected.into_iter().enumerate()
    {
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(
            quads.len(),
            2,
            "one explicit quad per Java {direction} face"
        );
        assert_eq!(
            quads[0].positions, outward_positions,
            "{direction} outward positions"
        );
        assert_eq!(quads[0].uvs, outward_uvs, "{direction} outward UVs");
        assert_eq!(quads[0].flags, outward_face, "{direction} outward face");
        assert_eq!(
            quads[1].positions, inward_positions,
            "{direction} inward positions"
        );
        assert_eq!(quads[1].uvs, inward_uvs, "{direction} inward UVs");
        assert_eq!(quads[1].flags, inward_face, "{direction} inward face");
        assert_eq!(quads[0].material, quads[1].material);
        assert_eq!(
            compiled.materials[quads[0].material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
}

#[test]
fn compiler_carpet_selectors_fail_closed_when_missing_invalid_or_extra() {
    let directory = tempfile::tempdir().expect("create invalid carpet fixture");
    write_carpet_pack(directory.path());
    let generated = generated_carpet_records();
    let ordinary = generated
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:black_carpet")
        .unwrap();
    let pale = generated
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:pale_moss_carpet"
                && pale_carpet_selector(record) == ([0; 4], false)
        })
        .unwrap();
    let typed = |fields: &[(ModelStateField, u32)]| {
        encoded_model_record(
            0,
            1,
            "minecraft:pale_moss_carpet",
            ModelFamily::Carpet,
            fields,
        )
        .model_state
    };
    let mut records = Vec::new();
    let mut extra_ordinary = ordinary.clone();
    extra_ordinary.model_state = typed(&[(ModelStateField::Flags, 0)]);
    records.push(extra_ordinary);
    let mut missing_typed = pale.clone();
    missing_typed.model_state = ModelState::default();
    records.push(missing_typed);
    let mut invalid_flags = pale.clone();
    invalid_flags.model_state = typed(&[(ModelStateField::Flags, 1)]);
    records.push(invalid_flags);
    let mut extra_typed = pale.clone();
    extra_typed.model_state = typed(&[(ModelStateField::Flags, 0), (ModelStateField::Half, 0)]);
    records.push(extra_typed);
    for state in [
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"extra":{"type":"byte","value":0},"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"low"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"byte","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":1}}"#,
    ] {
        let mut invalid = pale.clone();
        invalid.canonical_state = state.into();
        records.push(invalid);
    }
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 96_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid carpets");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_carpet_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = generated_carpet_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned carpets");
    assert_eq!(records.len(), 179);
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model)
    );
    assert!(
        compiled
            .model_quads
            .iter()
            .all(|quad| quad.material != DIAGNOSTIC_MATERIAL)
    );
    let baseline = encode_blob(&compiled).expect("encode pinned carpets");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed carpets");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

const BUTTON_PRESSED_FLAG: u32 = 1 << 1;

fn generated_button_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Button)
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 97_000 + id as u32;
    }
    records
}

fn write_button_pack(root: &Path) {
    let mappings = [
        ("acacia_button", "acacia_planks", "planks_acacia"),
        ("bamboo_button", "bamboo_planks", "bamboo_planks"),
        ("birch_button", "birch_planks", "planks_birch"),
        ("cherry_button", "cherry_planks", "cherry_planks"),
        (
            "crimson_button",
            "crimson_planks",
            "huge_fungus/crimson_planks",
        ),
        ("dark_oak_button", "dark_oak_planks", "planks_big_oak"),
        ("jungle_button", "jungle_planks", "planks_jungle"),
        ("mangrove_button", "mangrove_planks", "mangrove_planks"),
        ("pale_oak_button", "pale_oak_planks", "pale_oak_planks"),
        (
            "polished_blackstone_button",
            "polished_blackstone",
            "polished_blackstone",
        ),
        ("spruce_button", "spruce_planks", "planks_spruce"),
        ("stone_button", "stone", "stone"),
        (
            "warped_button",
            "warped_planks",
            "huge_fungus/warped_planks",
        ),
        ("wooden_button", "planks", "planks_oak"),
    ];
    let blocks = mappings
        .iter()
        .map(|(name, key, _)| format!(r#""{name}":{{"textures":"{key}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let terrain = mappings
        .iter()
        .map(|(_, key, path)| {
            if matches!(*key, "planks" | "stone") {
                format!(
                    r#""{key}":{{"textures":["textures/blocks/{path}","textures/blocks/button_unused_variant"]}}"#
                )
            } else {
                format!(r#""{key}":{{"textures":"textures/blocks/{path}"}}"#)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    write_pack(
        root,
        &format!("{{{blocks}}}"),
        &format!(r#"{{"texture_data":{{{terrain}}}}}"#),
        "[]",
    );
    for (index, (_, _, path)) in mappings.into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 71, 113, 255]),
        );
    }
    write_png(
        root,
        "textures/blocks/button_unused_variant",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [255, 0, 255, 255]),
    );
}

fn button_selector(record: &RegistryRecord) -> (u32, bool) {
    assert_eq!(record.model_state.mask(), 0x81);
    let orientation = record
        .model_state
        .get(ModelStateField::Orientation)
        .expect("button orientation");
    let flags = record
        .model_state
        .get(ModelStateField::Flags)
        .expect("button flags");
    assert!(orientation <= 5);
    assert!(matches!(flags, 0 | BUTTON_PRESSED_FLAG));
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .expect("parse button canonical state");
    assert_eq!(state.len(), 2);
    let pressed = carpet_state_value(&state, "button_pressed_bit", "byte")
        .as_u64()
        .expect("byte pressed bit");
    let facing = carpet_state_value(&state, "facing_direction", "int")
        .as_u64()
        .expect("integer facing direction");
    assert_eq!(facing, u64::from(orientation));
    assert_eq!(pressed == 1, flags == BUTTON_PRESSED_FLAG);
    (orientation, pressed == 1)
}

fn button_expected_bounds(orientation: u32, pressed: bool) -> ([i16; 3], [i16; 3]) {
    let h = if pressed { 16 } else { 32 };
    match orientation {
        0 => ([80, 256 - h, 96], [176, 256, 160]),
        1 => ([80, 0, 96], [176, h, 160]),
        2 => ([80, 96, 256 - h], [176, 160, 256]),
        3 => ([80, 96, 0], [176, 160, h]),
        4 => ([256 - h, 96, 80], [256, 160, 176]),
        5 => ([0, 96, 80], [h, 160, 176]),
        _ => panic!("invalid button orientation {orientation}"),
    }
}

fn button_face_positions(
    face: BlockFace,
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [[i16; 3]; 4] {
    match face {
        BlockFace::West => [
            [min_x, min_y, min_z],
            [min_x, min_y, max_z],
            [min_x, max_y, max_z],
            [min_x, max_y, min_z],
        ],
        BlockFace::East => [
            [max_x, min_y, min_z],
            [max_x, max_y, min_z],
            [max_x, max_y, max_z],
            [max_x, min_y, max_z],
        ],
        BlockFace::Down => [
            [min_x, min_y, min_z],
            [max_x, min_y, min_z],
            [max_x, min_y, max_z],
            [min_x, min_y, max_z],
        ],
        BlockFace::Up => [
            [min_x, max_y, min_z],
            [min_x, max_y, max_z],
            [max_x, max_y, max_z],
            [max_x, max_y, min_z],
        ],
        BlockFace::North => [
            [min_x, min_y, min_z],
            [min_x, max_y, min_z],
            [max_x, max_y, min_z],
            [max_x, min_y, min_z],
        ],
        BlockFace::South => [
            [min_x, min_y, max_z],
            [max_x, min_y, max_z],
            [max_x, max_y, max_z],
            [min_x, max_y, max_z],
        ],
    }
}

fn button_face_uvs(face: BlockFace, [u1, v1, u2, v2]: [u16; 4]) -> [[u16; 2]; 4] {
    let [u1, v1, u2, v2] = [u1, v1, u2, v2].map(|value| value * 256);
    match face {
        BlockFace::West | BlockFace::South => [[u1, v2], [u2, v2], [u2, v1], [u1, v1]],
        BlockFace::East | BlockFace::North => [[u1, v2], [u1, v1], [u2, v1], [u2, v2]],
        BlockFace::Down => [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        BlockFace::Up => [[u1, v1], [u1, v2], [u2, v2], [u2, v1]],
    }
}

fn button_source_rect(face: BlockFace, pressed: bool) -> [u16; 4] {
    match face {
        BlockFace::Down | BlockFace::Up => [5, 6, 11, 10],
        BlockFace::North | BlockFace::South => [5, 14, 11, if pressed { 15 } else { 16 }],
        BlockFace::West | BlockFace::East => [6, 14, 10, if pressed { 15 } else { 16 }],
    }
}

fn expected_button_uvlock_rect(
    face: BlockFace,
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [u16; 4] {
    let [min_x, min_y, min_z, max_x, max_y, max_z] =
        [min_x, min_y, min_z, max_x, max_y, max_z].map(|value| value as u16 / 16);
    match face {
        BlockFace::West | BlockFace::East => [min_z, 16 - max_y, max_z, 16 - min_y],
        BlockFace::North | BlockFace::South => [min_x, 16 - max_y, max_x, 16 - min_y],
        BlockFace::Down | BlockFace::Up => [min_x, min_z, max_x, max_z],
    }
}

fn button_rotated_face(face: BlockFace, orientation: u32) -> BlockFace {
    let after_x90 = match face {
        BlockFace::West => BlockFace::West,
        BlockFace::East => BlockFace::East,
        BlockFace::Down => BlockFace::South,
        BlockFace::Up => BlockFace::North,
        BlockFace::North => BlockFace::Down,
        BlockFace::South => BlockFace::Up,
    };
    let yaw = |face, turns| {
        let mut face = face;
        for _ in 0..turns {
            face = match face {
                BlockFace::North => BlockFace::East,
                BlockFace::East => BlockFace::South,
                BlockFace::South => BlockFace::West,
                BlockFace::West => BlockFace::North,
                vertical => vertical,
            };
        }
        face
    };
    match orientation {
        0 => match face {
            BlockFace::Down => BlockFace::Up,
            BlockFace::Up => BlockFace::Down,
            BlockFace::North => BlockFace::South,
            BlockFace::South => BlockFace::North,
            horizontal => horizontal,
        },
        1 => face,
        2 => after_x90,
        3 => yaw(after_x90, 2),
        4 => yaw(after_x90, 3),
        5 => yaw(after_x90, 1),
        _ => panic!("invalid button orientation {orientation}"),
    }
}

fn button_rotate_position([x, y, z]: [i16; 3], orientation: u32) -> [i16; 3] {
    match orientation {
        0 => [x, 256 - y, 256 - z],
        1 => [x, y, z],
        2 => [x, z, 256 - y],
        3 => [256 - x, z, y],
        4 => [256 - y, z, 256 - x],
        5 => [y, z, x],
        _ => panic!("invalid button orientation {orientation}"),
    }
}

type ExpectedButtonQuad = (u32, [[i16; 3]; 4], [[u16; 2]; 4]);

fn expected_button_quads(orientation: u32, pressed: bool) -> [ExpectedButtonQuad; 6] {
    let h = if pressed { 16 } else { 32 };
    let source_min = [80, 0, 96];
    let source_max = [176, h, 160];
    let (target_min, target_max) = button_expected_bounds(orientation, pressed);
    BlockFace::ALL.map(|source_face| {
        let target_face = button_rotated_face(source_face, orientation);
        let source_positions = button_face_positions(source_face, source_min, source_max);
        let positions = if orientation <= 1 {
            source_positions.map(|position| button_rotate_position(position, orientation))
        } else {
            button_face_positions(target_face, target_min, target_max)
        };
        let source_uvs = button_face_uvs(source_face, button_source_rect(source_face, pressed));
        let uvs = if orientation <= 1 {
            source_uvs
        } else {
            button_face_uvs(
                target_face,
                expected_button_uvlock_rect(target_face, target_min, target_max),
            )
        };
        (target_face as u32, positions, uvs)
    })
}

fn wall_button_uv_golden(orientation: u32, pressed: bool) -> [[[u16; 2]; 4]; 6] {
    match (orientation, pressed) {
        (2, false) => [
            [[3584, 2560], [4096, 2560], [4096, 1536], [3584, 1536]],
            [[3584, 2560], [3584, 1536], [4096, 1536], [4096, 2560]],
            [[1280, 3584], [2816, 3584], [2816, 4096], [1280, 4096]],
            [[1280, 3584], [1280, 4096], [2816, 4096], [2816, 3584]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (2, true) => [
            [[3840, 2560], [4096, 2560], [4096, 1536], [3840, 1536]],
            [[3840, 2560], [3840, 1536], [4096, 1536], [4096, 2560]],
            [[1280, 3840], [2816, 3840], [2816, 4096], [1280, 4096]],
            [[1280, 3840], [1280, 4096], [2816, 4096], [2816, 3840]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (3, false) => [
            [[0, 2560], [512, 2560], [512, 1536], [0, 1536]],
            [[0, 2560], [0, 1536], [512, 1536], [512, 2560]],
            [[1280, 0], [2816, 0], [2816, 512], [1280, 512]],
            [[1280, 0], [1280, 512], [2816, 512], [2816, 0]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (3, true) => [
            [[0, 2560], [256, 2560], [256, 1536], [0, 1536]],
            [[0, 2560], [0, 1536], [256, 1536], [256, 2560]],
            [[1280, 0], [2816, 0], [2816, 256], [1280, 256]],
            [[1280, 0], [1280, 256], [2816, 256], [2816, 0]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (4, false) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[3584, 1280], [4096, 1280], [4096, 2816], [3584, 2816]],
            [[3584, 1280], [3584, 2816], [4096, 2816], [4096, 1280]],
            [[3584, 2560], [3584, 1536], [4096, 1536], [4096, 2560]],
            [[3584, 2560], [4096, 2560], [4096, 1536], [3584, 1536]],
        ],
        (4, true) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[3840, 1280], [4096, 1280], [4096, 2816], [3840, 2816]],
            [[3840, 1280], [3840, 2816], [4096, 2816], [4096, 1280]],
            [[3840, 2560], [3840, 1536], [4096, 1536], [4096, 2560]],
            [[3840, 2560], [4096, 2560], [4096, 1536], [3840, 1536]],
        ],
        (5, false) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[0, 1280], [512, 1280], [512, 2816], [0, 2816]],
            [[0, 1280], [0, 2816], [512, 2816], [512, 1280]],
            [[0, 2560], [0, 1536], [512, 1536], [512, 2560]],
            [[0, 2560], [512, 2560], [512, 1536], [0, 1536]],
        ],
        (5, true) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[0, 1280], [256, 1280], [256, 2816], [0, 2816]],
            [[0, 1280], [0, 2816], [256, 2816], [256, 1280]],
            [[0, 2560], [0, 1536], [256, 1536], [256, 2560]],
            [[0, 2560], [256, 2560], [256, 1536], [0, 1536]],
        ],
        _ => panic!("wall button golden requires orientation 2..=5"),
    }
}

#[test]
fn compiler_button_wall_uvlock_matches_independent_target_space_goldens() {
    let directory = tempfile::tempdir().expect("create button UV-lock fixture");
    write_button_pack(directory.path());
    let generated = generated_button_records();
    let selectors = (2..=5)
        .flat_map(|orientation| [false, true].map(move |pressed| (orientation, pressed)))
        .collect::<Vec<_>>();
    let mut records = selectors
        .iter()
        .map(|selector| {
            generated
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:stone_button"
                        && button_selector(record) == *selector
                })
                .expect("requested wall button selector")
                .clone()
        })
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 97_500 + id as u32;
    }
    let compiled =
        compile_pack(directory.path(), &records).expect("compile button UV-lock fixture");
    for (id, &(orientation, pressed)) in selectors.iter().enumerate() {
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        let golden = wall_button_uv_golden(orientation, pressed);
        for face in BlockFace::ALL {
            let quad = quads
                .iter()
                .find(|quad| quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face as u32)
                .expect("one quad for each target wall face");
            assert_eq!(
                quad.uvs, golden[face as usize],
                "orientation {orientation} pressed {pressed} target face {face:?}"
            );
        }
    }
}

#[test]
fn generated_button_registry_has_exact_names_and_selector_matrix() {
    let records = generated_button_records();
    assert_eq!(records.len(), 168);
    let expected_names = [
        "minecraft:acacia_button",
        "minecraft:bamboo_button",
        "minecraft:birch_button",
        "minecraft:cherry_button",
        "minecraft:crimson_button",
        "minecraft:dark_oak_button",
        "minecraft:jungle_button",
        "minecraft:mangrove_button",
        "minecraft:pale_oak_button",
        "minecraft:polished_blackstone_button",
        "minecraft:spruce_button",
        "minecraft:stone_button",
        "minecraft:warped_button",
        "minecraft:wooden_button",
    ];
    for name in expected_names {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 12, "{name}");
        let selectors = selected
            .into_iter()
            .map(button_selector)
            .collect::<HashSet<_>>();
        assert_eq!(
            selectors,
            (0..6)
                .flat_map(|facing| [false, true].map(move |pressed| (facing, pressed)))
                .collect()
        );
    }
}

#[test]
fn compiler_covers_all_button_states_with_exact_geometry_uvs_materials_and_determinism() {
    let directory = tempfile::tempdir().expect("create button fixture");
    write_button_pack(directory.path());
    let records = generated_button_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile all buttons");
    assert_eq!(
        compiled.materials.len(),
        15,
        "diagnostic plus fourteen button materials"
    );
    for (id, record) in records.iter().enumerate() {
        let (orientation, pressed) = button_selector(record);
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert_eq!(record.face_coverage, 0);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        assert_eq!(
            model_bounds(quads),
            button_expected_bounds(orientation, pressed)
        );
        for (quad, (face, positions, uvs)) in quads
            .iter()
            .zip(expected_button_quads(orientation, pressed))
        {
            assert_eq!(quad.flags, face, "{}", record.canonical_state);
            assert_eq!(quad.positions, positions, "{}", record.canonical_state);
            assert_eq!(quad.uvs, uvs, "{}", record.canonical_state);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
            assert_ne!(quad.material, DIAGNOSTIC_MATERIAL);
        }
    }
    let baseline = encode_blob(&compiled).expect("encode buttons");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "button compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed button render geometry"
    );
}

#[test]
fn compiler_button_selectors_fail_closed_when_missing_invalid_extra_or_mismatched() {
    let directory = tempfile::tempdir().expect("create invalid button fixture");
    write_button_pack(directory.path());
    let valid = generated_button_records()
        .into_iter()
        .find(|record| button_selector(record) == (2, false))
        .expect("north unpressed button");
    let typed = |fields: &[(ModelStateField, u32)]| {
        encoded_model_record(0, 1, "minecraft:stone_button", ModelFamily::Button, fields)
            .model_state
    };
    let mut records = Vec::new();
    for fields in [
        vec![(ModelStateField::Orientation, 2)],
        vec![(ModelStateField::Flags, 0)],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 1),
        ],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 3),
        ],
        vec![
            (ModelStateField::Orientation, 6),
            (ModelStateField::Flags, 0),
        ],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 0),
            (ModelStateField::Half, 0),
        ],
    ] {
        let mut invalid = valid.clone();
        invalid.model_state = typed(&fields);
        records.push(invalid);
    }
    for state in [
        r#"{"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"extra":{"type":"byte","value":0},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"int","value":0},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":2},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"facing_direction":{"type":"int","value":6}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":1},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"facing_direction":{"type":"int","value":3}}"#,
    ] {
        let mut invalid = valid.clone();
        invalid.canonical_state = state.into();
        records.push(invalid);
    }
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 98_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid buttons");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_button_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = generated_button_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned buttons");
    assert_eq!(records.len(), 168);
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model)
    );
    assert!(
        compiled
            .model_quads
            .iter()
            .all(|quad| quad.material != DIAGNOSTIC_MATERIAL)
    );
    let baseline = encode_blob(&compiled).expect("encode pinned buttons");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed buttons");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn generated_gate_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.name.as_ref() == name && record.model_family == ModelFamily::Gate)
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Flags).unwrap(),
            record.model_state.get(ModelStateField::Open).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 93_000 + id as u32;
    }
    records
}

fn write_gate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"fence_gate":{"textures":{
            "west":"gate_west","east":"gate_east","down":"gate_down",
            "up":"gate_up","north":"gate_north","south":"gate_south"
        }}}"#,
        r#"{"texture_data":{
            "gate_west":{"textures":"textures/blocks/gate_west"},
            "gate_east":{"textures":"textures/blocks/gate_east"},
            "gate_down":{"textures":"textures/blocks/gate_down"},
            "gate_up":{"textures":"textures/blocks/gate_up"},
            "gate_north":{"textures":"textures/blocks/gate_north"},
            "gate_south":{"textures":"textures/blocks/gate_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/gate_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 29 + 1, 73, 113, 255]),
        );
    }
}

fn write_bamboo_gate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"bamboo_fence_gate":{"textures":"bamboo_fence_gate"}}"#,
        r#"{"texture_data":{"bamboo_fence_gate":{"textures":"textures/blocks/bamboo_fence_gate"}}}"#,
        "[]",
    );
    let mut pixels = solid(TILE_SIZE, TILE_SIZE, [211, 173, 77, 255]);
    pixels[0][3] = 0;
    write_png(
        root,
        "textures/blocks/bamboo_fence_gate",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
}

fn rotate_gate_face(face: usize, orientation: u32) -> usize {
    const ROTATED: [[usize; 6]; 4] = [
        [0, 1, 2, 3, 4, 5],
        [4, 5, 2, 3, 1, 0],
        [1, 0, 2, 3, 5, 4],
        [5, 4, 2, 3, 0, 1],
    ];
    ROTATED[orientation as usize][face]
}

fn rotate_gate_position([x, y, z]: [i16; 3], orientation: u32) -> [i16; 3] {
    match orientation {
        0 => [x, y, z],
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => unreachable!(),
    }
}

fn gate_face_from_flags(flags: u32) -> usize {
    match flags & MODEL_QUAD_FLAG_FACE_MASK {
        3 => BlockFace::West as usize,
        4 => BlockFace::East as usize,
        1 => BlockFace::Down as usize,
        2 => BlockFace::Up as usize,
        5 => BlockFace::North as usize,
        6 => BlockFace::South as usize,
        face => panic!("invalid gate face {face}"),
    }
}

fn expected_gate_uvs(face: usize, u_min: u16, v_min: u16, u_max: u16, v_max: u16) -> [[u16; 2]; 4] {
    match face {
        0 | 5 => [
            [u_min, v_max],
            [u_max, v_max],
            [u_max, v_min],
            [u_min, v_min],
        ],
        1 | 4 => [
            [u_min, v_max],
            [u_min, v_min],
            [u_max, v_min],
            [u_max, v_max],
        ],
        2 => [
            [u_min, v_min],
            [u_max, v_min],
            [u_max, v_max],
            [u_min, v_max],
        ],
        3 => [
            [u_min, v_min],
            [u_min, v_max],
            [u_max, v_max],
            [u_max, v_min],
        ],
        _ => unreachable!(),
    }
}

fn expected_gate_boxes(open: u32, in_wall: bool, orientation: u32) -> Vec<([i16; 3], [i16; 3])> {
    let source = if open == 0 {
        vec![
            ([0, 80, 112], [32, 256, 144]),
            ([224, 80, 112], [256, 256, 144]),
            ([96, 96, 112], [128, 240, 144]),
            ([128, 96, 112], [160, 240, 144]),
            ([32, 96, 112], [96, 144, 144]),
            ([32, 192, 112], [96, 240, 144]),
            ([160, 96, 112], [224, 144, 144]),
            ([160, 192, 112], [224, 240, 144]),
        ]
    } else {
        vec![
            ([0, 80, 112], [32, 256, 144]),
            ([224, 80, 112], [256, 256, 144]),
            ([0, 96, 208], [32, 240, 240]),
            ([224, 96, 208], [256, 240, 240]),
            ([0, 96, 144], [32, 144, 208]),
            ([0, 192, 144], [32, 240, 208]),
            ([224, 96, 144], [256, 144, 208]),
            ([224, 192, 144], [256, 240, 208]),
        ]
    };
    source
        .into_iter()
        .map(|(mut min, mut max)| {
            if in_wall {
                min[1] -= 48;
                max[1] -= 48;
            }
            let corners = [
                [min[0], min[1], min[2]],
                [min[0], min[1], max[2]],
                [min[0], max[1], min[2]],
                [min[0], max[1], max[2]],
                [max[0], min[1], min[2]],
                [max[0], min[1], max[2]],
                [max[0], max[1], min[2]],
                [max[0], max[1], max[2]],
            ]
            .map(|position| rotate_gate_position(position, orientation));
            let rotated_min = [0, 1, 2].map(|axis| corners.iter().map(|p| p[axis]).min().unwrap());
            let rotated_max = [0, 1, 2].map(|axis| corners.iter().map(|p| p[axis]).max().unwrap());
            (rotated_min, rotated_max)
        })
        .collect()
}

#[test]
fn compiler_routes_all_gate_selectors_to_exact_uv_locked_vanilla_templates() {
    const IN_WALL: u32 = 1 << 6;
    let directory = tempfile::tempdir().expect("create gate fixture");
    write_gate_pack(directory.path());
    let records = generated_gate_records("minecraft:fence_gate");
    assert_eq!(records.len(), 16);
    let selectors = records
        .iter()
        .map(|record| {
            (
                record
                    .model_state
                    .get(ModelStateField::Orientation)
                    .unwrap(),
                record.model_state.get(ModelStateField::Open).unwrap(),
                record.model_state.get(ModelStateField::Flags).unwrap(),
            )
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors.len(), 16);

    let compiled = compile_pack(directory.path(), &records).expect("compile all gate states");
    assert_eq!(
        compiled.materials.len(),
        7,
        "diagnostic plus six opaque faces"
    );
    assert_eq!(compiled.model_templates.len(), 32);
    for (id, record) in records.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        let in_wall = flags == IN_WALL;
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_compound_model_quads(&compiled, id);
        assert_eq!(quads.len(), 40);
        let expected_boxes = expected_gate_boxes(open, in_wall, orientation);
        let mut start = 0;
        for (element, expected) in expected_boxes.into_iter().enumerate() {
            let count = if element < 4 { 6 } else { 4 };
            assert_eq!(model_bounds(&quads[start..start + count]), expected);
            start += count;
        }
        assert_eq!(start, quads.len());
        let base = records
            .iter()
            .position(|candidate| {
                candidate.model_state.get(ModelStateField::Orientation) == Some(0)
                    && candidate.model_state.get(ModelStateField::Open) == Some(open)
                    && candidate.model_state.get(ModelStateField::Flags) == Some(flags)
            })
            .unwrap();
        let base_quads = compiled_compound_model_quads(&compiled, base);
        for (quad, base_quad) in quads.iter().zip(base_quads) {
            let source_face = gate_face_from_flags(base_quad.flags);
            let target_face = rotate_gate_face(source_face, orientation);
            assert_eq!(gate_face_from_flags(quad.flags), target_face);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.material, visual.faces[target_face]);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
            let mut expected_positions = base_quad
                .positions
                .map(|position| rotate_gate_position(position, orientation));
            let mut actual_positions = quad.positions;
            expected_positions.sort_unstable();
            actual_positions.sort_unstable();
            assert_eq!(actual_positions, expected_positions);
            let u_min = base_quad.uvs.iter().map(|uv| uv[0]).min().unwrap();
            let u_max = base_quad.uvs.iter().map(|uv| uv[0]).max().unwrap();
            let v_min = base_quad.uvs.iter().map(|uv| uv[1]).min().unwrap();
            let v_max = base_quad.uvs.iter().map(|uv| uv[1]).max().unwrap();
            assert_eq!(
                quad.uvs,
                expected_gate_uvs(target_face, u_min, v_min, u_max, v_max),
                "orientation={orientation} open={open} in_wall={in_wall}"
            );
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive gates");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "gate compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed gate render geometry"
    );
}

#[test]
fn compiler_bamboo_gate_uses_custom_missing_faces_and_reversed_rotated_uvs() {
    let directory = tempfile::tempdir().expect("create bamboo gate fixture");
    write_bamboo_gate_pack(directory.path());
    let mut records = generated_gate_records("minecraft:bamboo_fence_gate")
        .into_iter()
        .filter(|record| {
            record.model_state.get(ModelStateField::Orientation) == Some(0)
                && record.model_state.get(ModelStateField::Flags) == Some(0)
        })
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.model_state.get(ModelStateField::Open));
    assert_eq!(records.len(), 2);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 94_500 + id as u32;
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile custom bamboo gates");
    let closed = compiled_compound_model_quads(&compiled, 0);
    let open = compiled_compound_model_quads(&compiled, 1);
    assert_eq!(
        closed.len(),
        38,
        "closed bamboo omits two hidden inner faces"
    );
    assert_eq!(open.len(), 40);
    assert_eq!(compiled.model_templates.len(), 4);
    assert_eq!(compiled.model_templates[0].quad_count, 22);
    assert_eq!(compiled.model_templates[1].quad_count, 16);
    assert_eq!(compiled.model_templates[2].quad_count, 24);
    assert_eq!(compiled.model_templates[3].quad_count, 16);
    assert_eq!(
        compiled.model_templates[0].flags,
        assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | assets::MODEL_TEMPLATE_FLAG_GATE_AXIS_Z
    );
    assert_eq!(compiled.model_templates[1].flags, 0);

    assert_eq!(
        gate_face_from_flags(closed[12].flags),
        BlockFace::West as usize,
        "closed inner-left keeps west but omits east"
    );
    assert_eq!(
        gate_face_from_flags(closed[17].flags),
        BlockFace::East as usize,
        "closed inner-right keeps east but omits west"
    );
    assert_eq!(
        closed[2].uvs,
        [[4096, 3328], [3584, 3328], [3584, 3840], [4096, 3840]],
        "left post down face preserves the reversed 16..14 U range"
    );
    assert_eq!(
        open[27].uvs,
        [[512, 768], [1536, 768], [1536, 256], [512, 256]],
        "open left bar up face applies the vanilla 270-degree UV rotation"
    );
    assert_eq!(
        open[36].uvs,
        [[3584, 1536], [2560, 1536], [2560, 768], [3584, 768]],
        "upper-right west face preserves the intentional reversed 14..10 U range"
    );
}

#[test]
fn compiler_gate_selectors_fail_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid gate fixture");
    write_gate_pack(directory.path());
    let mut records = vec![model_record(
        0,
        94_000,
        "minecraft:fence_gate",
        "{}",
        ModelFamily::Gate,
    )];
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Flags, 1),
        (ModelStateField::Flags, 65),
        (ModelStateField::Flags, 128),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            94_000 + id,
            "minecraft:fence_gate",
            ModelFamily::Gate,
            &fields,
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid gate states");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_gate_requires_the_exact_typed_selector_mask() {
    let directory = tempfile::tempdir().expect("create exact-mask gate fixture");
    write_gate_pack(directory.path());
    let valid = encoded_model_record(
        0,
        94_400,
        "minecraft:fence_gate",
        ModelFamily::Gate,
        &[
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ],
    );
    assert_eq!(valid.model_state.mask(), 0x85);
    let unexpected = encoded_model_record(
        1,
        94_401,
        "minecraft:fence_gate",
        ModelFamily::Gate,
        &[
            (ModelStateField::Orientation, 0),
            (ModelStateField::Half, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ],
    );
    assert_eq!(unexpected.model_state.mask(), 0x87);

    let compiled = compile_pack(directory.path(), &[valid, unexpected])
        .expect("compile exact and over-specified gate selectors");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Model);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Diagnostic);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_gate_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Gate)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 192);
    assert_eq!(
        records
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        12
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned gates");
    for (id, record) in records.iter().enumerate() {
        assert_eq!(
            compiled.visuals[id].kind,
            VisualKind::Model,
            "{}",
            record.name
        );
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_compound_model_quads(&compiled, id);
        let expected_count = if record.name.as_ref() == "minecraft:bamboo_fence_gate"
            && record.model_state.get(ModelStateField::Open) == Some(0)
        {
            38
        } else {
            40
        };
        assert_eq!(quads.len(), expected_count);
        assert!(quads.iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL
                && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
                && compiled.materials[quad.material as usize].flags == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned gates");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed gates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn generated_flowerbed_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    growth: u32,
    orientation: u32,
) -> RegistryRecord {
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
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

fn generated_vine_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.name.as_ref() == "minecraft:vine")
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("vine direction bits")
    });
    assert_eq!(records.len(), 16, "protocol-1001 vine state count");
    for (mask, record) in records.iter_mut().enumerate() {
        assert_eq!(
            record.model_state.get(ModelStateField::Connections),
            Some(mask as u32),
            "protocol-1001 vine mask ordering"
        );
        record.model_family = ModelFamily::Vine;
        record.sequential_id = mask as u32;
        record.network_hash = 20_000 + mask as u32;
    }
    records
}

fn oriented_vine_pixels() -> Vec<[u8; 4]> {
    (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).map(move |x| {
                [
                    3 + x as u8 * 11,
                    5 + y as u8 * 13,
                    7 + (x as u8 ^ y as u8) * 9,
                    255,
                ]
            })
        })
        .collect()
}

#[test]
fn compiler_compiles_all_vine_masks_as_exact_tinted_attachment_planes() {
    let directory = tempfile::tempdir().expect("create vine fixture");
    write_pack(
        directory.path(),
        r#"{
            "vine":{"textures":"vine"},
            "decoy":{"textures":"decoy"}
        }"#,
        r#"{"texture_data":{
            "vine":{"textures":"textures/blocks/vine"},
            "decoy":{"textures":"textures/blocks/decoy"}
        }}"#,
        "[]",
    );
    let vine_pixels = oriented_vine_pixels();
    write_png(
        directory.path(),
        "textures/blocks/vine",
        TILE_SIZE,
        TILE_SIZE,
        &vine_pixels,
    );
    write_png(
        directory.path(),
        "textures/blocks/decoy",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [211, 3, 149, 255]),
    );

    let compiled = compile_pack(directory.path(), &generated_vine_records())
        .expect("compile every protocol-1001 vine mask");
    assert_eq!(compiled.visuals.len(), 16);
    assert_eq!(
        compiled.model_templates.len(),
        16,
        "one compact template per mask"
    );
    assert_eq!(
        compiled.model_quads.len(),
        32,
        "four bits each occur in eight masks"
    );

    let expected_planes = [
        (
            1_u32,
            6_u32,
            0_usize,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (
            2_u32,
            3_u32,
            2_usize,
            [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]],
        ),
        (
            4_u32,
            5_u32,
            0_usize,
            [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]],
        ),
        (
            8_u32,
            4_u32,
            2_usize,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    let expected_rgba = vine_pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect::<Vec<_>>();

    for (mask, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "mask {mask} diagnostic fallback"
        );
        assert_ne!(
            visual.model_template,
            assets::NO_MODEL_TEMPLATE,
            "mask {mask}"
        );
        assert!(
            !visual.flags.intersects(
                BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
            ),
            "mask {mask}: fake full-block semantics"
        );
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, 0, "mask {mask}");
        assert_eq!(
            template.quad_count,
            (mask as u32).count_ones(),
            "mask {mask}"
        );
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let expected = expected_planes
            .iter()
            .filter(|(bit, _, _, _)| mask as u32 & bit != 0)
            .collect::<Vec<_>>();
        assert_eq!(quads.len(), expected.len(), "mask {mask}");
        for (quad, (bit, face, tangent_axis, positions)) in quads.iter().zip(expected) {
            assert_eq!(quad.positions, *positions, "mask {mask} bit {bit}");
            for (position, uv) in quad.positions.iter().zip(quad.uvs) {
                assert_eq!(
                    uv,
                    [
                        position[*tangent_axis] as u16 * 16,
                        (256 - position[1] as u16) * 16,
                    ],
                    "mask {mask} bit {bit}: UV must preserve the asymmetric texture's horizontal and vertical axes"
                );
            }
            assert_eq!(
                quad.flags,
                MODEL_QUAD_FLAG_TWO_SIDED | face,
                "mask {mask} bit {bit}: attachment planes must be two-sided and never support-culled"
            );
            assert_eq!(
                quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK,
                0,
                "mask {mask} bit {bit}"
            );
            let material = compiled.materials[quad.material as usize];
            assert_eq!(
                material.flags,
                MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT,
                "mask {mask} bit {bit}"
            );
            assert_eq!(
                mip_layer(&compiled, 0, material.texture.layer()),
                expected_rgba,
                "mask {mask} bit {bit}: selected the wrong terrain layer"
            );
            assert!(
                quad.positions.iter().any(|position| position[1] == 0)
                    && quad.positions.iter().any(|position| position[1] == 256),
                "mask {mask} bit {bit}: unexpected horizontal/top attachment plane"
            );
        }
    }

    let bytes = encode_blob(&compiled).expect("encode all vine templates, including mask zero");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode all vine templates");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_vine_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let vines = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Vine)
        .collect::<Vec<_>>();
    assert_eq!(vines.len(), 16, "protocol-1001 vine state count");
    for record in vines {
        let mask = record
            .model_state
            .get(ModelStateField::Connections)
            .expect("vine direction bits");
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "mask {mask}");
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, mask.count_ones(), "mask {mask}");
        for quad in &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
        {
            assert_eq!(
                compiled.materials[quad.material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT,
                "mask {mask}"
            );
        }
    }
}

fn write_flowerbed_pack(root: &Path, include_stem: bool) {
    write_pack(
        root,
        r#"{
            "wildflowers":{"textures":"wildflowers"},
            "pink_petals":{"textures":"pink_petals"}
        }"#,
        if include_stem {
            r#"{"texture_data":{
                "wildflowers":{"textures":[
                    "textures/blocks/wildflowers",
                    "textures/blocks/wildflowers_stem"
                ]},
                "pink_petals":{"textures":[
                    "textures/blocks/pink_petals",
                    "textures/blocks/pink_petals_stem"
                ]}
            }}"#
        } else {
            r#"{"texture_data":{
                "wildflowers":{"textures":["textures/blocks/wildflowers"]},
                "pink_petals":{"textures":["textures/blocks/pink_petals"]}
            }}"#
        },
        "[]",
    );
    for (index, path) in [
        "wildflowers",
        "wildflowers_stem",
        "pink_petals",
        "pink_petals_stem",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 41, 79, 0]),
        );
    }
}

fn flowerbed_geometry_digest(quads: &[assets::ModelQuad]) -> String {
    let flower_material = quads
        .first()
        .expect("flowerbed template has quads")
        .material;
    let mut digest = Sha256::new();
    for quad in quads {
        for position in quad.positions {
            for coordinate in position {
                digest.update(coordinate.to_le_bytes());
            }
        }
        for uv in quad.uvs {
            for coordinate in uv {
                digest.update(coordinate.to_le_bytes());
            }
        }
        digest.update([u8::from(quad.material != flower_material)]);
        digest.update(quad.flags.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[test]
fn compiler_flowerbed_positions_and_uvs_match_pinned_layout_hashes() {
    let directory = tempfile::tempdir().expect("create flowerbed digest fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .flat_map(|growth| {
            (0..4).map(move |orientation| {
                let sequential_id = growth * 4 + orientation;
                generated_flowerbed_record(
                    sequential_id,
                    9_000 + sequential_id,
                    "minecraft:wildflowers",
                    growth,
                    orientation,
                )
            })
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbed digests");
    let actual = compiled
        .visuals
        .iter()
        .map(|visual| {
            let template = compiled.model_templates[visual.model_template as usize];
            flowerbed_geometry_digest(
                &compiled.model_quads[template.quad_start as usize
                    ..(template.quad_start + template.quad_count) as usize],
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        [
            "0535cc209cf5d041dac03f4b705b506e4dcbcf78631b3c19f08c29529c0372e1",
            "fe9ae6e63ab41e8a54a1d478d493ac7f27b6a65e6342e2e9897a0b2481277c5c",
            "a7b1c4a16e06435a244f86d6f363592604ac1e3004f159df82c8126eaab60b69",
            "137445d77e0b871726f1715da34ea13afc85f8e368b3eabe47aa73c226d139ff",
        ],
        [
            "7ab55b6772d41dda2a461a3a6283e15f80d2f37624b259be8f01e806764d592d",
            "4d7a49f3e00ddb42a2e4d1457c90d97c1cdc9530315fa4f84c7b4eb375470b03",
            "02732e5274ee362636178813a3757a7ecb172b64a4bc53a04543ade5dd825984",
            "17fa180ccb7197b23a4812f4aa9bf5836ff1f216c90b543e61e10b280bd31e8c",
        ],
        [
            "5ef044de509676b39536764fbe07a8dcff229c395f4c9a1e359f252491f2c206",
            "722e9e3b0baa2de6565fdd5784e9cd88573bd8d211759492f3c285680864ea64",
            "6c69274f1235f83290629448199c67bd4d50768906815b3d191f8c928ecb85f6",
            "b2876159d61f4efcfc7050cbe3d68a381b7c4f6d3231e1dbe2ec9578680223ca",
        ],
        [
            "0ad8b575a87c6d1b1b6acb04b77cdb9c7db62321e38af3d157c2af8d84b6b134",
            "6e86adaf45e3916de0372636dcc6ebd1dc93b8c97675c466235766e8027b4950",
            "18a7cddfe2d57f62c2fdd29ed8a0edf883c13e7b9d79ce084093685c55e82574",
            "9c35fc675c95aeca270cb20b6b68eea6e1f366a6785003b3b0af3dbab92663d5",
        ],
    ];
    for layout in 0..4 {
        for orientation in 0..4 {
            assert_eq!(
                actual[layout * 4 + orientation],
                expected[layout][orientation],
                "layout={layout} orientation={orientation}"
            );
        }
    }
}

#[test]
fn compiler_compiles_normal_flowerbeds_as_additive_near_ground_two_material_models() {
    let directory = tempfile::tempdir().expect("create flowerbed fixture");
    write_flowerbed_pack(directory.path(), true);
    let mut records = Vec::new();
    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        for growth in 0..=7 {
            let sequential_id = records.len() as u32;
            records.push(generated_flowerbed_record(
                sequential_id,
                10_000 + sequential_id,
                name,
                growth,
                2,
            ));
        }
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbeds");
    for name_index in 0..2 {
        for (growth, expected_flower_quads) in [1, 2, 3, 4, 4, 4, 4, 4].into_iter().enumerate() {
            let visual = compiled.visuals[name_index * 8 + growth];
            assert_eq!(visual.kind, VisualKind::Model, "growth={growth}");
            assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
            let template = compiled.model_templates[visual.model_template as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let flower_material = quads[0].material;
            assert_eq!(
                quads
                    .iter()
                    .filter(|quad| quad.material == flower_material)
                    .count(),
                expected_flower_quads,
                "growth={growth} additive patch count"
            );
            assert!(
                quads
                    .iter()
                    .flat_map(|quad| quad.positions)
                    .all(|position| position[1] < 64),
                "growth={growth} exceeded near-ground bound"
            );
            assert_eq!(
                quads
                    .iter()
                    .map(|quad| quad.material)
                    .collect::<HashSet<_>>()
                    .len(),
                2,
                "growth={growth} material count"
            );
            assert!(
                quads
                    .iter()
                    .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
            );
        }
    }
}

#[test]
fn compiler_rotates_north_baseline_flowerbeds_by_pinned_cardinal_authority() {
    let directory = tempfile::tempdir().expect("create flowerbed rotation fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .map(|orientation| {
            generated_flowerbed_record(
                orientation,
                11_000 + orientation,
                "minecraft:wildflowers",
                0,
                orientation,
            )
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile rotated flowerbeds");
    // Pinned wildflowers.json at be56c809: north has no Y rotation;
    // east=90, south=180, west=270. BREG encodes S=0, W=1, N=2, E=3.
    let authority = [
        (0, "south", 180),
        (1, "west", 270),
        (2, "north", 0),
        (3, "east", 90),
    ];
    let expected_flower_positions = [
        [
            [256, 48, 256],
            [128, 48, 256],
            [128, 48, 128],
            [256, 48, 128],
        ],
        [[0, 48, 256], [0, 48, 128], [128, 48, 128], [128, 48, 256]],
        [[0, 48, 0], [128, 48, 0], [128, 48, 128], [0, 48, 128]],
        [[256, 48, 0], [256, 48, 128], [128, 48, 128], [128, 48, 0]],
    ];
    let expected_stem_positions = [
        [[179, 0, 237], [190, 0, 226], [190, 48, 226], [179, 48, 237]],
        [[19, 0, 179], [30, 0, 190], [30, 48, 190], [19, 48, 179]],
        [[77, 0, 19], [66, 0, 30], [66, 48, 30], [77, 48, 19]],
        [[237, 0, 77], [226, 0, 66], [226, 48, 66], [237, 48, 77]],
    ];
    for (orientation, direction, degrees) in authority {
        let visual = compiled.visuals[orientation];
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert_eq!(
            quads[0].positions, expected_flower_positions[orientation],
            "BREG {direction}={orientation} must apply Y={degrees}"
        );
        assert_eq!(
            quads[1].positions, expected_stem_positions[orientation],
            "BREG {direction}={orientation} stem must apply Y={degrees}"
        );
        assert_eq!(quads[0].uvs, [[0, 0], [2048, 0], [2048, 2048], [0, 2048]]);
        assert_eq!(
            quads[1].uvs,
            [[0, 1792], [256, 1792], [256, 1024], [0, 1024]]
        );
    }
}

#[test]
fn compiler_flowerbed_templates_are_bounded_deduplicated_and_blob_stable() {
    let directory = tempfile::tempdir().expect("create flowerbed matrix fixture");
    write_flowerbed_pack(directory.path(), true);
    let mut records = Vec::new();
    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        for growth in 0..8 {
            for orientation in 0..4 {
                let sequential_id = records.len() as u32;
                records.push(generated_flowerbed_record(
                    sequential_id,
                    12_000 + sequential_id,
                    name,
                    growth,
                    orientation,
                ));
            }
        }
    }
    let duplicate_id = records.len() as u32;
    records.push(generated_flowerbed_record(
        duplicate_id,
        12_000 + duplicate_id,
        "minecraft:wildflowers",
        2,
        2,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile flowerbed matrix");
    assert_eq!(compiled.materials.len(), 5, "diagnostic plus four textures");
    assert_eq!(compiled.model_templates.len(), 32);
    assert_eq!(compiled.model_quads.len(), 432);
    assert_eq!(
        compiled.visuals[duplicate_id as usize].model_template, compiled.visuals[10].model_template,
        "identical material/growth/orientation identity must deduplicate"
    );
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model),
        "all 64 normal flowerbed states must route to models"
    );
    for name_index in 0..2 {
        for orientation in 0..4 {
            let full_layout =
                compiled.visuals[name_index * 32 + 3 * 4 + orientation].model_template;
            for growth in 4..8 {
                assert_eq!(
                    compiled.visuals[name_index * 32 + growth * 4 + orientation].model_template,
                    full_layout,
                    "growth={growth} must alias the measured full layout for block={name_index} orientation={orientation}"
                );
            }
        }
    }
    for (index, expected_quads) in [7, 10, 17, 20].into_iter().enumerate() {
        let visual = compiled.visuals[index * 4];
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, expected_quads);
        assert!(template.quad_count <= 32);
    }

    let bytes = encode_blob(&compiled).expect("encode compiled flowerbed templates");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode compiled flowerbed templates");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_keeps_flowerbeds_diagnostic_without_exact_second_terrain_variant() {
    let directory = tempfile::tempdir().expect("create incomplete flowerbed fixture");
    write_flowerbed_pack(directory.path(), false);
    let records = [generated_flowerbed_record(
        0,
        13_000,
        "minecraft:pink_petals",
        3,
        0,
    )];

    let compiled = compile_pack(directory.path(), &records).expect("compile incomplete flowerbed");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(
        compiled.visuals[0].model_template,
        assets::NO_MODEL_TEMPLATE
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_flowerbed_exact_pair_does_not_require_command_only_records() {
    let directory = tempfile::tempdir().expect("create exact-pair flowerbed fixture");
    write_flowerbed_pack(directory.path(), true);
    let records = (0..4)
        .map(|growth| {
            generated_flowerbed_record(growth, 13_100 + growth, "minecraft:wildflowers", growth, 2)
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile exact-pair flowerbed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model)
    );
    assert_eq!(compiled.model_templates.len(), 4);
}

#[test]
fn compiler_keeps_flowerbeds_diagnostic_for_an_overlong_terrain_variant_array() {
    let directory = tempfile::tempdir().expect("create malformed flowerbed fixture");
    write_pack(
        directory.path(),
        r#"{"pink_petals":{"textures":"pink_petals"}}"#,
        r#"{"texture_data":{
            "pink_petals":{"textures":[
                "textures/blocks/pink_petals",
                "textures/blocks/pink_petals_stem",
                "textures/blocks/pink_petals_unexpected"
            ]}
        }}"#,
        "[]",
    );
    for (index, path) in ["pink_petals", "pink_petals_stem", "pink_petals_unexpected"]
        .into_iter()
        .enumerate()
    {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 43, 83, 0]),
        );
    }
    let records = (0..4)
        .map(|growth| {
            generated_flowerbed_record(growth, 14_000 + growth, "minecraft:pink_petals", growth, 0)
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile malformed flowerbed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

fn generated_slab_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    half: u32,
) -> RegistryRecord {
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry");
    let mut record = records
        .into_iter()
        .find(|record| {
            record.model_family == ModelFamily::Slab
                && record.model_state.get(ModelStateField::Half) == Some(half)
        })
        .unwrap_or_else(|| panic!("missing generated slab half={half}"));
    record.sequential_id = sequential_id;
    record.network_hash = network_hash;
    record.name = name.into();
    record.canonical_state = "{}".into();
    record
}

fn slab_record_with_replaced_half(mut record: RegistryRecord, half: u32) -> RegistryRecord {
    record.collision_seed = CollisionSeed::default();
    record.provenance = RegistryProvenance::DRAGONFLY;
    let mut bytes = registry_bytes(std::slice::from_ref(&record));
    const REGISTRY_HEADER_BYTES: usize = 8 + 7 * 4;
    const RECORD_FIXED_PREFIX_BYTES: usize = 24;
    const HALF_VALUE_OFFSET: usize = REGISTRY_HEADER_BYTES + RECORD_FIXED_PREFIX_BYTES + 4;
    assert_ne!(bytes[REGISTRY_HEADER_BYTES + 11] & (1 << 1), 0);
    bytes[HALF_VALUE_OFFSET..HALF_VALUE_OFFSET + 4].copy_from_slice(&half.to_le_bytes());
    read_registry(&bytes)
        .expect("decode half-mutated slab fixture")
        .into_iter()
        .next()
        .expect("one half-mutated slab fixture")
}

fn write_slab_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "test_slab":{"textures":{"down":"slab_down","side":"slab_side","up":"slab_up"}},
            "test_double_slab":{"textures":{"down":"slab_down","side":"slab_side","up":"slab_up"}}
        }"#,
        r#"{"texture_data":{
            "slab_down":{"textures":"textures/blocks/slab_down"},
            "slab_side":{"textures":"textures/blocks/slab_side"},
            "slab_up":{"textures":"textures/blocks/slab_up"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("slab_down", [21, 41, 61, 255]),
        ("slab_side", [81, 101, 121, 255]),
        ("slab_up", [141, 161, 181, 255]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn expected_slab_quads(materials: [u32; 6], min_y: i16, max_y: i16) -> [ModelQuad; 6] {
    let min_v = u16::try_from(4096 - i32::from(min_y) * 16).expect("bounded slab min V");
    let max_v = u16::try_from(4096 - i32::from(max_y) * 16).expect("bounded slab max V");
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

fn slab_geometry_digest(quads: &[ModelQuad]) -> String {
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

fn compiled_model_quads(compiled: &CompiledAssets, sequential_id: usize) -> &[ModelQuad] {
    let visual = compiled.visuals[sequential_id];
    assert_eq!(visual.kind, VisualKind::Model);
    let template = compiled.model_templates[visual.model_template as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

fn compiled_compound_model_quads(
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

#[test]
fn compiler_slab_templates_match_exact_exterior_positions_uvs_materials_and_flags() {
    let directory = tempfile::tempdir().expect("create slab geometry fixture");
    write_slab_pack(directory.path());
    let records = [
        generated_slab_record(0, 20_000, "minecraft:test_slab", 0),
        generated_slab_record(1, 20_001, "minecraft:test_slab", 1),
        generated_slab_record(2, 20_002, "minecraft:test_double_slab", 2),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile exact slab geometry");
    assert_eq!(compiled.materials.len(), 4, "diagnostic plus down/side/up");
    let expected_digests = [
        "3b7f0f1e69d4254dee7b6454a76e3aac55c91208b014c174e6627a5980ff2d57",
        "3e687b84eebc0b0c72d2454918f0112aa04301702570abe9d59cdd6e2be84c21",
        "f50037c8ed2c82dad3727accf4be0b17de464f3432810573955bdd81b0b6837c",
    ];
    for (id, bounds) in [(0, (0, 128)), (1, (128, 256)), (2, (0, 256))] {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "slab id={id}");
        assert!(
            !visual
                .flags
                .intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL)
        );
        assert_eq!(
            visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE),
            id == 2,
            "only the full slab is a full-face occluder"
        );
        let actual = compiled_model_quads(&compiled, id);
        let expected = expected_slab_quads(visual.faces, bounds.0, bounds.1);
        assert_eq!(actual, expected, "slab id={id} exact quad contract");
        assert_eq!(actual.len(), 6);
        assert_eq!(
            actual.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
            match id {
                0 => vec![0x33, 0x44, 0x11, 0x02, 0x55, 0x66],
                1 => vec![0x33, 0x44, 0x01, 0x22, 0x55, 0x66],
                2 => vec![0x33, 0x44, 0x11, 0x22, 0x55, 0x66],
                _ => unreachable!(),
            },
            "only block-boundary faces carry cull-face flags"
        );
        assert_eq!(
            actual[0].uvs,
            match id {
                0 => [[0, 4096], [4096, 4096], [4096, 2048], [0, 2048]],
                1 => [[0, 2048], [4096, 2048], [4096, 0], [0, 0]],
                2 => [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                _ => unreachable!(),
            },
            "west side uses the vanilla lower/upper/full vertical crop"
        );
        assert_eq!(
            actual[1].uvs,
            match id {
                0 => [[0, 4096], [0, 2048], [4096, 2048], [4096, 4096]],
                1 => [[0, 2048], [0, 0], [4096, 0], [4096, 2048]],
                2 => [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
                _ => unreachable!(),
            },
            "east side preserves the transposed cube-face orientation"
        );
        assert_eq!(actual[2].uvs, [[0, 0], [4096, 0], [4096, 4096], [0, 4096]]);
        assert_eq!(actual[3].uvs, [[0, 0], [0, 4096], [4096, 4096], [4096, 0]]);
        assert_eq!(
            slab_geometry_digest(actual),
            expected_digests[id],
            "slab id={id} position/UV/flag digest"
        );
        assert_eq!(slab_geometry_digest(&expected), expected_digests[id]);
        for (face, quad) in actual.iter().enumerate() {
            assert_eq!(quad.material, visual.faces[face]);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert!((1..=6).contains(&(quad.flags & MODEL_QUAD_FLAG_FACE_MASK)));
            assert_eq!(
                quad.flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK),
                0
            );
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
        }
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::East as usize]
        );
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::North as usize]
        );
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::South as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Down as usize],
            visual.faces[BlockFace::Up as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Down as usize],
            visual.faces[BlockFace::West as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Up as usize],
            visual.faces[BlockFace::West as usize]
        );
    }
}

#[test]
fn compiler_slab_half_is_typed_fail_closed_and_ignores_collision_only_boxes() {
    let directory = tempfile::tempdir().expect("create slab half fixture");
    write_slab_pack(directory.path());
    let baseline = generated_slab_record(0, 21_000, "minecraft:test_slab", 0);
    let mut collision_only = generated_slab_record(1, 21_001, "minecraft:test_slab", 0);
    collision_only.collision_seed = CollisionSeed {
        shape_id: 65_000,
        confidence: CollisionConfidence::CollisionOnly,
        boxes: vec![CollisionBox {
            min_x: 25_000_000,
            min_y: 25_000_000,
            min_z: 25_000_000,
            max_x: 75_000_000,
            max_y: 75_000_000,
            max_z: 75_000_000,
        }]
        .into_boxed_slice(),
    };
    let missing = model_record(
        2,
        21_002,
        "minecraft:test_slab",
        r#"{"vertical_half":{"type":"int","value":0}}"#,
        ModelFamily::Slab,
    );
    let malformed = slab_record_with_replaced_half(
        generated_slab_record(3, 21_003, "minecraft:test_slab", 0),
        3,
    );

    let compiled = compile_pack(
        directory.path(),
        &[baseline, collision_only, missing, malformed],
    )
    .expect("compile fail-closed slab half fixture");
    assert_eq!(
        compiled.visuals[0].model_template, compiled.visuals[1].model_template,
        "collision-only boxes must not select render geometry"
    );
    for id in [2, 3] {
        assert_eq!(compiled.visuals[id].kind, VisualKind::Diagnostic, "id={id}");
        assert_eq!(
            compiled.visuals[id].model_template,
            assets::NO_MODEL_TEMPLATE
        );
    }
}

#[test]
fn compiler_covers_all_272_breg_slab_states_with_three_deduplicated_stable_templates() {
    let directory = tempfile::tempdir().expect("create exhaustive slab fixture");
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Slab)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 272);
    let mut half_counts = [0_usize; 3];
    let mut blocks = serde_json::Map::new();
    for record in &records {
        let half = record
            .model_state
            .get(ModelStateField::Half)
            .expect("every generated slab has typed half");
        half_counts[half as usize] += 1;
        blocks.insert(
            record
                .name
                .strip_prefix("minecraft:")
                .unwrap_or(&record.name)
                .to_owned(),
            serde_json::json!({"textures":"slab_all"}),
        );
    }
    assert_eq!(half_counts, [68, 68, 136]);
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        r#"{"texture_data":{"slab_all":{"textures":"textures/blocks/slab_all"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/slab_all",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [73, 109, 151, 255]),
    );

    let compiled = compile_pack(directory.path(), &records).expect("compile all BREG slabs");
    assert_eq!(compiled.model_templates.len(), 3);
    assert_eq!(compiled.model_quads.len(), 18);
    let mut template_by_half = [HashSet::new(), HashSet::new(), HashSet::new()];
    for record in &records {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        assert!(
            visual
                .faces
                .into_iter()
                .all(|material| material != DIAGNOSTIC_MATERIAL),
            "{} retained a diagnostic face",
            record.name
        );
        let half = record.model_state.get(ModelStateField::Half).unwrap() as usize;
        template_by_half[half].insert(visual.model_template);
    }
    assert!(
        template_by_half
            .iter()
            .all(|templates| templates.len() == 1)
    );
    assert_eq!(template_by_half[2].len(), 1, "all double slabs deduplicate");

    let baseline = encode_blob(&compiled).expect("encode exhaustive slab baseline");
    let reversed = records.iter().cloned().rev().collect::<Vec<_>>();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed BREG slabs");
    assert_eq!(
        encode_blob(&reversed).expect("encode reversed slabs"),
        baseline
    );
    let runtime = RuntimeAssets::decode(&baseline).expect("decode exhaustive slab blob");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_slab_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let slabs = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Slab)
        .collect::<Vec<_>>();
    assert_eq!(slabs.len(), 272);
    let diagnostic = slabs
        .iter()
        .filter(|record| {
            compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
        })
        .map(|record| (record.name.as_ref(), record.canonical_state.as_ref()))
        .collect::<Vec<_>>();
    assert!(
        diagnostic.is_empty(),
        "pinned pack retained diagnostic slabs: {diagnostic:?}"
    );
    assert_eq!(
        slabs
            .iter()
            .map(|record| compiled.visuals[record.sequential_id as usize].model_template)
            .collect::<HashSet<_>>()
            .len(),
        189,
        "pinned slab material/half templates"
    );
}

fn write_stair_pack(root: &Path) {
    write_pack(
        root,
        r#"{"oak_stairs":{"textures":{"down":"stair_down","side":"stair_side","up":"stair_up"}}}"#,
        r#"{"texture_data":{"stair_down":{"textures":"textures/blocks/stair_down"},"stair_side":{"textures":"textures/blocks/stair_side"},"stair_up":{"textures":"textures/blocks/stair_up"}}}"#,
        "[]",
    );
    for (path, colour) in [
        ("stair_down", [17, 37, 57, 255]),
        ("stair_side", [77, 97, 117, 255]),
        ("stair_up", [137, 157, 177, 255]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn write_asymmetric_stair_pack(root: &Path) {
    write_pack(
        root,
        r#"{"oak_stairs":{"textures":{"west":"stair_west","east":"stair_east","down":"stair_down","up":"stair_up","north":"stair_north","south":"stair_south"}}}"#,
        r#"{"texture_data":{"stair_west":{"textures":"textures/blocks/stair_west"},"stair_east":{"textures":"textures/blocks/stair_east"},"stair_down":{"textures":"textures/blocks/stair_down"},"stair_up":{"textures":"textures/blocks/stair_up"},"stair_north":{"textures":"textures/blocks/stair_north"},"stair_south":{"textures":"textures/blocks/stair_south"}}}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        let mut uv_marker = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                uv_marker.push([17 + index as u8 * 31, x as u8 * 16, y as u8 * 16, 255]);
            }
        }
        write_png(
            root,
            &format!("textures/blocks/stair_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &uv_marker,
        );
    }
}

fn rotate_stair_position([x, y, z]: [i16; 3], rotation: u32) -> [i16; 3] {
    match rotation & 3 {
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => [x, y, z],
    }
}

fn rotate_stair_face(face: usize, rotation: u32) -> usize {
    let horizontal = match rotation & 3 {
        1 => [4, 5, 2, 3, 1, 0],
        2 => [1, 0, 2, 3, 5, 4],
        3 => [5, 4, 2, 3, 0, 1],
        _ => [0, 1, 2, 3, 4, 5],
    };
    horizontal[face]
}

#[test]
fn compiler_stair_rotation_preserves_asymmetric_materials_geometry_and_uv_lock_for_all_states() {
    let directory = tempfile::tempdir().expect("create asymmetric stair fixture");
    write_asymmetric_stair_pack(directory.path());
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed registry")
        .into_iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Half).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 31_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile asymmetric stairs");

    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        let rotation = visual.variant & 3;
        let half = record.model_state.get(ModelStateField::Half).unwrap();
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let north_id = records
            .iter()
            .position(|candidate| {
                candidate.model_state.get(ModelStateField::Half) == Some(half)
                    && candidate.model_state.get(ModelStateField::Orientation) == Some(2)
            })
            .unwrap();
        let north_visual = compiled.visuals[north_id];
        let north_template = compiled.model_templates[north_visual.model_template as usize];
        let north_quads = &compiled.model_quads[north_template.quad_start as usize
            ..(north_template.quad_start + north_template.quad_count) as usize];
        assert_eq!(quads.len(), north_quads.len());
        let mut high_top_centres = Vec::new();
        for (quad, north_quad) in quads.iter().zip(north_quads) {
            assert_eq!(
                quad.positions, north_quad.positions,
                "orientation must stay canonical"
            );
            assert_eq!(
                quad.uvs, north_quad.uvs,
                "orientation must preserve UV lock"
            );
            assert_eq!(
                quad.flags, north_quad.flags,
                "orientation changed canonical faces"
            );
            let canonical_face =
                [2, 3, 0, 1, 4, 5][((quad.flags & MODEL_QUAD_FLAG_FACE_MASK) - 1) as usize];
            let world_face = rotate_stair_face(canonical_face, rotation);
            assert_eq!(
                quad.material, visual.faces[world_face],
                "half={half} orientation={orientation} canonical_face={canonical_face}"
            );
            let world_positions = quad
                .positions
                .map(|position| rotate_stair_position(position, rotation));
            assert!(
                world_positions
                    .iter()
                    .flatten()
                    .all(|&coordinate| (0..=256).contains(&coordinate))
            );
            assert!(
                quad.uvs
                    .iter()
                    .flatten()
                    .all(|&coordinate| coordinate <= 4096)
            );
            let step_outer_face = if half == 0 {
                BlockFace::Up as usize
            } else {
                BlockFace::Down as usize
            };
            let step_outer_y = if half == 0 { 256 } else { 0 };
            if world_face == step_outer_face
                && world_positions
                    .iter()
                    .all(|position| position[1] == step_outer_y)
            {
                high_top_centres.push([
                    world_positions
                        .iter()
                        .map(|position| i32::from(position[0]))
                        .sum::<i32>()
                        / 4,
                    world_positions
                        .iter()
                        .map(|position| i32::from(position[2]))
                        .sum::<i32>()
                        / 4,
                ]);
            }
        }
        assert!(
            !high_top_centres.is_empty(),
            "half={half} orientation={orientation}"
        );
        assert!(
            high_top_centres.iter().any(|&[x, z]| match orientation {
                0 => z > 128,
                1 => x < 128,
                2 => z < 128,
                3 => x > 128,
                _ => false,
            }),
            "high step lost world-space handedness: half={half} orientation={orientation} centres={high_top_centres:?}"
        );
    }
}

#[test]
fn compiler_stairs_emit_five_contiguous_bounded_exterior_templates_for_every_state() {
    let directory = tempfile::tempdir().expect("create stair fixture");
    write_stair_pack(directory.path());
    let mut records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed registry")
        .into_iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 8);
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Half).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 30_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile all oak stair states");
    assert_eq!(
        compiled.model_templates.len(),
        10,
        "five shapes per upside state; orientation is compact"
    );
    let mut bases_by_half = [HashSet::new(), HashSet::new()];
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(
            !visual
                .flags
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        assert_eq!(
            visual.variant & 3,
            (record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap()
                + 2)
                & 3
        );
        assert_eq!(
            (visual.variant >> 2) & 1,
            record.model_state.get(ModelStateField::Half).unwrap()
        );
        let base = visual.model_template as usize;
        assert_eq!(base % 5, 0);
        bases_by_half[record.model_state.get(ModelStateField::Half).unwrap() as usize].insert(base);
        for template in &compiled.model_templates[base..base + 5] {
            assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_STAIR);
            assert!((1..=32).contains(&template.quad_count));
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert!(quads.iter().all(|quad| {
                let [a, b, c, _] = quad.positions;
                a != b
                    && b != c
                    && a != c
                    && quad.uvs.iter().all(|uv| uv[0] <= 4096 && uv[1] <= 4096)
                    && quad.flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK)
                        == 0
            }));
        }
    }
    assert_eq!(bases_by_half[0].len(), 1);
    assert_eq!(bases_by_half[1].len(), 1);
    let north_lower = compiled.visuals[records
        .iter()
        .position(|record| {
            record.model_state.get(ModelStateField::Orientation) == Some(2)
                && record.model_state.get(ModelStateField::Half) == Some(0)
        })
        .unwrap()]
    .model_template as usize;
    let straight = compiled.model_templates[north_lower];
    let expected_shape_digests = [
        "2e07913dd24532f98c2e2a2352f4434cee0485f3b80a1b36346543b8f41fb381",
        "65128da5f92158b78301af0bb455f5d5a9a74fc0434e50553787d31c64ac88da",
        "17ed41557ef2ecfd36c077b347aafea71e8deca603f4155fe1001b2992b0deb2",
        "a8362bb0405925933f2a24acf62338455c1f07390d136446f2e6cf34dd2166b5",
        "3836909fd60a7bedb8e51c4b8358a42fdb5e23ad580a53536b3c81492423720b",
        "d18605f16826c3570a3691c95793e25d4be00702d6ae7221ffaa75d75e1efee6",
        "28c0a4d6a13b85633117437f6bb6b8263e7cd03b05ab890b69e294b84d11f990",
        "02a8b452d0d4f1de93c604bca2175dbfac432380ce2c54036b79e6073403eb7f",
        "f06c28159d93b5528ae9aff35580d1bae8676a2e0c72c7264c1b1b3fe046691f",
        "f1f5f7b2c7527ca6105c875dc4e04f0d4239e46e220325147dc27e965c267760",
    ];
    let mut actual_shape_digests = Vec::new();
    for base in [
        *bases_by_half[0].iter().next().unwrap(),
        *bases_by_half[1].iter().next().unwrap(),
    ] {
        for template in &compiled.model_templates[base..base + 5] {
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            actual_shape_digests.push(slab_geometry_digest(quads));
        }
    }
    assert_eq!(actual_shape_digests, expected_shape_digests);
    let straight_quads = &compiled.model_quads
        [straight.quad_start as usize..(straight.quad_start + straight.quad_count) as usize];
    assert!(
        straight_quads
            .iter()
            .any(|quad| quad.positions.iter().all(|p| p[2] == 128)
                && quad.positions.iter().any(|p| p[1] == 128)
                && quad.positions.iter().any(|p| p[1] == 256)),
        "north stair riser"
    );
    assert!(
        straight_quads
            .iter()
            .all(|quad| quad.positions.windows(2).all(|pair| pair[0] != pair[1])),
        "no flat edges"
    );

    let first = encode_blob(&compiled).expect("encode stairs");
    records.reverse();
    let second =
        encode_blob(&compile_pack(directory.path(), &records).expect("compile reversed stairs"))
            .expect("encode reversed stairs");
    assert_eq!(
        first, second,
        "stair blob is deterministic across input order"
    );
    RuntimeAssets::decode(&first).expect("runtime accepts canonical stair groups");
}

#[test]
fn compiler_covers_every_breg_stair_state_with_compact_stable_groups() {
    let directory = tempfile::tempdir().expect("create exhaustive stair fixture");
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed registry")
        .into_iter()
        .filter(|record| record.model_family == ModelFamily::Stair)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 512);
    assert!(records.iter().all(|record| {
        record
            .model_state
            .get(ModelStateField::Orientation)
            .is_some_and(|value| value < 4)
            && record
                .model_state
                .get(ModelStateField::Half)
                .is_some_and(|value| value < 2)
    }));
    let mut names = records
        .iter()
        .map(|record| record.name.strip_prefix("minecraft:").unwrap().to_owned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names.len(), 64);
    for name in &names {
        let selectors = records
            .iter()
            .filter(|record| record.name.strip_prefix("minecraft:") == Some(name.as_str()))
            .map(|record| {
                (
                    record.model_state.get(ModelStateField::Orientation),
                    record.model_state.get(ModelStateField::Half),
                )
            })
            .collect::<HashSet<_>>();
        let expected = (0..4)
            .flat_map(|orientation| (0..2).map(move |half| (Some(orientation), Some(half))))
            .collect::<HashSet<_>>();
        assert_eq!(selectors, expected, "{name} exact stair selector matrix");
    }
    let blocks = names
        .iter()
        .map(|name| format!(r#""{name}":{{"textures":"stair_all"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    write_pack(
        directory.path(),
        &format!("{{{blocks}}}"),
        r#"{"texture_data":{"stair_all":{"textures":"textures/blocks/stair_all"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stair_all",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [91, 111, 131, 255]),
    );
    let compiled = compile_pack(directory.path(), &records).expect("compile every BREG stair");
    assert!(records.iter().all(|record| {
        let visual = compiled.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Model
            && visual.model_template != assets::NO_MODEL_TEMPLATE
            && compiled.model_templates[visual.model_template as usize].flags
                == MODEL_TEMPLATE_FLAG_STAIR
    }));
    assert_eq!(
        compiled.model_templates.len(),
        10,
        "one symmetric-material group per half"
    );
    let first = encode_blob(&compiled).expect("encode exhaustive stairs");
    let mut reversed = records.clone();
    reversed.reverse();
    let second = encode_blob(
        &compile_pack(directory.path(), &reversed).expect("compile reversed exhaustive stairs"),
    )
    .expect("encode reversed exhaustive stairs");
    assert_eq!(first, second);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_stair_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!("../data/block-registry-v1001.bin"))
        .expect("decode committed generated registry");
    assert_eq!(records.len(), 16_913);
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let stairs = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Stair)
        .collect::<Vec<_>>();
    assert_eq!(stairs.len(), 512);
    assert_eq!(
        stairs
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        64
    );
    for record in stairs {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let template = compiled
            .model_templates
            .get(visual.model_template as usize)
            .unwrap_or_else(|| {
                panic!(
                    "missing stair template for {} {}",
                    record.name, record.canonical_state
                )
            });
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_STAIR);
        assert!((1..=32).contains(&template.quad_count));
    }
}

#[test]
fn compiler_compiles_exact_terrestrial_cross_alias_tint_and_crop_variants() {
    let directory = tempfile::tempdir().expect("create terrestrial cross fixture");
    write_pack(
        directory.path(),
        r#"{
            "short_grass":{"textures":"short_grass"},
            "fern":{"textures":"fern"},
            "yellow_flower":{"textures":"yellow_flower"},
            "sapling":{"textures":"sapling"},
            "wheat":{"textures":"wheat"},
            "carrots":{"textures":"carrots"},
            "melon_stem":{"textures":"melon_stem"}
        }"#,
        r#"{"texture_data":{
            "short_grass":{"textures":"textures/blocks/short_grass"},
            "fern":{"textures":"textures/blocks/fern"},
            "yellow_flower":{"textures":"textures/blocks/dandelion"},
            "sapling":{"textures":["textures/blocks/oak","textures/blocks/spruce"]},
            "wheat":{"textures":[
                "textures/blocks/wheat0","textures/blocks/wheat1",
                "textures/blocks/wheat2","textures/blocks/wheat3",
                "textures/blocks/wheat4","textures/blocks/wheat5",
                "textures/blocks/wheat6","textures/blocks/wheat7"
            ]},
            "carrots":{"textures":[
                "textures/blocks/carrots0","textures/blocks/carrots1",
                "textures/blocks/carrots2","textures/blocks/carrots3"
            ]},
            "melon_stem":{"textures":[
                "textures/blocks/melon_disconnected","textures/blocks/melon_connected"
            ]}
        }}"#,
        "[]",
    );
    for (index, path) in [
        "short_grass",
        "fern",
        "dandelion",
        "oak",
        "spruce",
        "wheat0",
        "wheat1",
        "wheat2",
        "wheat3",
        "wheat4",
        "wheat5",
        "wheat6",
        "wheat7",
        "carrots0",
        "carrots1",
        "carrots2",
        "carrots3",
        "melon_disconnected",
        "melon_connected",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 17, 31, 255]),
        );
    }
    let records = [
        model_record(0, 100, "minecraft:short_grass", "{}", ModelFamily::Cross),
        model_record(1, 101, "minecraft:fern", "{}", ModelFamily::Cross),
        model_record(2, 102, "minecraft:dandelion", "{}", ModelFamily::Cross),
        model_record(
            3,
            103,
            "minecraft:oak_sapling",
            r#"{"age_bit":{"type":"byte","value":1}}"#,
            ModelFamily::Cross,
        ),
        model_record(
            4,
            104,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":0}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            5,
            105,
            "minecraft:wheat",
            r#"{"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            6,
            106,
            "minecraft:carrots",
            r#"{"growth":{"type":"int","value":5}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            7,
            107,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":0},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
        model_record(
            8,
            108,
            "minecraft:melon_stem",
            r#"{"facing_direction":{"type":"int","value":2},"growth":{"type":"int","value":7}}"#,
            ModelFamily::Crop,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile crossed plants");
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Cross && visual.model_template != assets::NO_MODEL_TEMPLATE
    }));
    assert_eq!(
        compiled
            .visuals
            .iter()
            .map(|visual| visual.variant)
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 7, 2, 0, 1]
    );
    for (index, visual) in compiled.visuals.iter().enumerate() {
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(
            template.quad_count, 2,
            "visual {index} did not use one crossed pair"
        );
        assert!(template.quad_count <= 32);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert!(
            quads
                .iter()
                .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
        );
        assert!(quads.iter().all(|quad| {
            compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
        }));
    }
    for index in [0_usize, 1] {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            MATERIAL_FLAG_GRASS_TINT,
            "grass and fern must use the biome grass tint class"
        );
    }
    for index in 2..compiled.visuals.len() {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        let material = compiled.model_quads[template.quad_start as usize].material as usize;
        assert_eq!(
            compiled.materials[material].flags & MATERIAL_FLAG_TINT_MASK,
            0
        );
    }
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[5].model_template
    );
    assert_ne!(
        compiled.visuals[4].model_template,
        compiled.visuals[6].model_template
    );
}

#[test]
fn compiler_compiles_exact_animated_seagrass_pairs_without_biome_tint() {
    let directory = tempfile::tempdir().expect("create seagrass fixture");
    write_pack(
        directory.path(),
        r#"{
            "seagrass":{"textures":{
                "up":"seagrass_short",
                "down":"seagrass_tall_bot_a",
                "south":"seagrass_tall_bot_b",
                "east":"seagrass_tall_top_a",
                "west":"seagrass_tall_top_b"
            }}
        }"#,
        r#"{"texture_data":{
            "seagrass_short":{"textures":"textures/blocks/seagrass"},
            "seagrass_tall_bot_a":{"textures":"textures/blocks/seagrass_bottom_a"},
            "seagrass_tall_bot_b":{"textures":"textures/blocks/seagrass_bottom_b"},
            "seagrass_tall_top_a":{"textures":"textures/blocks/seagrass_top_a"},
            "seagrass_tall_top_b":{"textures":"textures/blocks/seagrass_top_b"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/seagrass","atlas_tile":"seagrass_short","ticks_per_frame":4},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_a","atlas_tile":"seagrass_tall_bot_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_bottom_b","atlas_tile":"seagrass_tall_bot_b","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_a","atlas_tile":"seagrass_tall_top_a","ticks_per_frame":3},
            {"flipbook_texture":"textures/blocks/seagrass_top_b","atlas_tile":"seagrass_tall_top_b","ticks_per_frame":3}
        ]"#,
    );
    for (index, path) in [
        "seagrass",
        "seagrass_bottom_a",
        "seagrass_bottom_b",
        "seagrass_top_a",
        "seagrass_top_b",
    ]
    .into_iter()
    .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 40, 80, 255]);
        strip.extend(solid(TILE_SIZE, TILE_SIZE, [index as u8 + 11, 50, 90, 255]));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }
    let records = [
        model_record(
            0,
            200,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"default"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            1,
            201,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_bot"}}"#,
            ModelFamily::Aquatic,
        ),
        model_record(
            2,
            202,
            "minecraft:seagrass",
            r#"{"sea_grass_type":{"type":"string","value":"double_top"}}"#,
            ModelFamily::Aquatic,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile animated seagrass");
    assert_eq!(compiled.visuals.len(), 3);
    let expected_ticks = [[4, 4], [3, 3], [3, 3]];
    for (index, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(visual.kind, VisualKind::Cross);
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.quad_count, 2);
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for (quad, ticks) in quads.iter().zip(expected_ticks[index]) {
            assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
            let material = compiled.materials[quad.material as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            assert_eq!(
                compiled.animations[material.animation as usize].ticks_per_frame,
                ticks
            );
        }
        assert!(visual.flags.is_empty());
    }
    let materials_for = |index: usize| {
        let template = compiled.model_templates[compiled.visuals[index].model_template as usize];
        compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
            .iter()
            .map(|quad| quad.material)
            .collect::<Vec<_>>()
    };
    let short = materials_for(0);
    assert_eq!(short[0], short[1]);
    assert_ne!(materials_for(1)[0], materials_for(1)[1]);
    assert_ne!(materials_for(2)[0], materials_for(2)[1]);
}

#[test]
fn compiler_compiles_all_kelp_ages_as_six_animated_body_and_head_faces() {
    let directory = tempfile::tempdir().expect("create kelp fixture");
    write_pack(
        directory.path(),
        r#"{"kelp":{"textures":{
            "down":"kelp_d","east":"kelp_top","north":"kelp_a",
            "south":"kelp_b","up":"kelp_c","west":"kelp_top_bulb"
        }}}"#,
        r#"{"texture_data":{
            "kelp_a":{"textures":"textures/blocks/kelp_a"},
            "kelp_b":{"textures":"textures/blocks/kelp_b"},
            "kelp_c":{"textures":"textures/blocks/kelp_c"},
            "kelp_d":{"textures":"textures/blocks/kelp_d"},
            "kelp_top":{"textures":"textures/blocks/kelp_top"},
            "kelp_top_bulb":{"textures":"textures/blocks/kelp_top_bulb"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/kelp_a","atlas_tile":"kelp_a","ticks_per_frame":4,"frames":[0,1,2,3,4,5]},
            {"flipbook_texture":"textures/blocks/kelp_b","atlas_tile":"kelp_b","ticks_per_frame":4,"frames":[1,2,3,4,5,0]},
            {"flipbook_texture":"textures/blocks/kelp_c","atlas_tile":"kelp_c","ticks_per_frame":4,"frames":[2,3,4,5,0,1]},
            {"flipbook_texture":"textures/blocks/kelp_d","atlas_tile":"kelp_d","ticks_per_frame":4,"frames":[3,4,5,0,1,2]},
            {"flipbook_texture":"textures/blocks/kelp_top","atlas_tile":"kelp_top","ticks_per_frame":4,"frames":[4,5,0,1,2,3]},
            {"flipbook_texture":"textures/blocks/kelp_top_bulb","atlas_tile":"kelp_top_bulb","ticks_per_frame":4,"frames":[5,0,1,2,3,4]}
        ]"#,
    );
    for (texture_index, path) in [
        "kelp_a",
        "kelp_b",
        "kelp_c",
        "kelp_d",
        "kelp_top",
        "kelp_top_bulb",
    ]
    .into_iter()
    .enumerate()
    {
        let strip = (0..6)
            .flat_map(|frame| {
                solid(
                    TILE_SIZE,
                    TILE_SIZE,
                    [texture_index as u8 + 1, frame as u8 + 20, 90, 255],
                )
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 6,
            &strip,
        );
    }
    let records = (0..26)
        .map(|age| {
            model_record(
                age,
                300 + age,
                "minecraft:kelp",
                &format!(r#"{{"kelp_age":{{"type":"int","value":{age}}}}}"#),
                ModelFamily::Aquatic,
            )
        })
        .collect::<Vec<_>>();

    let compiled = compile_pack(directory.path(), &records).expect("compile all kelp ages");
    assert_eq!(compiled.visuals.len(), 26);
    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Model
            && visual.model_template == compiled.visuals[0].model_template
            && visual.flags.is_empty()
    }));
    let template = compiled.model_templates[compiled.visuals[0].model_template as usize];
    assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_KELP);
    assert_eq!(template.quad_count, 6);
    let quads = &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
    assert_eq!(
        quads.iter().map(|quad| quad.material).collect::<Vec<_>>(),
        [
            compiled.visuals[0].faces[BlockFace::North as usize],
            compiled.visuals[0].faces[BlockFace::South as usize],
            compiled.visuals[0].faces[BlockFace::Up as usize],
            compiled.visuals[0].faces[BlockFace::Down as usize],
            compiled.visuals[0].faces[BlockFace::East as usize],
            compiled.visuals[0].faces[BlockFace::West as usize],
        ]
    );
    assert_eq!(
        quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
        [
            0,
            0,
            0,
            0,
            MODEL_QUAD_FLAG_TWO_SIDED,
            MODEL_QUAD_FLAG_TWO_SIDED
        ]
    );
    assert_eq!(
        quads[2].positions,
        [
            quads[0].positions[1],
            quads[0].positions[0],
            quads[0].positions[3],
            quads[0].positions[2]
        ]
    );
    assert_eq!(
        quads[3].positions,
        [
            quads[1].positions[1],
            quads[1].positions[0],
            quads[1].positions[3],
            quads[1].positions[2]
        ]
    );
    let normal = |quad: &assets::ModelQuad| {
        let a = [
            i64::from(quad.positions[1][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[1][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[1][2]) - i64::from(quad.positions[0][2]),
        ];
        let b = [
            i64::from(quad.positions[2][0]) - i64::from(quad.positions[0][0]),
            i64::from(quad.positions[2][1]) - i64::from(quad.positions[0][1]),
            i64::from(quad.positions[2][2]) - i64::from(quad.positions[0][2]),
        ];
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    for (forward, reverse) in [(0, 2), (1, 3)] {
        let forward = normal(&quads[forward]);
        let reverse = normal(&quads[reverse]);
        assert!(
            forward
                .into_iter()
                .zip(reverse)
                .map(|(left, right)| left * right)
                .sum::<i64>()
                < 0,
            "kelp body windings must face opposite directions"
        );
    }
    let animations = quads
        .iter()
        .map(|quad| compiled.materials[quad.material as usize])
        .map(|material| {
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
            assert_ne!(material.animation, assets::NO_ANIMATION);
            let animation = compiled.animations[material.animation as usize];
            assert_eq!(animation.ticks_per_frame, 4);
            compiled.animation_frames[animation.frame_start as usize
                ..(animation.frame_start + animation.frame_count) as usize]
                .to_vec()
        })
        .collect::<Vec<_>>();
    assert_eq!(animations.len(), 6);
    for left in 0..animations.len() {
        for right in left + 1..animations.len() {
            assert_ne!(animations[left], animations[right]);
        }
    }
}

fn material_for_face(compiled: &CompiledAssets, sequential_id: usize, face: BlockFace) -> Material {
    compiled.materials[compiled.visuals[sequential_id].faces[face as usize] as usize]
}

fn leaf_material_fixture() -> (TempDir, PathBuf, Vec<RegistryRecord>) {
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

fn biome_registry_bytes(id: u32, name: &str) -> Vec<u8> {
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

fn write_biome_fixture(resource_pack: &Path) {
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

fn registry_bytes(records: &[RegistryRecord]) -> Vec<u8> {
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

fn shuffled_records(records: &[RegistryRecord], mut state: u64) -> Vec<RegistryRecord> {
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

fn mip_layer(compiled: &CompiledAssets, mip_index: usize, layer: u32) -> &[u8] {
    let mip = &compiled.texture_pages[0].texture.mips[mip_index];
    let layer_bytes = usize::try_from(mip.size * mip.size * 4).expect("small mip");
    let start = usize::try_from(layer).expect("small layer") * layer_bytes;
    &mip.rgba8[start..start + layer_bytes]
}

fn alpha_survivors(rgba: &[u8]) -> usize {
    assert_eq!(rgba.len() % 4, 0);
    rgba.chunks_exact(4).filter(|pixel| pixel[3] >= 128).count()
}

fn scaled_survivors(raw_rgba: &[u8], scale: u32) -> usize {
    raw_rgba
        .chunks_exact(4)
        .filter(|pixel| {
            let alpha = ((u32::from(pixel[3]) * scale + 0x8000) >> 16).min(255) as u8;
            alpha >= 128
        })
        .count()
}

fn reference_nearest_scale(raw_rgba: &[u8], target: usize) -> u32 {
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

fn reference_nearest_survivors(raw_rgba: &[u8], target: usize) -> usize {
    scaled_survivors(raw_rgba, reference_nearest_scale(raw_rgba, target))
}

fn cutout_pattern(colour: [u8; 3], threshold: u32) -> Vec<[u8; 4]> {
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

fn aligned_half_pattern(colour: [u8; 3]) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for _y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            pixels.push([colour[0], colour[1], colour[2], u8::MAX * u8::from(x < 8)]);
        }
    }
    pixels
}

fn reference_raw_mips(base: &[[u8; 4]], colour: [u8; 3]) -> Vec<Vec<u8>> {
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

#[test]
fn compiler_marks_only_leaf_faces_as_alpha_cutout() {
    let (_directory, resource_pack, records) = leaf_material_fixture();
    let compiled = compile_pack(&resource_pack, &records).expect("compile leaf materials");

    assert_eq!(MATERIAL_FLAG_UV_MASK, 0x0f);
    assert_eq!(MATERIAL_FLAG_ALPHA_CUTOUT, 0x100);
    assert_eq!(MATERIAL_FLAG_LIQUID_DEPTH_WRITE, 0x800);
    assert_eq!(MATERIAL_FLAGS_MASK, 0xfff);
    assert_eq!(std::mem::size_of::<Material>(), 12);
    let opaque_id = compiled.visuals[0].faces[BlockFace::Up as usize];
    let opaque = compiled.materials[opaque_id as usize];
    assert_eq!(opaque.flags & MATERIAL_FLAG_ALPHA_CUTOUT, 0);
    for leaf in 1..=3 {
        for face in BlockFace::ALL {
            let material_id = compiled.visuals[leaf].faces[face as usize];
            assert_ne!(material_id, DIAGNOSTIC_MATERIAL);
            let material = compiled.materials[material_id as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_UV_MASK, 0);
        }
    }
    let cherry_id = compiled.visuals[1].faces[BlockFace::Up as usize];
    let cherry = compiled.materials[cherry_id as usize];
    assert_eq!(
        opaque.texture.layer(),
        cherry.texture.layer(),
        "pixels must remain deduplicated"
    );
    assert_ne!(
        opaque_id, cherry_id,
        "opaque and cutout descriptors must differ"
    );
    assert!(
        compiled
            .materials
            .iter()
            .all(|material| material.flags & !MATERIAL_FLAGS_MASK == 0)
    );

    let baseline = encode_blob(&compiled).expect("encode cutout baseline");
    for iteration in 0..100_u64 {
        let shuffled = shuffled_records(&records, 0x9e37_79b9 ^ iteration);
        let actual = compile_pack(&resource_pack, &shuffled).expect("compile shuffled cutout pack");
        assert_eq!(encode_blob(&actual).expect("encode shuffle"), baseline);
    }
}

#[test]
fn compiler_assigns_generic_birch_evergreen_and_self_colored_leaf_flags() {
    let directory = tempfile::tempdir().expect("create leaf class fixture");
    write_pack(
        directory.path(),
        r#"{
            "oak_leaves":{"textures":"leaves"},
            "birch_leaves":{"textures":"leaves"},
            "spruce_leaves":{"textures":"leaves"},
            "cherry_leaves":{"textures":"leaves"}
        }"#,
        r#"{"texture_data":{"leaves":{"textures":"textures/blocks/leaves"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/leaves",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [80, 160, 40, 255]),
    );
    let leaf = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 100, "minecraft:oak_leaves", "{}", leaf),
        record(1, 101, "minecraft:birch_leaves", "{}", leaf),
        record(2, 102, "minecraft:spruce_leaves", "{}", leaf),
        record(3, 103, "minecraft:cherry_leaves", "{}", leaf),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile leaf classes");
    let flags = |record_id| material_for_face(&compiled, record_id, BlockFace::Up).flags;

    assert_eq!(
        flags(0),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT
    );
    assert_eq!(
        flags(1),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE
    );
    assert_eq!(
        flags(2),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE
    );
    assert_eq!(flags(3), MATERIAL_FLAG_ALPHA_CUTOUT);
    assert_eq!(flags(3) & MATERIAL_FLAG_TINT_MASK, 0);
    assert_eq!(flags(3) & MATERIAL_FLAG_FOLIAGE_CLASS_MASK, 0);
}

#[test]
fn assetc_summary_reports_deterministic_cutout_material_count() {
    let (directory, resource_pack, records) = leaf_material_fixture();
    let registry = directory.path().join("registry.bin");
    let biome_registry = directory.path().join("biome-registry.bin");
    let output_blob = directory.path().join("vanilla-v1001.mcbea");
    fs::write(&registry, registry_bytes(&records)).expect("write registry fixture");
    fs::write(&biome_registry, biome_registry_bytes(0, "minecraft:plains"))
        .expect("write biome registry fixture");
    write_biome_fixture(&resource_pack);
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["compile", "--pack"])
        .arg(&resource_pack)
        .arg("--registry")
        .arg(&registry)
        .arg("--biome-registry")
        .arg(&biome_registry)
        .arg("--out")
        .arg(&output_blob)
        .output()
        .expect("run assetc compile");
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 summary"),
        format!(
            "compiled 4 visuals, 5 materials (3 alpha cutout), 4 texture layers, and 1 biome rules to {}\n",
            output_blob.display()
        )
    );
}

#[test]
fn cutout_mips_preserve_each_layer_coverage_without_cross_layer_bleed() {
    let directory = tempfile::tempdir().expect("create coverage fixture");
    write_pack(
        directory.path(),
        r#"{
            "cherry_leaves":{"textures":"red"},
            "azalea_leaves":{"textures":"blue"},
            "azalea_leaves_flowered":{"textures":"green"}
        }"#,
        r#"{"texture_data":{
            "red":{"textures":"textures/blocks/a_red"},
            "blue":{"textures":"textures/blocks/b_blue"},
            "green":{"textures":"textures/blocks/c_green"}
        }}"#,
        "[]",
    );
    let red = cutout_pattern([255, 0, 0], 78);
    let blue = cutout_pattern([0, 0, 255], 181);
    let green = aligned_half_pattern([0, 255, 0]);
    for (path, pixels) in [
        ("textures/blocks/a_red", &red),
        ("textures/blocks/b_blue", &blue),
        ("textures/blocks/c_green", &green),
    ] {
        write_png(directory.path(), path, TILE_SIZE, TILE_SIZE, pixels);
    }
    let flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 200, "minecraft:cherry_leaves", "{}", flags),
        record(1, 201, "minecraft:azalea_leaves", "{}", flags),
        record(2, 202, "minecraft:azalea_leaves_flowered", "{}", flags),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile coverage fixture");
    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
    assert_eq!(
        blue_layer,
        red_layer + 1,
        "red and blue must be adjacent layers"
    );

    let mut correction_exercised = false;
    for (record_id, base, colour, no_tie) in [
        (0, red.as_slice(), [255, 0, 0], false),
        (1, blue.as_slice(), [0, 0, 255], false),
        (2, green.as_slice(), [0, 255, 0], true),
    ] {
        let layer = material_for_face(&compiled, record_id, BlockFace::Up)
            .texture
            .layer();
        let raw_mips = reference_raw_mips(base, colour);
        let base_survivors = alpha_survivors(&raw_mips[0]);
        for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
            let actual = mip_layer(&compiled, mip_index, layer);
            let raw = &raw_mips[mip_index];
            let pixels = usize::try_from(mip.size * mip.size).expect("small mip");
            let target = (base_survivors * pixels + 128) / 256;
            let expected_scale = reference_nearest_scale(raw, target);
            assert_eq!(
                alpha_survivors(actual),
                reference_nearest_survivors(raw, target),
                "coverage mismatch for record {record_id} mip {mip_index}"
            );
            for (actual, raw) in actual.chunks_exact(4).zip(raw.chunks_exact(4)) {
                assert_eq!(&actual[..3], &raw[..3], "coverage scaling changed RGB");
                let expected_alpha = if mip_index == 0 {
                    raw[3]
                } else {
                    ((u32::from(raw[3]) * expected_scale + 0x8000) >> 16).min(255) as u8
                };
                assert_eq!(
                    actual[3], expected_alpha,
                    "coverage scaling chose the wrong tie-break scale"
                );
            }
            if record_id == 0 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[2] == 0));
            } else if record_id == 1 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[0] == 0));
            }
            if no_tie {
                assert_eq!(alpha_survivors(raw), target, "no-tie fixture missed target");
                assert_eq!(alpha_survivors(actual), target);
            } else if mip_index > 0 && alpha_survivors(raw) != target {
                correction_exercised = true;
            }
        }
    }
    assert!(
        correction_exercised,
        "patterns did not exercise coverage correction"
    );
}

#[test]
fn compiler_rejects_invalid_block_flag_semantics() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"format_version":[1,1,0]}"#,
        r#"{"texture_data":{}}"#,
        "[]",
    );

    for invalid in [
        BlockFlags::from_bits_retain(0x10),
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::AIR | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::LEAF_MODEL | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        let records = [record(0, 1, "minecraft:test", "{}", invalid)];
        assert!(matches!(
            compile_pack(directory.path(), &records),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }
}

fn mip_pixel(
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

#[test]
fn compiler_deduplicates_pixels_without_conflating_uv_flags() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "same_a": {"textures": "red_a"},
            "same_b": {"textures": "red_b"},
            "pillar": {"textures": {"up": "red_a", "down": "red_a", "side": "red_a"}},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red_a": {"textures": "textures/blocks/a_red"},
            "red_b": {"textures": "textures/blocks/b_red_copy"},
            "blue": {"textures": "textures/blocks/c_blue"}
        }}"#,
        "[]",
    );
    let red = solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]);
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/b_red_copy",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/c_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 100, "minecraft:same_a", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 101, "minecraft:same_b", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            102,
            "minecraft:pillar",
            r#"{"pillar_axis":"x"}"#,
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(3, 103, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(
        compiled.texture_pages[0].texture.layers, 3,
        "diagnostic + red + blue"
    );
    assert_eq!(
        compiled.materials.len(),
        4,
        "diagnostic + three descriptors"
    );
    assert_eq!(compiled.visuals[0].faces, compiled.visuals[1].faces);

    let red_plain = material_for_face(&compiled, 0, BlockFace::Up);
    let red_rotated = material_for_face(&compiled, 2, BlockFace::North);
    let blue = material_for_face(&compiled, 3, BlockFace::Up);
    assert_eq!(red_plain.texture.layer(), red_rotated.texture.layer());
    assert_eq!(red_plain.flags, 0);
    assert_eq!(red_rotated.flags, MATERIAL_FLAG_ROTATE_UV);
    assert_ne!(red_plain, red_rotated);
    assert_ne!(red_plain.texture.layer(), blue.texture.layer());
}

#[test]
fn compiler_builds_diagnostic_and_layer_isolated_linear_mips() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "red": {"textures": "red"},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red": {"textures": "textures/blocks/a_red"},
            "blue": {"textures": "textures/blocks/b_blue"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]),
    );
    write_png(
        directory.path(),
        "textures/blocks/b_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 200, "minecraft:red", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 201, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(
        compiled.materials[0],
        Material {
            texture: assets::TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: assets::NO_ANIMATION
        }
    );
    assert_eq!(mip_pixel(&compiled, 0, 0, 0, 0), [255, 0, 255, 255]);
    assert_eq!(mip_pixel(&compiled, 0, 0, 1, 0), [0, 0, 0, 255]);
    assert_eq!(compiled.texture_pages[0].texture.mips.len(), 5);
    assert_eq!(
        compiled.texture_pages[0]
            .texture
            .mips
            .iter()
            .map(|mip| mip.size)
            .collect::<Vec<_>>(),
        [16, 8, 4, 2, 1]
    );

    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
    for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
        for y in 0..mip.size as usize {
            for x in 0..mip.size as usize {
                assert_eq!(
                    mip_pixel(&compiled, mip_index, red_layer, x, y),
                    [255, 0, 0, 255]
                );
                assert_eq!(
                    mip_pixel(&compiled, mip_index, blue_layer, x, y),
                    [0, 0, 255, 255]
                );
            }
        }
    }
}

#[test]
fn compiler_supports_vanilla_glass_and_fails_closed_for_arbitrary_tinted_full_cubes() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "stone": {"textures": "stone"},
            "glass": {"textures": "glass"},
            "tinted_cube": {"textures": "tinted_cube"},
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }}
        }"#,
        r##"{"texture_data": {
            "stone": {"textures": "textures/blocks/stone"},
            "glass": {"textures": "textures/blocks/glass"},
            "tinted_cube": {"textures": {
                "path": "textures/blocks/tinted_cube", "overlay_color": "#79c05a"
            }},
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"}
        }}"##,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let mut glass = solid(TILE_SIZE, TILE_SIZE, [210, 230, 255, 0]);
    glass[0] = [210, 230, 255, 255];
    write_png(
        directory.path(),
        "textures/blocks/glass",
        TILE_SIZE,
        TILE_SIZE,
        &glass,
    );
    for path in [
        "textures/blocks/tinted_cube",
        "textures/blocks/grass_bottom",
        "textures/blocks/grass_side",
        "textures/blocks/grass_top",
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [80, 160, 60, 255]),
        );
    }
    let records = [
        record(0, 300, "minecraft:stone", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 301, "minecraft:glass", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            302,
            "minecraft:tinted_cube",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            3,
            303,
            "minecraft:grass_block",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    assert_eq!(compiled.visuals[1].kind, VisualKind::Cube);
    assert!(
        compiled.visuals[1]
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    for face in BlockFace::ALL {
        assert_eq!(
            material_for_face(&compiled, 1, face).flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    let artifact = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let artifact_glass = artifact.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(artifact_glass.kind(), VisualKind::Cube);
    assert!(artifact_glass.flags().contains(BlockFlags::CUBE_GEOMETRY));
    for face in BlockFace::ALL {
        assert_eq!(
            artifact
                .material(artifact_glass.face(face).material_id())
                .flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    assert_eq!(
        compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6],
        "arbitrary tinted full cube must remain fail closed"
    );
    let grass = compiled.visuals[3];
    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 3, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 3, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_eq!(
            material_for_face(&compiled, 3, face).flags,
            MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
        );
    }
}

#[test]
fn alpha_support_is_scoped_to_each_descriptor_when_a_texture_path_is_shared() {
    let directory = tempfile::tempdir().expect("create shared alpha fixture");
    write_pack(
        directory.path(),
        r#"{
            "red_stained_glass": {"textures": "shared_stained_glass"},
            "red_stained_glass_pane": {"textures": "shared_stained_glass"}
        }"#,
        r#"{"texture_data": {
            "shared_stained_glass": {"textures": "textures/blocks/shared_stained_glass"}
        }}"#,
        "[]",
    );
    let mut pixels = solid(TILE_SIZE, TILE_SIZE, [180, 20, 30, 96]);
    pixels[0] = [180, 20, 30, 255];
    write_png(
        directory.path(),
        "textures/blocks/shared_stained_glass",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    let records = [
        record(
            0,
            400,
            "minecraft:red_stained_glass",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        model_record(
            1,
            401,
            "minecraft:red_stained_glass_pane",
            "{}",
            ModelFamily::Pane,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile shared alpha pack");

    assert_eq!(
        compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6],
        "the pane's blend permission must not admit an opaque full-cube descriptor"
    );
    assert_eq!(compiled.visuals[1].kind, VisualKind::Model);
    let pane = compiled.visuals[1];
    assert!(
        template_quads(&compiled, pane.model_template)
            .iter()
            .all(|quad| compiled.materials[quad.material as usize].flags
                & MATERIAL_FLAG_ALPHA_BLEND
                != 0),
        "the stained pane must retain its per-descriptor blend material"
    );
}

#[test]
fn compiler_output_is_identical_across_shuffled_sources_and_records() {
    fn fixture(blocks: &str, terrain: &str) -> TempDir {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(directory.path(), blocks, terrain, "[]");
        write_png(
            directory.path(),
            "textures/blocks/a",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 255]),
        );
        write_png(
            directory.path(),
            "textures/blocks/z",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [220, 210, 200, 255]),
        );
        directory
    }

    let first = fixture(
        r#"{"alpha":{"textures":"alpha"},"zeta":{"textures":"zeta"}}"#,
        r#"{"texture_data":{"alpha":{"textures":"textures/blocks/a"},"zeta":{"textures":"textures/blocks/z"}}}"#,
    );
    let second = fixture(
        r#"{"zeta":{"textures":"zeta"},"alpha":{"textures":"alpha"}}"#,
        r#"{"texture_data":{"zeta":{"textures":"textures/blocks/z"},"alpha":{"textures":"textures/blocks/a"}}}"#,
    );
    let alpha = record(
        0,
        0xffff_fff0,
        "minecraft:alpha",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );
    let zeta = record(
        1,
        0x8000_0001,
        "minecraft:zeta",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );

    let first = compile_pack(first.path(), &[alpha.clone(), zeta.clone()]).expect("first compile");
    let second = compile_pack(second.path(), &[zeta, alpha]).expect("second compile");

    assert_eq!(first, second);
    assert_eq!(
        encode_blob(&first).expect("encode first"),
        encode_blob(&second).expect("encode second")
    );
}

#[test]
fn compiler_selects_huge_mushroom_face_variants_and_keeps_other_arrays_at_zero() {
    let directory = tempfile::tempdir().expect("create fixture");
    let families = [
        ("brown_mushroom_block", "mushroom_brown"),
        ("red_mushroom_block", "mushroom_red"),
        ("mushroom_stem", "mushroom_stem"),
    ];
    let faces = [
        (BlockFace::West, "west", "west"),
        (BlockFace::East, "east", "east"),
        (BlockFace::Down, "down", "bottom"),
        (BlockFace::Up, "up", "top"),
        (BlockFace::North, "north", "north"),
        (BlockFace::South, "south", "south"),
    ];
    let colour = |family: usize, face: usize, bits: u8| {
        let discriminator = 1 + family as u8 * 36 + face as u8 * 2 + u8::from(bits == 15);
        [discriminator, 255 - discriminator, bits, 255]
    };
    let is_static_stem_face = |family: usize, face: BlockFace| {
        family == 2
            && matches!(
                face,
                BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South
            )
    };

    let mut block_entries = serde_json::Map::new();
    let mut terrain_entries = serde_json::Map::new();
    for (family_index, (block_name, texture_prefix)) in families.iter().enumerate() {
        let mut face_entries = serde_json::Map::new();
        for (face_index, (face, block_face, texture_suffix)) in faces.iter().enumerate() {
            let key = format!("{texture_prefix}_{texture_suffix}");
            face_entries.insert((*block_face).into(), serde_json::Value::String(key.clone()));
            let selected_bits: &[u8] = if is_static_stem_face(family_index, *face) {
                terrain_entries.insert(
                    key,
                    serde_json::json!({
                        "textures": format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_static"
                        )
                    }),
                );
                &[0]
            } else {
                let variants = (0..16)
                    .map(|bits| {
                        serde_json::Value::String(format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_{bits}"
                        ))
                    })
                    .collect::<Vec<_>>();
                terrain_entries.insert(key, serde_json::json!({ "textures": variants }));
                &[0, 15]
            };

            for &bits in selected_bits {
                let source = if is_static_stem_face(family_index, *face) {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_static")
                } else {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_{bits}")
                };
                write_png(
                    directory.path(),
                    &source,
                    TILE_SIZE,
                    TILE_SIZE,
                    &solid(TILE_SIZE, TILE_SIZE, colour(family_index, face_index, bits)),
                );
            }
        }
        block_entries.insert(
            (*block_name).into(),
            serde_json::json!({ "textures": face_entries }),
        );
    }

    let unrelated_variants = (0..16)
        .map(|bits| serde_json::Value::String(format!("textures/blocks/unrelated_{bits}")))
        .collect::<Vec<_>>();
    block_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": "unrelated" }),
    );
    terrain_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": unrelated_variants }),
    );
    write_png(
        directory.path(),
        "textures/blocks/unrelated_0",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [240, 120, 60, 255]),
    );

    write_pack(
        directory.path(),
        &serde_json::Value::Object(block_entries).to_string(),
        &serde_json::json!({ "texture_data": terrain_entries }).to_string(),
        "[]",
    );

    let mut records = Vec::new();
    for (family_index, (block_name, _)) in families.iter().enumerate() {
        for (state_index, bits) in [0_u8, 15].into_iter().enumerate() {
            let sequential_id = (family_index * 2 + state_index) as u32;
            records.push(record(
                sequential_id,
                0x8000_1000 + sequential_id,
                &format!("minecraft:{block_name}"),
                &format!(r#"{{"huge_mushroom_bits":{bits}}}"#),
                BlockFlags::CUBE_GEOMETRY,
            ));
        }
    }
    let fallback_states = [
        "{}",
        "null",
        "not JSON",
        r#"{"huge_mushroom_bits":-1}"#,
        r#"{"huge_mushroom_bits":16}"#,
        r#"{"huge_mushroom_bits":"15"}"#,
    ];
    for state in fallback_states {
        let sequential_id = records.len() as u32;
        records.push(record(
            sequential_id,
            0x8000_1000 + sequential_id,
            "minecraft:brown_mushroom_block",
            state,
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    let invalid_stem_id = records.len() as u32;
    records.push(record(
        invalid_stem_id,
        0x8000_1000 + invalid_stem_id,
        "minecraft:mushroom_stem",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    ));
    let unrelated_id = records.len() as u32;
    records.push(record(
        unrelated_id,
        0x8000_1000 + unrelated_id,
        "minecraft:unrelated",
        r#"{"huge_mushroom_bits":15}"#,
        BlockFlags::CUBE_GEOMETRY,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile mushroom variants");
    for (family_index, _) in families.iter().enumerate() {
        for (state_index, bits) in [0_u8, 15].into_iter().enumerate() {
            let sequential_id = family_index * 2 + state_index;
            for (face_index, (face, _, _)) in faces.iter().enumerate() {
                let material = material_for_face(&compiled, sequential_id, *face);
                let expected_bits = if is_static_stem_face(family_index, *face) {
                    0
                } else {
                    bits
                };
                assert_eq!(
                    mip_pixel(&compiled, 0, material.texture.layer(), 0, 0),
                    colour(family_index, face_index, expected_bits),
                    "wrong {bits} texture for {} {face:?}",
                    families[family_index].0
                );
            }
        }
    }
    for sequential_id in 6..6 + fallback_states.len() {
        assert_eq!(
            compiled.visuals[sequential_id].faces, [0; 6],
            "invalid or absent mushroom selector must fail closed"
        );
    }
    assert_eq!(
        compiled.visuals[invalid_stem_id as usize].faces, [0; 6],
        "invalid selector must also fail closed for static mushroom face paths"
    );
    let unrelated = material_for_face(&compiled, unrelated_id as usize, BlockFace::Up);
    assert_eq!(
        mip_pixel(&compiled, 0, unrelated.texture.layer(), 0, 0),
        [240, 120, 60, 255],
        "unrelated terrain arrays must retain variant-zero selection"
    );

    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed records");
    assert_eq!(
        encode_blob(&compiled).expect("encode mushroom variants"),
        encode_blob(&reversed).expect("encode reversed mushroom variants")
    );
}

#[test]
fn compiler_fails_closed_for_noncanonical_mushroom_variant_counts() {
    let directory = tempfile::tempdir().expect("create fixture");
    let variants = (0..15)
        .map(|bits| format!("textures/blocks/mushroom_brown_top_{bits}"))
        .collect::<Vec<_>>();
    write_pack(
        directory.path(),
        r#"{"brown_mushroom_block":{"textures":"mushroom_brown_top"}}"#,
        &serde_json::json!({
            "texture_data": {
                "mushroom_brown_top": { "textures": variants }
            }
        })
        .to_string(),
        "[]",
    );
    let records = [record(
        0,
        0x8000_2000,
        "minecraft:brown_mushroom_block",
        r#"{"huge_mushroom_bits":14}"#,
        BlockFlags::CUBE_GEOMETRY,
    )];

    let compiled = compile_pack(directory.path(), &records)
        .expect("a malformed mushroom variant table must fail closed without loading a texture");

    assert_eq!(compiled.visuals[0].faces, [0; 6]);
    assert_eq!(compiled.materials.len(), 1);
    assert_eq!(compiled.texture_pages[0].texture.layers, 1);
}

#[test]
fn compiler_only_loads_full_cubes_and_builds_equivalent_lookup_tables() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "flower": {"textures": "missing_flower"},
            "stone": {"textures": "stone"}
        }"#,
        r#"{"texture_data": {
            "missing_flower": {"textures": "textures/blocks/not_present"},
            "stone": {"textures": "textures/blocks/stone"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let records = [
        record(
            5,
            0xf000_0005,
            "minecraft:stone",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x8000_0002,
            "minecraft:flower",
            "{}",
            BlockFlags::empty(),
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile full cubes only");

    assert_eq!(compiled.visuals.len(), 6);
    assert_eq!(compiled.visuals[2].faces, [0; 6]);
    assert!(
        compiled.visuals[5]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    assert_eq!(&*compiled.hashed, &[(0x8000_0002, 2), (0xf000_0005, 5)]);
    for &(hash, visual_index) in compiled.hashed.iter() {
        let record = records
            .iter()
            .find(|record| record.network_hash == hash)
            .expect("hash record");
        assert_eq!(visual_index, record.sequential_id);
        assert_eq!(
            compiled.visuals[visual_index as usize],
            compiled.visuals[record.sequential_id as usize]
        );
    }
}

#[test]
fn compiler_compiles_grass_and_flipbook_faces_but_keeps_unlisted_blocks_diagnostic() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }},
            "seaLantern": {"textures": "sea_lantern"}
        }"#,
        r#"{"texture_data": {
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"},
            "sea_lantern": {"textures": "textures/blocks/sea_lantern"}
        }}"#,
        r#"[{
            "flipbook_texture": "textures/blocks/sea_lantern",
            "atlas_tile": "sea_lantern",
            "ticks_per_frame": 5
        }]"#,
    );
    for (path, colour) in [
        ("textures/blocks/grass_bottom", [80, 50, 20, 255]),
        ("textures/blocks/grass_top", [70, 190, 50, 255]),
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    write_tga(
        directory.path(),
        "textures/blocks/grass_side",
        TILE_SIZE as u16,
        TILE_SIZE as u16,
        &solid(TILE_SIZE, TILE_SIZE, [100, 150, 60, 255]),
    );
    let mut sea_lantern_strip = solid(TILE_SIZE, TILE_SIZE, [30, 180, 200, 255]);
    sea_lantern_strip.extend(solid(TILE_SIZE, TILE_SIZE, [50, 210, 230, 255]));
    write_png(
        directory.path(),
        "textures/blocks/sea_lantern",
        TILE_SIZE,
        TILE_SIZE * 2,
        &sea_lantern_strip,
    );
    let records = [
        record(
            0,
            0xde31_28b4,
            "minecraft:grass_block",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            1,
            0x1111_1111,
            "minecraft:sea_lantern",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x2222_2222,
            "minecraft:invisible_bedrock",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile deferred visuals");
    let grass = compiled.visuals[0];
    let sea_lantern = compiled.visuals[1];

    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 0, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::North).flags,
        MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
    );
    assert!(
        sea_lantern
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(
        compiled.materials[sea_lantern.faces[0] as usize].animation,
        0
    );
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.materials.len(), 5);
    assert_eq!(compiled.texture_pages[0].texture.layers, 6);
}

#[test]
fn compiler_installs_recognized_flipbooks_after_loading_the_strip() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "water":{"textures":{"up":"water_still","down":"water_still","side":"water_flow"}},
            "lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}},
            "broken_water":{"textures":{"west":"water_flow","east":"water_flow","up":"water_still","north":"water_flow","south":"water_flow"}}
        }"#,
        r#"{"texture_data":{
            "water_still":{"textures":"textures/blocks/water_still"},
            "water_flow":{"textures":"textures/blocks/water_flow"},
            "lava_still":{"textures":"textures/blocks/lava_still"},
            "lava_flow":{"textures":"textures/blocks/lava_flow"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/water_still","atlas_tile":"water_still"},
            {"flipbook_texture":"textures/blocks/water_flow","atlas_tile":"water_flow"},
            {"flipbook_texture":"textures/blocks/lava_still","atlas_tile":"lava_still"},
            {"flipbook_texture":"textures/blocks/lava_flow","atlas_tile":"lava_flow"}
        ]"#,
    );
    let mut water_record = model_record(
        0,
        700,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":3}}"#,
        ModelFamily::Liquid,
    );
    water_record.contributor_role = ContributorRole::LiquidAdditional;
    assert_eq!(
        water_record.model_state.get(ModelStateField::LiquidDepth),
        None
    );
    let mut lava_record = model_record(
        1,
        701,
        "minecraft:lava",
        r#"{"liquid_depth":{"type":"int","value":15}}"#,
        ModelFamily::Liquid,
    );
    lava_record.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_depth = model_record(2, 702, "minecraft:water", "{}", ModelFamily::Liquid);
    missing_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut out_of_range_depth = model_record(
        3,
        703,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":16}}"#,
        ModelFamily::Liquid,
    );
    out_of_range_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_face = model_record(
        4,
        704,
        "minecraft:broken_water",
        r#"{"liquid_depth":{"type":"int","value":2}}"#,
        ModelFamily::Liquid,
    );
    missing_face.contributor_role = ContributorRole::LiquidAdditional;
    let records = [
        water_record,
        lava_record,
        missing_depth,
        out_of_range_depth,
        missing_face,
    ];
    for (index, path) in ["water_still", "water_flow", "lava_still", "lava_flow"]
        .into_iter()
        .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [20 + index as u8, 80, 200, 180]);
        strip.extend(solid(
            TILE_SIZE,
            TILE_SIZE,
            [30 + index as u8, 100, 220, 180],
        ));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile flipbook reference");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Liquid);
    assert_eq!(
        compiled.visuals[0].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert!(
        !compiled.visuals[0]
            .flags
            .contains(BlockFlags::CUBE_GEOMETRY)
    );
    assert_eq!(compiled.visuals[0].variant, 3);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Liquid);
    assert_eq!(compiled.visuals[1].variant, 15);
    for visual in &compiled.visuals[2..] {
        assert_eq!(visual.kind, VisualKind::Diagnostic);
        assert_eq!(visual.faces, [DIAGNOSTIC_MATERIAL; 6]);
        assert_eq!(visual.variant, 0);
    }
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::Down as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::West as usize],
        compiled.visuals[0].faces[BlockFace::East as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::North as usize],
        compiled.visuals[0].faces[BlockFace::South as usize]
    );
    assert_ne!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::North as usize]
    );
    assert_eq!(
        compiled.visuals[1].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert_eq!(
        compiled.materials.len(),
        5,
        "unsupported liquid descriptors sharing alpha strips must not create materials"
    );
    let water_material =
        compiled.materials[compiled.visuals[0].faces[BlockFace::Up as usize] as usize];
    let water_flow =
        compiled.materials[compiled.visuals[0].faces[BlockFace::North as usize] as usize];
    assert_eq!(
        water_material.flags,
        MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
    );
    assert_eq!(water_material.animation, 0);
    assert_eq!(water_flow.animation, 1);
    let lava_still = compiled.materials[compiled.visuals[1].faces[BlockFace::Up as usize] as usize];
    let lava_flow =
        compiled.materials[compiled.visuals[1].faces[BlockFace::North as usize] as usize];
    assert_eq!(lava_still.flags, MATERIAL_FLAG_LIQUID_DEPTH_WRITE);
    assert_eq!(lava_flow.flags, MATERIAL_FLAG_LIQUID_DEPTH_WRITE);
    assert_eq!(lava_still.animation, 2);
    assert_eq!(lava_flow.animation, 3);
    assert_eq!(compiled.animations.len(), 4);
    assert_eq!(compiled.animation_frames.len(), 8);
    assert_eq!(compiled.texture_pages[0].texture.layers, 9);
}

#[test]
fn compiler_maps_every_protocol_1001_lava_depth_for_both_runtime_names() {
    let directory = tempfile::tempdir().expect("create lava fixture");
    write_pack(
        directory.path(),
        r#"{
            "lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}},
            "flowing_lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}}
        }"#,
        r#"{"texture_data":{
            "lava_still":{"textures":"textures/blocks/lava_still"},
            "lava_flow":{"textures":"textures/blocks/lava_flow"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/lava_still","atlas_tile":"lava_still"},
            {"flipbook_texture":"textures/blocks/lava_flow","atlas_tile":"lava_flow"}
        ]"#,
    );
    for (index, path) in ["lava_still", "lava_flow"].into_iter().enumerate() {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [240, 40 + index as u8, 4, 255]);
        strip.extend(solid(TILE_SIZE, TILE_SIZE, [255, 80 + index as u8, 8, 255]));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }

    let mut records = Vec::new();
    for (name_index, name) in ["minecraft:lava", "minecraft:flowing_lava"]
        .into_iter()
        .enumerate()
    {
        for depth in 0..16_u32 {
            let sequential_id = (name_index as u32) * 16 + depth;
            let mut record = model_record(
                sequential_id,
                10_000 + sequential_id,
                name,
                &format!(r#"{{"liquid_depth":{{"type":"int","value":{depth}}}}}"#),
                ModelFamily::Liquid,
            );
            record.contributor_role = ContributorRole::LiquidAdditional;
            records.push(record);
        }
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile all lava states");
    assert_eq!(compiled.visuals.len(), 32);
    for (sequential_id, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(visual.kind, VisualKind::Liquid, "state {sequential_id}");
        assert_eq!(
            visual.contributor_role,
            ContributorRole::LiquidAdditional,
            "state {sequential_id}",
        );
        assert_eq!(visual.variant, sequential_id as u32 % 16);
        for material in visual.faces {
            assert_ne!(material, DIAGNOSTIC_MATERIAL);
            assert_eq!(
                compiled.materials[material as usize].flags,
                MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
            );
        }
    }
    assert_eq!(compiled.animations.len(), 2);
}

fn one_texture_fixture() -> (TempDir, RegistryRecord, PathBuf) {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken"}}}"#,
        "[]",
    );
    let expected = directory.path().join("textures/blocks/broken.png");
    (
        directory,
        record(0, 900, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY),
        expected,
    )
}

fn assert_source_context(error: &AssetError, expected_path: &Path) {
    let rendered = error.to_string();
    assert!(
        rendered.contains("broken_key"),
        "missing source key: {rendered}"
    );
    assert!(
        rendered.contains(expected_path.to_string_lossy().as_ref()),
        "missing source path: {rendered}"
    );
}

#[test]
fn compiler_reports_missing_malformed_and_wrong_size_png_sources() {
    let (missing_root, missing_record, missing_path) = one_texture_fixture();
    let error = compile_pack(missing_root.path(), &[missing_record]).expect_err("missing PNG");
    assert!(matches!(error, AssetError::TextureIo { .. }));
    assert_source_context(&error, &missing_path);

    let (malformed_root, malformed_record, malformed_path) = one_texture_fixture();
    write_file(&malformed_path, b"not a png");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed PNG");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = one_texture_fixture();
    write_file(&sized_path, png_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong PNG size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_reports_malformed_and_wrong_size_tga_with_source_context() {
    let fixture = || {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(
            directory.path(),
            r#"{"broken":{"textures":"broken_key"}}"#,
            r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.tga"}}}"#,
            "[]",
        );
        let path = directory.path().join("textures/blocks/broken.tga");
        let record = record(0, 901, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);
        (directory, record, path)
    };

    let (malformed_root, malformed_record, malformed_path) = fixture();
    write_file(&malformed_path, b"not a tga");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed TGA");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = fixture();
    write_file(&sized_path, tga_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong TGA size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_rejects_unsupported_static_texture_formats_with_source_context() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.jpg"}}}"#,
        "[]",
    );
    let path = directory.path().join("textures/blocks/broken.jpg");
    write_file(&path, b"not a supported static texture");
    let record = record(0, 902, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);

    let error = compile_pack(directory.path(), &[record]).expect_err("unsupported texture format");
    let rendered = error.to_string();
    assert!(rendered.contains("unsupported"), "{rendered}");
    assert!(rendered.contains("broken_key"), "{rendered}");
    assert!(
        rendered.contains(path.to_string_lossy().as_ref()),
        "{rendered}"
    );
}

#[test]
fn compiler_rolls_reachable_layers_into_the_bounded_second_page() {
    let directory = tempfile::tempdir().expect("create fixture");
    let mut blocks = String::from("{");
    let mut terrain = String::from(r#"{"texture_data":{"#);
    let mut records = Vec::with_capacity(MAX_TEXTURE_LAYERS);
    for index in 0..MAX_TEXTURE_LAYERS {
        if index != 0 {
            blocks.push(',');
            terrain.push(',');
        }
        write!(blocks, r#""block_{index}":{{"textures":"key_{index}"}}"#)
            .expect("append block JSON");
        write!(
            terrain,
            r#""key_{index}":{{"textures":"textures/generated/layer_{index}"}}"#
        )
        .expect("append terrain JSON");
        let color = [index as u8, (index >> 8) as u8, (index >> 16) as u8, 255];
        write_png(
            directory.path(),
            &format!("textures/generated/layer_{index}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, color),
        );
        records.push(record(
            index as u32,
            0x8000_0000 + index as u32,
            &format!("minecraft:block_{index}"),
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    blocks.push('}');
    terrain.push_str("}}");
    write_pack(directory.path(), &blocks, &terrain, "[]");

    let compiled = compile_pack(directory.path(), &records).expect("two-page asset set");
    assert_eq!(compiled.texture_pages.len(), 2);
    assert_eq!(
        compiled.texture_pages[0].texture.layers,
        MAX_TEXTURE_LAYERS as u32
    );
    assert_eq!(compiled.texture_pages[1].texture.layers, 1);
}
