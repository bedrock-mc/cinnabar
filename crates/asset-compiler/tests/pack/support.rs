pub use std::{fs, path::Path};

pub use asset_compiler::{
    BlockFace, MAX_FLIPBOOK_FRAMES, MAX_FLIPBOOKS, TextureKey, read_pack, resolve_texture_key,
};
pub use assets::{
    AssetError, BlockFlags, CollisionConfidence, ContributorRole, ModelFamily, ModelState,
    ModelStateField, RegistryProvenance, RegistryRecord, read_registry,
};
pub use tempfile::TempDir;

pub const MINIMAL_BLOCKS: &str = r#"{
    "format_version": [1, 1, 0],
    "stone": { "textures": "stone" }
}"#;
pub const MINIMAL_TERRAIN: &str = r#"{
    "texture_data": {
        "stone": { "textures": "textures/blocks/stone" }
    }
}"#;
pub const EMPTY_FLIPBOOKS: &str = "[]";
pub type RegistryFixture<'a> = (u32, u32, u8, &'a [u8], &'a [u8]);

pub fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture");
}

pub fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    write_file(root.join("blocks.json"), blocks);
    write_file(root.join("textures/terrain_texture.json"), terrain);
    write_file(root.join("textures/flipbook_textures.json"), flipbooks);
}

pub fn minimal_pack() -> TempDir {
    let directory = tempfile::tempdir().expect("create pack fixture");
    write_pack(
        directory.path(),
        MINIMAL_BLOCKS,
        MINIMAL_TERRAIN,
        EMPTY_FLIPBOOKS,
    );
    directory
}

pub fn pack_with_flipbooks(flipbooks: &str) -> TempDir {
    let directory = tempfile::tempdir().expect("create flipbook fixture");
    write_pack(
        directory.path(),
        MINIMAL_BLOCKS,
        r#"{
            "texture_data": {
                "stone": { "textures": "textures/blocks/stone" },
                "water": { "textures": "textures/blocks/water" },
                "lava": { "textures": "textures/blocks/lava" }
            }
        }"#,
        flipbooks,
    );
    directory
}

pub fn registry_bytes(records: &[RegistryFixture<'_>]) -> Vec<u8> {
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for &(sequential_id, network_hash, flags, name, state) in records {
        bytes.extend_from_slice(&sequential_id.to_le_bytes());
        bytes.extend_from_slice(&network_hash.to_le_bytes());
        bytes.push(flags);
        bytes.push(if flags & 1 != 0 { 1 } else { 0 });
        bytes.push(if flags & 1 != 0 { 2 } else { 0 });
        bytes.push(0);
        bytes.push(if flags & 4 != 0 { 0x3f } else { 0 });
        bytes.push(0);
        bytes.push(1 << 1);
        bytes.push(0);
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
        for _ in 0..8 {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(state);
    }
    bytes
}

pub fn record(name: &str, canonical_state: &str) -> RegistryRecord {
    RegistryRecord {
        sequential_id: 7,
        network_hash: 0x8000_0007,
        name: name.into(),
        canonical_state: canonical_state.into(),
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        model_family: ModelFamily::Cube,
        contributor_role: ContributorRole::Primary,
        model_state: ModelState::default(),
        face_coverage: 0x3f,
        collision_seed: Default::default(),
        provenance: RegistryProvenance::DRAGONFLY,
    }
}

pub fn assert_key(actual: TextureKey, expected_key: &str, rotate_uv: bool) {
    assert_eq!(actual.key.as_deref(), Some(expected_key));
    assert_eq!(actual.rotate_uv, rotate_uv);
}
