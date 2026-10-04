use super::{farmland::*, support::*};

fn target_farmland_records() -> Vec<RegistryRecord> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("assets/bedrock-target.json")).expect("read target manifest"),
    )
    .expect("decode target manifest");
    let bytes = fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap()))
        .expect("read target registry");
    let mut records = assets::read_registry_for_protocol(
        &bytes,
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .expect("decode target registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:farmland")
    .collect::<Vec<_>>();
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
