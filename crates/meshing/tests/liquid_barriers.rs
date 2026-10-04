//! Ice blocks flow and height sampling and hides contacting water side faces.

use std::{fs, sync::OnceLock};

use assets::{
    BlockFlags, ContributorRole, ModelFamily, NetworkIdMode, RegistryProvenance, RegistryRecord,
    RuntimeAssets, encode_blob,
};
use image::{Rgba, RgbaImage};
use meshing::{BlockClassifier, Face, PackedLiquidQuad, mesh_sub_chunk_in_neighbourhood};
use pack_compiler::compile_pack;
use world::{BlockUpdate, ChunkStore, MeshNeighbourhood, SubChunk, SubChunkKey};

struct Fixture {
    assets: RuntimeAssets,
    records: Vec<RegistryRecord>,
}

impl Fixture {
    fn id(&self, name: &str, mode: NetworkIdMode) -> u32 {
        let record = self
            .records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap();
        match mode {
            NetworkIdMode::Sequential => record.sequential_id,
            NetworkIdMode::Hashed => record.network_hash,
        }
    }

    fn chunk(&self, mode: NetworkIdMode, placements: &[(u32, [u8; 3])]) -> SubChunk {
        let key = SubChunkKey::new(0, 0, 0, 0);
        let mut store = ChunkStore::new();
        store.apply_request_mode_air(key).unwrap();
        store.mark_sub_chunk_loaded(key).unwrap();
        let air = self.id("minecraft:air", mode);
        for &(id, [x, y, z]) in placements {
            store
                .update_block(key, BlockUpdate::new(x, y, z, 0, id), air)
                .unwrap();
        }
        store.sub_chunk(key).unwrap().as_ref().clone()
    }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = [
            (
                "minecraft:air",
                BlockFlags::AIR,
                ModelFamily::Air,
                ContributorRole::Air,
            ),
            (
                "minecraft:water",
                BlockFlags::empty(),
                ModelFamily::Liquid,
                ContributorRole::LiquidAdditional,
            ),
            (
                "minecraft:ice",
                BlockFlags::empty(),
                ModelFamily::Unknown,
                ContributorRole::Primary,
            ),
            (
                "minecraft:stone",
                BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                ModelFamily::Cube,
                ContributorRole::Primary,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(
            |(index, (name, flags, model_family, contributor_role))| RegistryRecord {
                sequential_id: index as u32,
                network_hash: index as u32 + 101,
                name: name.into(),
                canonical_state: if model_family == ModelFamily::Liquid {
                    r#"{"liquid_depth":{"type":"int","value":0}}"#.into()
                } else {
                    "{}".into()
                },
                flags,
                model_family,
                contributor_role,
                model_state: Default::default(),
                face_coverage: 0,
                collision_seed: Default::default(),
                provenance: RegistryProvenance::PMMP,
            },
        )
        .collect::<Vec<_>>();
        let mut records = records;
        let ice = records
            .iter_mut()
            .find(|record| record.name.as_ref() == "minecraft:ice")
            .unwrap();
        let sequential_id = ice.sequential_id;
        // compile_pack owns the legacy fallback table. Its real ice hash and
        // canonical state must hit that table; invented hashes hide alpha bugs.
        *ice = assets::read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .unwrap()
            .into_iter()
            .find(|record| record.name.as_ref() == "minecraft:ice")
            .unwrap();
        ice.sequential_id = sequential_id;
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        fs::write(
            directory.path().join("blocks.json"),
            r#"{
            "water":{"textures":{"down":"still","side":"flow","up":"still"}},
            "ice":{"textures":"ice"},
            "stone":{"textures":"stone"}
        }"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{
            "resource_pack_name":"Original liquid barrier fixture",
            "texture_name":"atlas.terrain",
            "texture_data":{
                "still":{"textures":"textures/blocks/still"},
                "flow":{"textures":"textures/blocks/flow"},
                "ice":{"textures":"textures/blocks/ice"},
                "stone":{"textures":"textures/blocks/stone"}
            }
        }"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        for (name, colour) in [
            ("still", [90, 90, 90, 120]),
            ("flow", [100, 100, 100, 120]),
            ("ice", [120, 140, 180, 120]),
            ("stone", [100, 100, 100, 255]),
        ] {
            RgbaImage::from_pixel(16, 16, Rgba(colour))
                .save(directory.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let light = vec![assets::LightProperties::default(); records.len()];
        let compiled = compile_pack(directory.path(), &records, &light).unwrap();
        Fixture {
            assets: RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap(),
            records,
        }
    })
}

