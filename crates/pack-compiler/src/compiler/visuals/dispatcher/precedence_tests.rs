use super::*;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use image::{Rgba, RgbaImage};
use std::fs;

/// The active registry and a pack drawing every block with one opaque tile.
fn active_registry_with_uniform_pack() -> (Box<[RegistryRecord]>, tempfile::TempDir) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .expect("active registry");
    let directory = tempfile::tempdir().expect("uniform pack");
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    let blocks = records
        .iter()
        .filter_map(|record| record.name.strip_prefix("minecraft:"))
        .map(|name| (name, serde_json::json!({"textures": "tile"})))
        .collect::<BTreeMap<_, _>>();
    fs::write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"tile":{"textures":"textures/blocks/tile"}}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([90, 90, 90, 255]),
    )
    .save(directory.path().join("textures/blocks/tile.png"))
    .unwrap();
    (records, directory)
}

/// A record a dedicated rule claims never takes the envelope's box or material flags.
#[test]
fn dedicated_rules_supersede_the_provisional_envelope() {
    let (records, directory) = active_registry_with_uniform_pack();
    let pack = read_pack(directory.path()).expect("read uniform pack");
    let fallback = super::super::fallback::inventory(assets::active_content_registry_protocol())
        .expect("active fallback inventory");
    let material_by_descriptor = records
        .iter()
        .flat_map(|record| BlockFace::ALL.map(|face| descriptor_for(fallback, &pack, record, face)))
        .flatten()
        .map(|(descriptor, _)| (descriptor, 1))
        .collect::<BTreeMap<_, _>>();
    let inputs = RuleInputs {
        pack: &pack,
        material_by_descriptor: &material_by_descriptor,
        vanilla_fallback_material: 1,
        fallback,
    };
    let canonical_air = resolve_canonical_air(&records).expect("canonical air");
    let admissions = ExactAdmissions {
        mineral_cubes: true,
        chiseled_bookshelves: true,
        resin_clumps: true,
        selector_alias_cubes: true,
        cacti: true,
        cakes: true,
        farmland: true,
        bee_housing: true,
    };
    // Rules that draw from the envelope's own descriptors and box.
    let envelope_readers = [R::Anvil, R::Bamboo];
    let mut compiler = VisualCompiler::default();
    let mut conflicts = BTreeSet::new();
    let mut readers_seen = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| fallback.entry_unfiltered(record).is_some())
    {
        let kind = block_render_fact(&record.name).kind;
        let claimed = compiler
            .compile_dedicated(record, &inputs, admissions, canonical_air)
            .expect("compile dedicated rules")
            != CompileRuleResult::NoMatch;
        let enveloped = fallback.entry(record).is_some();
        if claimed && enveloped && envelope_readers.contains(&kind) {
            readers_seen.insert(format!("{kind:?}"));
        } else if claimed == enveloped {
            conflicts.insert(format!("{kind:?} {} claimed={claimed}", record.name));
        }
    }
    assert!(conflicts.is_empty(), "{conflicts:#?}");
    assert_eq!(
        readers_seen,
        envelope_readers.map(|kind| format!("{kind:?}")).into(),
        "every listed envelope reader still reads the envelope"
    );
}

/// Rule outputs that replaced envelope boxes must still satisfy the carrier's template contracts.
#[test]
fn active_registry_encodes_with_dedicated_rules_ahead_of_the_envelope() {
    let (records, directory) = active_registry_with_uniform_pack();
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        directory.path(),
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        assets::active_content_registry_protocol(),
    )
    .expect("compile active registry");
    assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).expect("encode"))
        .expect("decode");
}
