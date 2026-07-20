use super::support::*;

fn generated_sign_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Sign)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 120_000 + id as u32;
    }
    records
}

fn write_sign_pack(root: &Path, records: &[RegistryRecord]) {
    let names = records
        .iter()
        .map(|record| record.name.strip_prefix("minecraft:").unwrap())
        .collect::<HashSet<_>>();
    let mut names = names.into_iter().collect::<Vec<_>>();
    names.sort_unstable();
    let mut blocks = String::from("{");
    for (index, name) in names.iter().enumerate() {
        if index != 0 {
            blocks.push(',');
        }
        write!(&mut blocks, r#""{name}":{{"textures":"sign_texture"}}"#).unwrap();
    }
    blocks.push('}');
    write_pack(
        root,
        &blocks,
        r#"{"texture_data":{"sign_texture":{"textures":"textures/blocks/sign_texture"}}}"#,
        "[]",
    );
    write_png(
        root,
        "textures/blocks/sign_texture",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [137, 98, 55, 255]),
    );
}

#[test]
fn generated_sign_registry_has_exact_mode_dependent_selector_matrix() {
    let records = generated_sign_records();
    assert_eq!(records.len(), 4_872);
    let standing = records
        .iter()
        .filter(|record| record.name.ends_with("standing_sign"))
        .collect::<Vec<_>>();
    let wall = records
        .iter()
        .filter(|record| record.name.ends_with("wall_sign"))
        .collect::<Vec<_>>();
    let hanging = records
        .iter()
        .filter(|record| record.name.ends_with("hanging_sign"))
        .collect::<Vec<_>>();
    assert_eq!(
        (standing.len(), wall.len(), hanging.len()),
        (192, 72, 4_608)
    );
    assert_eq!(
        standing
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        12
    );
    assert_eq!(
        wall.iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        12
    );
    assert_eq!(
        hanging
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        12
    );
    assert!(standing.iter().all(|record| {
        record.model_state.mask() == 0x01
            && record
                .model_state
                .get(ModelStateField::Orientation)
                .is_some_and(|rotation| rotation <= 15)
    }));
    assert!(wall.iter().all(|record| {
        record.model_state.mask() == 0x01
            && record
                .model_state
                .get(ModelStateField::Orientation)
                .is_some_and(|facing| facing <= 5)
    }));
    assert!(hanging.iter().all(|record| {
        let orientation = record.model_state.get(ModelStateField::Orientation);
        let flags = record.model_state.get(ModelStateField::Flags);
        record.model_state.mask() == 0x81
            && orientation.is_some_and(|selector| selector >> 4 <= 5)
            && flags.is_some_and(|flags| flags & !(MODEL_FLAG_ATTACHED | MODEL_FLAG_HANGING) == 0)
    }));
    for name in hanging
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<HashSet<_>>()
    {
        let selectors = hanging
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .map(|record| {
                (
                    record
                        .model_state
                        .get(ModelStateField::Orientation)
                        .unwrap(),
                    record.model_state.get(ModelStateField::Flags).unwrap(),
                )
            })
            .collect::<HashSet<_>>();
        assert_eq!(selectors.len(), 384, "{name}");
    }
}