fn top_at(mesh: &meshing::ChunkMesh, block: [u8; 3]) -> PackedLiquidQuad {
    *mesh
        .liquid_quads()
        .iter()
        .find(|quad| quad.origin() == block && quad.face() == Face::PositiveY)
        .unwrap()
}

/// Ice over water must not turn a calm source into a downhill stream or slope
/// its top. Classic water hides the side touching ice but keeps sides facing air.
#[test]
fn compiled_ice_over_water_preserves_still_source_and_flat_top() {
    let fixture = fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let water = fixture.id("minecraft:water", mode);
        let ice = fixture.id("minecraft:ice", mode);
        let mut placements = Vec::new();
        for x in 7..=9 {
            for z in 7..=9 {
                placements.push((if [x, z] == [9, 8] { ice } else { water }, [x, 8, z]));
            }
        }
        placements.push((water, [9, 7, 8]));
        let chunk = fixture.chunk(mode, &placements);
        let neighbourhood = MeshNeighbourhood::new(&chunk);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(fixture.id("minecraft:air", mode)),
            &fixture.assets,
            mode,
            &neighbourhood,
        );
        let top = top_at(&mesh, [8, 8, 8]);
        assert_eq!(
            top.flow_gradient(),
            [0, 0],
            "ice must not create false downhill flow"
        );
        assert_eq!(
            top.heights(),
            [meshing::LiquidLevel::from_variant(0).unwrap().height(); 4]
        );
        let source = fixture.assets.resolve(mode, water);
        assert_eq!(
            top.material_id(),
            source.face(assets::BlockFace::Up).material_id()
        );
        assert!(
            !mesh
                .liquid_quads()
                .iter()
                .any(|quad| { quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveX }),
            "classic water must omit the side touching primary ice"
        );
        assert!(
            mesh.liquid_quads()
                .iter()
                .any(|quad| { quad.origin() == [9, 8, 7] && quad.face() == Face::PositiveX }),
            "classic water must retain sides facing primary air"
        );
        assert_eq!(mesh.model_refs().len(), 1, "ice remains a real cube model");
        assert_eq!(mesh.transparent_model_draw_refs().len(), 6);
        assert!(
            mesh.model_draw_refs().is_empty(),
            "ice must never draw opaque"
        );
    }
}

#[test]
fn transparent_ice_cube_cannot_erase_its_own_underwater_faces() {
    let fixture = fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let ice = fixture.id("minecraft:ice", mode);
        let water = fixture.id("minecraft:water", mode);
        let chunk = fixture.chunk(mode, &[(ice, [8, 8, 8]), (water, [8, 7, 8])]);
        let neighbourhood = MeshNeighbourhood::new(&chunk);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(fixture.id("minecraft:air", mode)),
            &fixture.assets,
            mode,
            &neighbourhood,
        );
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(mesh.model_refs()[0].words()[3], 0b11_1111);
        assert_eq!(mesh.transparent_model_draw_refs().len(), 6);
        assert!(
            mesh.model_draw_refs().is_empty(),
            "ice must never draw opaque"
        );
        let water_top = top_at(&mesh, [8, 7, 8]);
        assert!(water_top.heights().iter().all(|&height| height < u8::MAX));
    }
}
