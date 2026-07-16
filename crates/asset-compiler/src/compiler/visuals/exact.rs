use super::super::*;
use super::context::{CuboidTemplateKey, intern_cuboid_template, push_model_template};

fn exact_family_admission(
    record: &RegistryRecord,
    admissions: ExactAdmissions,
) -> CompileRuleResult {
    let rejected = (is_mineral_cube_name(&record.name)
        && (!admissions.mineral_cubes || !is_mineral_cube_record(record)))
        || (is_bee_housing_name(&record.name)
            && (!admissions.bee_housing || !is_bee_housing_record(record)))
        || (is_cactus_name(&record.name) && (!admissions.cacti || !is_cactus_record(record)))
        || (is_cake_name(&record.name) && (!admissions.cakes || !is_cake_record(record)))
        || (is_farmland_name(&record.name)
            && (!admissions.farmland || !is_farmland_record(record)))
        || (is_selector_alias_cube_name(&record.name)
            && (!admissions.selector_alias_cubes || !is_selector_alias_cube_record(record)))
        || (is_resin_clump_name(&record.name)
            && (!admissions.resin_clumps || !is_resin_clump_record(record)))
        || (is_chiseled_bookshelf_name(&record.name)
            && (!admissions.chiseled_bookshelves || !is_chiseled_bookshelf_record(record)));
    if rejected {
        CompileRuleResult::Reject
    } else {
        CompileRuleResult::NoMatch
    }
}

pub(in crate::compiler) struct ExactRuleContext<'a> {
    pub(in crate::compiler) pack: &'a PackSources,
    pub(in crate::compiler) material_by_descriptor: &'a BTreeMap<Descriptor, u32>,
    pub(in crate::compiler) admissions: ExactAdmissions,
    pub(in crate::compiler) visual: &'a mut BlockVisual,
    pub(in crate::compiler) cuboid_template_by_key: &'a mut BTreeMap<CuboidTemplateKey, u32>,
    pub(in crate::compiler) chiseled_bookshelf_template_by_key: &'a mut BTreeMap<[u32; 5], u32>,
    pub(in crate::compiler) model_templates: &'a mut Vec<ModelTemplate>,
    pub(in crate::compiler) model_quads: &'a mut Vec<ModelQuad>,
}

