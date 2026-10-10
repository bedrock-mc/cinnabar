use super::{
    cactus::write_cactus_pack, cake::write_cake_pack, farmland::*,
    inventory::write_selector_alias_cube_pack, mineral_cubes::write_mineral_pack,
    resin_clump::write_resin_clump_pack, special_cubes::*, support::*,
};

/// Records named `names` from the registry the bedrock target pins.
fn target_records(names: &[&str]) -> Vec<RegistryRecord> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("assets/bedrock-target.json")).expect("read target manifest"),
    )
    .expect("decode target manifest");
    let bytes = fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap()))
        .expect("read target registry");
    assets::read_registry_for_protocol(&bytes, target["wire_protocol"].as_u64().unwrap() as u32)
        .expect("decode target registry")
        .into_iter()
        .filter(|record| names.contains(&record.name.as_ref()))
        .collect()
}

fn target_farmland_records() -> Vec<RegistryRecord> {
    let mut records = target_records(&["minecraft:farmland"]);
    records.sort_unstable_by_key(|record| record.model_state.get(ModelStateField::Growth).unwrap());
    assert_eq!(records.len(), 8);
    records
}

fn assert_farmland_models(compiled: &CompiledAssets, records: &[RegistryRecord]) {
    let runtime = RuntimeAssets::decode(&encode_blob(compiled).expect("encode farmland"))
        .expect("decode farmland");
    let mut templates = HashSet::new();
    for record in records {
        let amount = record.model_state.get(ModelStateField::Growth).unwrap();
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "moisture {amount}");
        templates.insert(visual.model_template);
        let quads = template_quads(compiled, visual.model_template);
        assert_eq!(quads.len(), 6);
        assert_eq!(quads[BlockFace::Up as usize].positions[0][1], 240);
        let top = visual.faces[BlockFace::Up as usize];
        assert_ne!(top, DIAGNOSTIC_MATERIAL);
        assert_eq!(compiled.materials[top as usize].flags, 0);
        assert_eq!(
            top,
            compiled.visuals[records[usize::from(amount != 0)].sequential_id as usize].faces
                [BlockFace::Up as usize]
        );
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let resolved = runtime.resolve(mode, id);
            assert_eq!(
                resolved.kind(),
                VisualKind::Model,
                "{mode:?} moisture {amount}"
            );
            assert_eq!(resolved.model_template(), Some(visual.model_template));
        }
    }
    assert_eq!(templates.len(), 2);
}

#[test]
fn compiler_admits_farmland_from_target_registry() {
    let directory = tempfile::tempdir().expect("farmland fixture");
    write_farmland_pack(directory.path());
    let mut records = target_farmland_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile target farmland");
    assert_farmland_models(&compiled, &records);
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed farmland");
    assert_eq!(
        encode_blob(&compiled).unwrap(),
        encode_blob(&reversed).unwrap()
    );
}

#[test]
fn compiler_farmland_uses_registry_identity_without_a_numeric_formula() {
    let directory = tempfile::tempdir().expect("farmland fixture");
    write_farmland_pack(directory.path());
    let mut records = farmland_records();
    let count = records.len();
    for (index, record) in records.iter_mut().enumerate() {
        record.sequential_id = ((count - index) * 3) as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile reordered farmland");
    assert_farmland_models(&compiled, &records);
}

#[test]
fn compiler_farmland_rejects_duplicate_network_identities() {
    let directory = tempfile::tempdir().expect("farmland fixture");
    write_farmland_pack(directory.path());
    let records = target_farmland_records();
    let mut duplicate_id = records.clone();
    duplicate_id[0].sequential_id = duplicate_id[1].sequential_id;
    assert!(matches!(
        compile_pack(directory.path(), &duplicate_id),
        Err(AssetError::DuplicateSequentialId(_))
    ));
    let mut duplicate_hash = records;
    duplicate_hash[0].network_hash = duplicate_hash[1].network_hash;
    assert!(matches!(
        compile_pack(directory.path(), &duplicate_hash),
        Err(AssetError::DuplicateNetworkHash(_))
    ));
}

/// Block names and the check their compiled family must pass.
type Family = (&'static [&'static str], fn(&Path));

/// Exact block families must not depend on another registry version's sequential IDs.
#[test]
fn exact_families_compile_from_target_registry() {
    let families: [Family; 7] = [
        (
            &[
                "minecraft:bone_block",
                "minecraft:chiseled_quartz_block",
                "minecraft:hay_block",
                "minecraft:purpur_block",
                "minecraft:quartz_block",
                "minecraft:smooth_quartz",
                "minecraft:tnt",
            ],
            |root| {
                write_selector_alias_cube_pack(
                    root,
                    r#"{"down":"hayblock_top","side":"hayblock_side","up":"hayblock_top"}"#,
                );
            },
        ),
        (&["minecraft:cactus"], write_cactus_pack),
        (&["minecraft:cake"], write_cake_pack),
        (&["minecraft:resin_clump"], |root| {
            write_resin_clump_pack(root);
        }),
        (&["minecraft:chiseled_bookshelf"], |root| {
            write_chiseled_bookshelf_pack(root, None);
        }),
        (
            &["minecraft:bee_nest", "minecraft:beehive"],
            write_bee_housing_pack,
        ),
        (
            &["minecraft:cinnabar", "minecraft:sulfur"],
            write_mineral_pack,
        ),
    ];
    for (names, write) in families {
        let directory = tempfile::tempdir().expect("family fixture");
        write(directory.path());
        let records = target_records(names);
        assert!(!records.is_empty(), "{names:?} absent from target registry");
        let compiled = compile_pack(directory.path(), &records).expect("compile target family");
        let mut renumbered = records.clone();
        for record in &mut renumbered {
            record.collision_seed.shape_id ^= u16::MAX;
        }
        let same_geometry = compile_pack(directory.path(), &renumbered)
            .expect("compile family with carrier-local shape keys");
        assert_eq!(
            encode_blob(&compiled).unwrap(),
            encode_blob(&same_geometry).unwrap()
        );
        for record in &records {
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_ne!(
                visual.kind,
                VisualKind::Diagnostic,
                "{} {}",
                record.name,
                record.canonical_state
            );
        }
    }
}
