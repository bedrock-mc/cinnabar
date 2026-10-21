use std::sync::Arc;

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, CompiledAssets, ContributorRole, Material,
    NetworkIdMode, RegistryRecord, RuntimeAssets, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, VisualSupport,
};
use client_world::WorldAuthority;
use meshing::{BlockClassifier, Face, Neighbourhood, mesh_sub_chunk};
use world::{ChunkKey, DecodedLevelChunk, SubChunkKey};

use super::{BREG_V2193, active_content_registry_protocol, bind};

const CUSTOM: &str = "test:palette";
const COLOURS: [[u8; 4]; 5] = [
    [255, 0, 255, 255],
    [166, 112, 62, 255],
    [91, 98, 103, 255],
    [27, 183, 71, 255],
    [23, 61, 211, 255],
];

#[test]
fn sequential_palette_decode_to_render_and_pick() {
    palette_path(false);
}

#[test]
fn hashed_palette_decode_to_render_and_pick() {
    palette_path(true);
}

/// Exercises full and omitted-definition palettes through the same session consumers.
fn palette_path(hashed: bool) {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        include_bytes!("../../../../../assets/data/block-physics-v2193.bin"),
        version,
    )
    .unwrap();
    let air = records
        .iter()
        .find(|r| &*r.name == assets::facts::block::AIR_IDENTIFIER)
        .unwrap();
    let plank = records
        .iter()
        .find(|r| &*r.name == "minecraft:oak_planks")
        .unwrap();
    let stone = records
        .iter()
        .find(|r| &*r.name == "minecraft:stone")
        .unwrap();
    let base = original_art(&records, air, plank, stone);
    let custom_nbt = two_state_definition();
    let empty_nbt = [10, 0, 0];
    for supply_vanilla in [true, false] {
        let mut definitions = vec![("", empty_nbt.as_slice()), (CUSTOM, custom_nbt.as_slice())];
        if supply_vanilla {
            for block in assets::server_defined_blocks() {
                definitions.push(("", &empty_nbt));
                definitions.push((block.name, &empty_nbt));
            }
        }
        definitions.push(("", &empty_nbt));
        let game = protocol::test_support::startup::with_block_definitions(definitions, hashed);
        let blocks = &game.custom_blocks;
        assert_eq!(blocks.total_states(), 2);
        let registration = registries.begin_session_blocks(blocks, hashed);
        assert!(registration.supported);
        let mode = if game.world.block_network_ids_are_hashes {
            NetworkIdMode::Hashed
        } else {
            NetworkIdMode::Sequential
        };
        let first_custom = records.len() as u32;
        let custom_hashes = blocks.blocks[0].hashed_states();
        let overlay = BlockOverlay {
            visuals: (3..5).map(cube).collect(),
            light_properties: vec![assets::LightProperties::OPAQUE_DARK; 2],
            materials: (0..COLOURS.len()).map(material).collect(),
            hashes: custom_hashes.iter().map(|state| Some(state.hash)).collect(),
            ..Default::default()
        };
        let runtime = Arc::new(base.with_block_overlay(first_custom, &overlay).unwrap());
        let mut authority = WorldAuthority::new(game.world, runtime.clone(), None, [0.0; 3], None);
        if let Some(ids) = registration.custom_ids {
            authority.set_custom_block_ids(ids);
        }
        authority.set_custom_block_identities(blocks);
        authority.set_sequential_id_remap(registration.remap);

        // Build the server palette independently, without using the client's remap to encode it.
        let mut palette: Vec<_> = records
            .iter()
            .filter(|r| supply_vanilla || !server_defined(r.sequential_id))
            .map(|r| (&*r.name, r.sequential_id, r.network_hash))
            .collect();
        // Reserved carrier slots retain their positions; only named runs determine insertions.
        let custom_key = (protocol::block_name_sort_key(CUSTOM), CUSTOM);
        let insertion = palette
            .iter()
            .position(|&(name, _, _)| {
                name != assets::facts::block::RESERVED_BLOCK_NAME
                    && (protocol::block_name_sort_key(name), name) > custom_key
            })
            .unwrap_or(palette.len());
        palette.splice(
            insertion..insertion,
            custom_hashes
                .iter()
                .enumerate()
                .map(|(index, state)| (CUSTOM, first_custom + index as u32, state.hash)),
        );
        let wire_id = |internal| {
            let index = palette
                .iter()
                .position(|&(_, id, _)| id == internal)
                .unwrap();
            if hashed {
                palette[index].2
            } else {
                index as u32
            }
        };
        for (wire, &(_, internal, hash)) in palette.iter().enumerate() {
            let expected = if hashed { hash } else { wire as u32 };
            let stored = if hashed { hash } else { internal };
            assert_eq!(authority.block_network_id(stored), Some(expected));
        }
        let cells = [
            ([4, 4, 4], plank.sequential_id, &*plank.name, COLOURS[1]),
            ([5, 4, 4], stone.sequential_id, &*stone.name, COLOURS[2]),
            ([6, 4, 4], first_custom, CUSTOM, COLOURS[3]),
            ([7, 4, 4], first_custom + 1, CUSTOM, COLOURS[4]),
        ];
        let wire_palette: Vec<_> = std::iter::once(wire_id(air.sequential_id))
            .chain(cells.iter().map(|&(_, id, _, _)| wire_id(id)))
            .collect();
        let payload = chunk_bytes(&wire_palette, &cells.map(|(pos, ..)| pos));
        let decoded = DecodedLevelChunk::decode(0, 1, &payload, &authority.decode_ids(0));
        assert_eq!(decoded.bytes_consumed(), payload.len());
        let key = SubChunkKey::new(0, 0, 0, 0);
        authority
            .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded)
            .unwrap();
        let sub = authority.terrain().sub_chunk(key).unwrap();
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(authority.air_block_id()),
            &runtime,
            mode,
            &Neighbourhood::empty(),
            &sub,
        );
        assert!(mesh.quad_count() > 0, "decoded cells must reach meshing");
        for (pos, internal, name, colour) in cells {
            let expected_id = if hashed { wire_id(internal) } else { internal };
            assert_eq!(sub.runtime_id(0, pos[0], pos[1], pos[2]), Some(expected_id));
            assert_eq!(registries.block_identifier(mode, expected_id), Some(name));
            let top = mesh
                .quads()
                .iter()
                .find(|quad| quad.face() == Face::PositiveY && quad.origin() == pos)
                .expect("each distinct block must emit its own top face");
            assert_eq!((top.width(), top.height()), (1, 1));
            assert_eq!(artwork(&runtime, top.material_id()), colour);
            let hit = sim::PaletteWorld::new(
                authority.terrain(),
                registries.registry(mode),
                0,
                Default::default(),
            )
            .block_interaction_ray_current(
                sim::Vec3::new(f64::from(pos[0]) + 0.5, 6.5, f64::from(pos[2]) + 0.5),
                sim::Vec3::new(0.0, -1.0, 0.0),
                4.0,
            )
            .unwrap()
            .expect("the drawn block must be pickable");
            assert_eq!(hit.block_pos, pos.map(i32::from));
            assert_eq!(hit.runtime_id, expected_id);
            assert_eq!(
                registries.block_identifier(mode, hit.runtime_id),
                Some(name)
            );
            assert_eq!(
                authority.block_network_id(hit.runtime_id),
                Some(wire_id(internal))
            );
        }
        // The drawn control above proves that absence here cannot pass on an empty mesh.
        assert!(
            mesh.quads()
                .iter()
                .all(|quad| quad.material_id() != assets::DIAGNOSTIC_MATERIAL)
        );
    }
}