pub(in crate::compiler) fn compile_exact_families(
    record: &RegistryRecord,
    context: &mut ExactRuleContext<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let ExactRuleContext {
        pack,
        material_by_descriptor,
        admissions,
        visual,
        cuboid_template_by_key,
        chiseled_bookshelf_template_by_key,
        model_templates,
        model_quads,
    } = context;
    let admissions = *admissions;
    let ExactAdmissions {
        mineral_cubes: admit_mineral_cubes,
        chiseled_bookshelves: admit_chiseled_bookshelves,
        resin_clumps: admit_resin_clumps,
        selector_alias_cubes: admit_selector_alias_cubes,
        cacti: admit_cacti,
        cakes: admit_cakes,
        farmland: admit_farmland,
        bee_housing: admit_bee_housing,
        ..
    } = admissions;
    if matches!(
        exact_family_admission(record, admissions),
        CompileRuleResult::Reject
    ) {
        return Ok(CompileRuleResult::Reject);
    }
    if is_mineral_cube_name(&record.name)
        && (!admit_mineral_cubes || !is_mineral_cube_record(record))
    {
        // The two protocol-gap mineral states are admitted atomically and may
        // not inherit a generic cube route from malformed source metadata.
    } else if admit_mineral_cubes && is_mineral_cube_record(record) {
        let material = mineral_cube_material_descriptor(pack, record)
            .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied());
        if let Some(material) = material {
            visual.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
            visual.faces = [material; 6];
            visual.kind = VisualKind::Cube;
        }
    } else if is_bee_housing_name(&record.name)
        && (!admit_bee_housing || !is_bee_housing_record(record))
    {
        // Both exact 24-state families are admitted atomically and may not
        // fall through to the generic cube/terrain route.
    } else if admit_bee_housing && is_bee_housing_record(record) {
        let materials = bee_housing_material_descriptors(pack).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let Some(
            [
                Some(nest_bottom),
                Some(nest_front),
                Some(nest_front_honey),
                Some(nest_side),
                Some(nest_top),
                Some(hive_front),
                Some(hive_front_honey),
                Some(hive_side),
                Some(hive_top),
            ],
        ) = materials
        {
            let (direction, honey_level) =
                exact_bee_housing_state(record).expect("exact bee housing state");
            let front_face = [
                BlockFace::South,
                BlockFace::West,
                BlockFace::North,
                BlockFace::East,
            ][direction as usize] as usize;
            let mut faces = if record.name.as_ref() == "minecraft:bee_nest" {
                [
                    nest_side,
                    nest_side,
                    nest_bottom,
                    nest_top,
                    nest_side,
                    nest_side,
                ]
            } else {
                [
                    hive_side, hive_side, hive_top, hive_top, hive_side, hive_side,
                ]
            };
            faces[front_face] = match (record.name.as_ref(), honey_level == 5) {
                ("minecraft:bee_nest", false) => nest_front,
                ("minecraft:bee_nest", true) => nest_front_honey,
                ("minecraft:beehive", false) => hive_front,
                ("minecraft:beehive", true) => hive_front_honey,
                _ => unreachable!("exact bee housing name"),
            };
            visual.faces = faces;
            visual.kind = VisualKind::Cube;
        }
    } else if is_cactus_name(&record.name) && (!admit_cacti || !is_cactus_record(record)) {
        // This exact family is all-or-nothing and malformed states may not
        // fall through to any generic age, crop, cube, or cuboid route.
    } else if admit_cacti && is_cactus_record(record) {
        let materials = cactus_material_descriptors(pack).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let Some([Some(side), Some(bottom), Some(top)]) = materials {
            let faces = [side, side, bottom, top, side, side];
            let template = intern_cuboid_template(
                faces,
                [16, 0, 16],
                [240, 256, 240],
                cuboid_template_by_key,
                model_templates,
                model_quads,
            )?;
            visual.flags.remove(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL,
            );
            visual.faces = faces;
            visual.kind = VisualKind::Model;
            visual.model_template = template;
        }
    } else if is_cake_name(&record.name) && (!admit_cakes || !is_cake_record(record)) {
        // The exact seven-state family is admitted atomically and may not
        // inherit a generic cuboid or descriptor-alias visual.
    } else if admit_cakes && is_cake_record(record) {
        let materials = cake_material_descriptors(pack).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let Some([Some(side), Some(bottom), Some(top), Some(inner)]) = materials {
            let bite = exact_cake_bite(record).expect("exact cake record has a bite");
            let faces = [
                if bite == 0 { side } else { inner },
                side,
                bottom,
                top,
                side,
                side,
            ];
            let template = intern_cuboid_template(
                faces,
                [16 + 32 * bite as i16, 0, 16],
                [240, 128, 240],
                cuboid_template_by_key,
                model_templates,
                model_quads,
            )?;
            visual.flags.remove(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL,
            );
            visual.faces = faces;
            visual.kind = VisualKind::Model;
            visual.model_template = template;
            visual.variant = 0;
        }
    } else if is_farmland_name(&record.name) && (!admit_farmland || !is_farmland_record(record)) {
        // The exact moisture product is atomic and must never fall through
        // to generic cuboid or terrain-array selection.
    } else if admit_farmland && is_farmland_record(record) {
        let materials = farmland_material_descriptors(pack).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let Some([Some(side), Some(wet), Some(dry)]) = materials {
            let amount =
                exact_farmland_moisture(record).expect("exact farmland record has moisture");
            let top = if amount == 0 { dry } else { wet };
            let faces = [side, side, side, top, side, side];
            let template = intern_cuboid_template(
                faces,
                [0, 0, 0],
                [256, 240, 256],
                cuboid_template_by_key,
                model_templates,
                model_quads,
            )?;
            visual.flags.remove(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL,
            );
            visual.faces = faces;
            visual.kind = VisualKind::Model;
            visual.model_template = template;
            visual.variant = 0;
        }
    } else if is_selector_alias_cube_name(&record.name)
        && (!admit_selector_alias_cubes || !is_selector_alias_cube_record(record))
    {
        // The seven reviewed compatibility products are admitted only as
        // one complete exact registry inventory.
    } else if admit_selector_alias_cubes && is_selector_alias_cube_record(record) {
        let materials = selector_alias_cube_material_descriptors(pack, record).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let Some(
            [
                Some(west),
                Some(east),
                Some(down),
                Some(up),
                Some(north),
                Some(south),
            ],
        ) = materials
        {
            visual.faces = [west, east, down, up, north, south];
            visual.kind = VisualKind::Cube;
        }
    } else if is_resin_clump_name(&record.name)
        && (!admit_resin_clumps || !is_resin_clump_record(record))
    {
        // This exact family is all-or-nothing and may not fall through to
        // generic multi-face handling.
    } else if is_chiseled_bookshelf_name(&record.name)
        && (!admit_chiseled_bookshelves || !is_chiseled_bookshelf_record(record))
    {
        // This exact family is all-or-nothing so malformed or incomplete
        // selector products cannot fall through to generic cube handling.
    } else if admit_chiseled_bookshelves && is_chiseled_bookshelf_record(record) {
        let materials = chiseled_bookshelf_material_descriptors(pack).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        let state = exact_chiseled_bookshelf_state(record);
        if let Some([Some(empty), Some(occupied), Some(side), Some(top)]) = materials
            && let Some((books, direction)) = state
        {
            let key = [empty, occupied, side, top, books];
            let template = if let Some(&template) = chiseled_bookshelf_template_by_key.get(&key) {
                template
            } else {
                let template = push_model_template(
                    chiseled_bookshelf_quads(empty, occupied, side, top, books),
                    0,
                    model_templates,
                    model_quads,
                )?;
                chiseled_bookshelf_template_by_key.insert(key, template);
                template
            };
            visual
                .flags
                .remove(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL);
            visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
            visual.faces = [side, side, top, top, empty, side];
            visual.kind = VisualKind::Model;
            visual.model_template = template;
            visual.variant = (direction + 2) & 3;
        }
    } else {
        return Ok(CompileRuleResult::NoMatch);
    }
    Ok(CompileRuleResult::Compiled(**visual))
}