#[test]
fn compiler_routes_all_sign_states_to_reviewed_blank_static_models() {
    let directory = tempfile::tempdir().expect("create sign fixture");
    let records = generated_sign_records();
    write_sign_pack(directory.path(), &records);
    let compiled = compile_pack(directory.path(), &records).expect("compile all sign states");

    assert!(compiled.visuals.iter().all(|visual| {
        visual.kind == VisualKind::Model
            && visual.model_template != assets::NO_MODEL_TEMPLATE
            && visual
                .faces
                .iter()
                .all(|&material| material != DIAGNOSTIC_MATERIAL)
    }));
    for (record, visual) in records.iter().zip(compiled.visuals.iter()) {
        let template = compiled.model_templates[visual.model_template as usize];
        assert!(
            (1..=32).contains(&template.quad_count),
            "{}",
            record.canonical_state
        );
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert!(quads.iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL
                && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
                && quad.flags & MODEL_QUAD_FLAG_TWO_SIDED == 0
        }));
    }

    let oak_standing = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:standing_sign")
        .map(|record| compiled.visuals[record.sequential_id as usize].model_template)
        .collect::<HashSet<_>>();
    assert_eq!(
        oak_standing.len(),
        16,
        "all standing rotations must be visible"
    );
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:standing_sign")
    {
        let rotation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let visual = compiled.visuals[record.sequential_id as usize];
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let directional_quads = quads
            .iter()
            .filter(|quad| quad.flags & MODEL_QUAD_FLAG_FACE_MASK != 0)
            .count();
        assert_eq!(
            directional_quads,
            if rotation.is_multiple_of(4) { 12 } else { 4 },
            "rotation {rotation} must retain exact axis-aligned lighting faces"
        );
    }
    let template_quads = |record: &RegistryRecord| {
        let visual = compiled.visuals[record.sequential_id as usize];
        let template = compiled.model_templates[visual.model_template as usize];
        &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
    };
    let standing_zero = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:standing_sign"
                && record.model_state.get(ModelStateField::Orientation) == Some(0)
        })
        .unwrap();
    let standing_front = template_quads(standing_zero)[5];
    let (board_min, board_max) = model_bounds(&template_quads(standing_zero)[..6]);
    assert_eq!(
        [board_max[0] - board_min[0], board_max[1] - board_min[1]],
        [256, 128],
        "24x12 SignModel pixels scaled by the vanilla 2/3 render pose must occupy a 16x8-pixel world silhouette"
    );
    assert_eq!(
        standing_front.positions,
        [
            [0, 112, 136],
            [256, 112, 136],
            [256, 240, 136],
            [0, 240, 136],
        ]
    );
    assert_eq!(
        standing_front.uvs,
        [[0, 2_304], [4_096, 2_304], [4_096, 256], [0, 256]],
        "standing board intentionally projects its pinned terrain tile over the full front"
    );
    let oak_wall = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:wall_sign")
        .map(|record| compiled.visuals[record.sequential_id as usize].model_template)
        .collect::<HashSet<_>>();
    assert_eq!(oak_wall.len(), 6, "all wall facings must be visible");
    let expected_wall_bounds = [
        ([0, 240, 0], [256, 256, 256]),
        ([0, 0, 0], [256, 16, 256]),
        ([0, 72, 240], [256, 200, 256]),
        ([0, 72, 0], [256, 200, 16]),
        ([240, 72, 0], [256, 200, 256]),
        ([0, 72, 0], [16, 200, 256]),
    ];
    for (facing, expected) in expected_wall_bounds.into_iter().enumerate() {
        let record = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:wall_sign"
                    && record.model_state.get(ModelStateField::Orientation) == Some(facing as u32)
            })
            .unwrap();
        assert_eq!(
            model_bounds(template_quads(record)),
            expected,
            "wall-sign facing {facing} must place the board opposite its supporting side"
        );
    }

    let expected_hanging_wall_support_bounds = [
        ([96, 128, 176], [160, 256, 256]),
        ([96, 0, 0], [160, 128, 80]),
        ([96, 224, 128], [160, 256, 256]),
        ([96, 224, 0], [160, 256, 128]),
        ([128, 224, 96], [256, 256, 160]),
        ([0, 224, 96], [128, 256, 160]),
    ];
    for (facing, expected_support) in expected_hanging_wall_support_bounds.into_iter().enumerate() {
        let record = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:oak_hanging_sign"
                    && record.model_state.get(ModelStateField::Orientation)
                        == Some((facing as u32) << 4)
                    && record.model_state.get(ModelStateField::Flags) == Some(0)
            })
            .unwrap();
        let quads = template_quads(record);
        assert_eq!(
            model_bounds(&quads[6..12]),
            expected_support,
            "wall-hanging facing {facing} must extend its support opposite its front"
        );
    }

    let oak_hanging = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_hanging_sign")
        .collect::<Vec<_>>();
    for record in &oak_hanging {
        let selector = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let rotation = selector & 0x0f;
        let facing = selector >> 4;
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        let hanging = flags & MODEL_FLAG_HANGING != 0;
        let peer = oak_hanging
            .iter()
            .find(|peer| {
                let peer_selector = peer.model_state.get(ModelStateField::Orientation).unwrap();
                let same_flags = peer.model_state.get(ModelStateField::Flags) == Some(flags);
                same_flags
                    && if hanging {
                        peer_selector & 0x0f == rotation && peer_selector >> 4 == (facing + 1) % 6
                    } else {
                        peer_selector >> 4 == facing && peer_selector & 0x0f == (rotation + 1) % 16
                    }
            })
            .expect("inactive hanging-sign selector peer");
        assert_eq!(
            compiled.visuals[record.sequential_id as usize].model_template,
            compiled.visuals[peer.sequential_id as usize].model_template,
            "inactive selector changed {}",
            record.canonical_state
        );
    }
}