/// Reports whether an internal state requires a server definition in the pinned registry.
fn server_defined(id: u32) -> bool {
    assets::server_defined_blocks().iter().any(|block| {
        (block.first_internal_id..block.first_internal_id + block.state_count).contains(&id)
    })
}

/// Assigns a solid cube to one original artwork layer.
fn cube(material: u32) -> BlockVisual {
    BlockVisual {
        faces: [material; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::Exact,
        ..BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
    }
}

/// Binds a material to its same-numbered layer of the original base artwork.
fn material(layer: usize) -> Material {
    Material {
        texture: TextureRef::new(0, layer as u32).unwrap(),
        ..Material::unvaried()
    }
}

/// Builds a mandatory carrier with original solid colours and the pinned registry identities.
fn original_art(
    records: &[RegistryRecord],
    air: &RegistryRecord,
    plank: &RegistryRecord,
    stone: &RegistryRecord,
) -> RuntimeAssets {
    let mut visuals =
        vec![BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary); records.len()];
    visuals[air.sequential_id as usize] = BlockVisual {
        flags: BlockFlags::AIR,
        kind: VisualKind::Invisible,
        support: VisualSupport::Exact,
        contributor_role: ContributorRole::Air,
        ..visuals[air.sequential_id as usize]
    };
    visuals[plank.sequential_id as usize] = cube(1);
    visuals[stone.sequential_id as usize] = cube(2);
    let mut hashes: Vec<_> = records
        .iter()
        .map(|r| (r.network_hash, r.sequential_id))
        .collect();
    hashes.sort_unstable();
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; visuals.len()].into(),
        visuals: visuals.into(),
        hashed: hashes.into(),
        materials: (0..COLOURS.len()).map(material).collect(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: COLOURS.len() as u32,
            mips: (0..assets::MIP_COUNT)
                .map(|level| {
                    let size = assets::TILE_SIZE >> level;
                    TextureMip {
                        size,
                        rgba8: COLOURS
                            .iter()
                            .flat_map(|colour| colour.repeat(size as usize * size as usize))
                            .collect(),
                    }
                })
                .collect(),
        })]
        .into(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

/// Resolves emitted geometry all the way to its original RGBA artwork.
fn artwork(runtime: &RuntimeAssets, material: u32) -> [u8; 4] {
    let texture = runtime.material(material).texture;
    let mip = &runtime.texture_pages()[texture.page() as usize]
        .texture
        .mips[0];
    let offset = texture.layer() as usize * mip.size as usize * mip.size as usize * 4;
    mip.rgba8[offset..offset + 4].try_into().unwrap()
}

/// Encodes one named two-value property in network NBT without installed fixtures.
fn two_state_definition() -> Vec<u8> {
    let mut bytes = vec![10, 0];
    bytes.extend([9, 10]);
    bytes.extend(b"properties");
    bytes.extend([10, 2, 8, 4]);
    bytes.extend(b"name");
    bytes.push(12);
    bytes.extend(b"test:variant");
    bytes.extend([9, 4]);
    bytes.extend(b"enum");
    bytes.extend([1, 4, 0, 1, 0, 0]);
    bytes
}

/// Encodes unsigned values using the signed runtime-palette varint representation.
fn zigzag(bytes: &mut Vec<u8>, id: u32) {
    let mut value = (id << 1) ^ ((id as i32 >> 31) as u32);
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

/// Encodes a version-nine subchunk with air plus four distinct adjacent cells.
fn chunk_bytes(palette: &[u32], positions: &[[u8; 3]]) -> Vec<u8> {
    let bits = 3;
    let per_word = 32 / bits;
    let mut words = vec![0_u32; 4096_usize.div_ceil(per_word)];
    for (index, &[x, y, z]) in positions.iter().enumerate() {
        let cell = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        words[cell / per_word] |= (index as u32 + 1) << ((cell % per_word) * bits);
    }
    let mut bytes = vec![9, 1, 0, ((bits << 1) | 1) as u8];
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    zigzag(&mut bytes, palette.len() as u32);
    for &id in palette {
        zigzag(&mut bytes, id);
    }
    bytes
}