#[test]
fn compiler_sign_selectors_fail_closed_when_typed_state_is_missing_or_mismatched() {
    let directory = tempfile::tempdir().expect("create sign fail-closed fixture");
    let generated = generated_sign_records();
    let mut records = [
        generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:standing_sign")
            .unwrap()
            .clone(),
        generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:wall_sign")
            .unwrap()
            .clone(),
        generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:oak_hanging_sign")
            .unwrap()
            .clone(),
    ];
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 130_000 + id as u32;
        record.canonical_state = "{}".into();
    }
    write_sign_pack(directory.path(), &records);
    let compiled = compile_pack(directory.path(), &records).expect("fail closed on bad sign state");
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
fn compiler_real_pinned_pack_has_zero_diagnostic_sign_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let pack_sources = read_pack(Path::new(&pack)).expect("read requested pinned pack");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let signs = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Sign)
        .collect::<Vec<_>>();
    assert_eq!(signs.len(), 4_872);
    let expected_aliases = [
        ("acacia", "acacia_sign"),
        ("bamboo", "bamboo_sign"),
        ("birch", "birch_sign"),
        ("cherry", "cherry_planks"),
        ("crimson", "crimson_sign"),
        ("darkoak", "darkoak_sign"),
        ("jungle", "jungle_sign"),
        ("mangrove", "mangrove_sign"),
        ("oak", "sign"),
        ("pale_oak", "pale_oak_planks"),
        ("spruce", "spruce_sign"),
        ("warped", "warped_sign"),
    ];
    for record in &signs {
        let name = record.name.strip_prefix("minecraft:").unwrap();
        let wood = if matches!(name, "standing_sign" | "wall_sign" | "oak_hanging_sign") {
            "oak"
        } else if name.starts_with("dark_oak_") || name.starts_with("darkoak_") {
            "darkoak"
        } else {
            expected_aliases
                .iter()
                .map(|(wood, _)| *wood)
                .find(|wood| name.starts_with(&format!("{wood}_")))
                .unwrap_or_else(|| panic!("unreviewed sign wood family: {name}"))
        };
        let expected = expected_aliases
            .iter()
            .find_map(|(candidate, key)| (*candidate == wood).then_some(*key))
            .unwrap();
        let resolved = resolve_texture_key(&pack_sources.blocks, record, BlockFace::South);
        assert_eq!(
            resolved.key.as_deref(),
            Some(expected),
            "{name} must use its reviewed blocks.json terrain alias without a generic fallback"
        );
    }
    let diagnostic = signs
        .iter()
        .filter(|record| {
            compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
        })
        .map(|record| (record.name.as_ref(), record.canonical_state.as_ref()))
        .collect::<Vec<_>>();
    assert!(
        diagnostic.is_empty(),
        "pinned pack retained diagnostic signs: {diagnostic:?}"
    );
}
