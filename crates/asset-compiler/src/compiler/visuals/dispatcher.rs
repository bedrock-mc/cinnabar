use super::super::*;
use super::context::{
    ButtonTemplateKey, CarpetState, CuboidTemplateKey, GateTemplateKey, PaleMossCarpetTemplateKey,
    PressurePlateTemplateKey, SignState, SignTemplateKey, intern_cuboid_template,
    push_model_template,
};

#[derive(Clone, Copy)]
pub(in crate::compiler) struct ExactAdmissions {
    pub(in crate::compiler) chiseled_bookshelves: bool,
    pub(in crate::compiler) resin_clumps: bool,
    pub(in crate::compiler) selector_alias_cubes: bool,
    pub(in crate::compiler) cacti: bool,
    pub(in crate::compiler) cakes: bool,
    pub(in crate::compiler) farmland: bool,
    pub(in crate::compiler) bee_housing: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) enum CompileRuleResult {
    NoMatch,
    Reject,
    Compiled(BlockVisual),
}

pub(in crate::compiler) fn exact_family_admission(
    record: &RegistryRecord,
    admissions: ExactAdmissions,
) -> CompileRuleResult {
    let rejected = (is_bee_housing_name(&record.name)
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

pub(in crate::compiler) fn compile_visuals(
    records: &[RegistryRecord],
    pack: &PackSources,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
    admissions: ExactAdmissions,
) -> Result<CompiledVisuals, AssetError> {
    let visual_count = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap_or(0);
    let mut visuals =
        vec![BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary); visual_count];
    let mut hashed = Vec::with_capacity(records.len());
    let mut model_templates = Vec::new();
    let mut model_quads = Vec::new();
    let mut template_by_material = BTreeMap::<[u32; 2], u32>::new();
    let mut kelp_template_by_material = BTreeMap::<[u32; 6], u32>::new();
    let mut transparent_cube_template_by_material = BTreeMap::<[u32; 6], u32>::new();
    let mut flowerbed_template_by_key = BTreeMap::<[u32; 4], u32>::new();
    let mut slab_template_by_key = BTreeMap::<[u32; 7], u32>::new();
    let mut stair_template_by_key = BTreeMap::<[u32; 7], u32>::new();
    let mut vine_template_by_key = BTreeMap::<[u32; 2], u32>::new();
    let mut multiface_template_by_key = BTreeMap::<[u32; 3], u32>::new();
    let mut cuboid_template_by_key = BTreeMap::<CuboidTemplateKey, u32>::new();
    let mut wall_template_by_key = BTreeMap::<[u32; 7], u32>::new();
    let mut pressure_plate_template_by_key = BTreeMap::<PressurePlateTemplateKey, u32>::new();
    let mut button_template_by_key = BTreeMap::<ButtonTemplateKey, u32>::new();
    let mut pale_moss_carpet_template_by_key = BTreeMap::<PaleMossCarpetTemplateKey, u32>::new();
    let mut gate_template_by_key = BTreeMap::<GateTemplateKey, u32>::new();
    let mut pane_template_by_key = BTreeMap::<[u32; 2], u32>::new();
    let mut fence_template_by_key = BTreeMap::<[u32; 2], u32>::new();
    let mut sign_template_by_key = BTreeMap::<SignTemplateKey, u32>::new();
    let mut chiseled_bookshelf_template_by_key = BTreeMap::<[u32; 5], u32>::new();

    let mut ordered_records = records.iter().collect::<Vec<_>>();
    ordered_records.sort_unstable_by_key(|record| record.sequential_id);
    for record in ordered_records {
        let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
        let rule_result = match super::exact::compile_exact_families(
            record,
            &mut super::exact::ExactRuleContext {
                pack,
                material_by_descriptor,
                admissions,
                visual: &mut visual,
                cuboid_template_by_key: &mut cuboid_template_by_key,
                chiseled_bookshelf_template_by_key: &mut chiseled_bookshelf_template_by_key,
                model_templates: &mut model_templates,
                model_quads: &mut model_quads,
            },
        )? {
            CompileRuleResult::NoMatch => {
                match super::surfaces::compile_surface_rule(
                    record,
                    &mut super::surfaces::SurfaceRuleContext {
                        pack,
                        material_by_descriptor,
                        visual: &mut visual,
                        transparent_cube_template_by_material:
                            &mut transparent_cube_template_by_material,
                        flowerbed_template_by_key: &mut flowerbed_template_by_key,
                        vine_template_by_key: &mut vine_template_by_key,
                        multiface_template_by_key: &mut multiface_template_by_key,
                        model_templates: &mut model_templates,
                        model_quads: &mut model_quads,
                    },
                )? {
                    CompileRuleResult::NoMatch => {
                        if is_sign(record) {
                            let material = descriptor_for(pack, record, BlockFace::South).and_then(
                                |(descriptor, _)| material_by_descriptor.get(&descriptor).copied(),
                            );
                            if let (Some(material), Some(state)) = (material, sign_state(record)) {
                                let key = match state {
                                    SignState::Standing { rotation } => {
                                        SignTemplateKey::Standing { material, rotation }
                                    }
                                    SignState::Wall { facing } => {
                                        SignTemplateKey::Wall { material, facing }
                                    }
                                    SignState::HangingWall { facing } => {
                                        SignTemplateKey::HangingWall { material, facing }
                                    }
                                    SignState::HangingCeiling { rotation, attached } => {
                                        SignTemplateKey::HangingCeiling {
                                            material,
                                            rotation,
                                            attached,
                                        }
                                    }
                                };
                                let template =
                                    if let Some(&template) = sign_template_by_key.get(&key) {
                                        template
                                    } else {
                                        let quads = sign_quads(material, state);
                                        let template = push_model_template(
                                            quads,
                                            0,
                                            &mut model_templates,
                                            &mut model_quads,
                                        )?;
                                        sign_template_by_key.insert(key, template);
                                        template
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = [material; 6];
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_door(record) {
                            const UPPER: u32 = 1 << 7;
                            let orientation = record.model_state.get(ModelStateField::Orientation);
                            let open = record.model_state.get(ModelStateField::Open);
                            let hinge = record.model_state.get(ModelStateField::Hinge);
                            let flags = record.model_state.get(ModelStateField::Flags);
                            if let (
                                Some(orientation @ 0..=3),
                                Some(open @ 0..=1),
                                Some(hinge @ 0..=1),
                                Some(flags),
                            ) = (orientation, open, hinge, flags)
                                && flags & !UPPER == 0
                            {
                                let texture_face = if flags & UPPER == 0 {
                                    BlockFace::Down
                                } else {
                                    BlockFace::South
                                };
                                let material = descriptor_for(pack, record, texture_face).and_then(
                                    |(descriptor, _)| {
                                        material_by_descriptor.get(&descriptor).copied()
                                    },
                                );
                                if let Some(material) = material {
                                    let materials = [material; 6];
                                    let (min, max) = door_bounds(orientation, open, hinge);
                                    let template = intern_cuboid_template(
                                        materials,
                                        min,
                                        max,
                                        &mut cuboid_template_by_key,
                                        &mut model_templates,
                                        &mut model_quads,
                                    )?;
                                    visual.flags.remove(
                                        BlockFlags::AIR
                                            | BlockFlags::CUBE_GEOMETRY
                                            | BlockFlags::OCCLUDES_FULL_FACE
                                            | BlockFlags::LEAF_MODEL,
                                    );
                                    visual.faces = materials;
                                    visual.kind = VisualKind::Model;
                                    visual.model_template = template;
                                }
                            }
                        } else if is_trapdoor(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            let orientation = record.model_state.get(ModelStateField::Orientation);
                            let open = record.model_state.get(ModelStateField::Open);
                            let half = record.model_state.get(ModelStateField::Half);
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let (
                                    Some(orientation @ 0..=3),
                                    Some(open @ 0..=1),
                                    Some(half @ 0..=1),
                                ) = (orientation, open, half)
                            {
                                let materials = [west, east, down, up, north, south];
                                let (min, max) = trapdoor_bounds(orientation, open, half);
                                let template = intern_cuboid_template(
                                    materials,
                                    min,
                                    max,
                                    &mut cuboid_template_by_key,
                                    &mut model_templates,
                                    &mut model_quads,
                                )?;
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_pane(record) {
                            let body = descriptor_for(pack, record, BlockFace::North).and_then(
                                |(descriptor, _)| material_by_descriptor.get(&descriptor).copied(),
                            );
                            let edge = descriptor_for(pack, record, BlockFace::East).and_then(
                                |(descriptor, _)| material_by_descriptor.get(&descriptor).copied(),
                            );
                            if let (Some(body), Some(edge)) = (body, edge) {
                                let key = [body, edge];
                                let base = if let Some(&base) = pane_template_by_key.get(&key) {
                                    base
                                } else {
                                    let base =
                                        u32::try_from(model_templates.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            }
                                        })?;
                                    for mask in 0..16 {
                                        let quads = pane_quads(body, edge, mask);
                                        push_model_template(
                                            quads,
                                            MODEL_TEMPLATE_FLAG_PANE,
                                            &mut model_templates,
                                            &mut model_quads,
                                        )?;
                                    }
                                    pane_template_by_key.insert(key, base);
                                    base
                                };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = [body, body, edge, edge, body, body];
                                visual.kind = VisualKind::Model;
                                visual.model_template = base;
                            }
                        } else if is_fence(record) {
                            let material = descriptor_for(pack, record, BlockFace::South).and_then(
                                |(descriptor, _)| material_by_descriptor.get(&descriptor).copied(),
                            );
                            if let Some(material) = material {
                                let flag = if record.name.as_ref() == "minecraft:nether_brick_fence"
                                {
                                    MODEL_TEMPLATE_FLAG_FENCE_NETHER
                                } else {
                                    MODEL_TEMPLATE_FLAG_FENCE_WOOD
                                };
                                let key = [material, flag];
                                let base = if let Some(&base) = fence_template_by_key.get(&key) {
                                    base
                                } else {
                                    let base =
                                        u32::try_from(model_templates.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            }
                                        })?;
                                    push_model_template(
                                        cuboid_quads([material; 6], [96, 0, 96], [160, 256, 160])
                                            .to_vec(),
                                        flag,
                                        &mut model_templates,
                                        &mut model_quads,
                                    )?;
                                    for mask in 0..16 {
                                        push_model_template(
                                            fence_arm_quads(material, mask),
                                            flag,
                                            &mut model_templates,
                                            &mut model_quads,
                                        )?;
                                    }
                                    fence_template_by_key.insert(key, base);
                                    base
                                };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = [material; 6];
                                visual.kind = VisualKind::Model;
                                visual.model_template = base;
                            }
                        } else if is_wall(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            let connections = record.model_state.get(ModelStateField::Connections);
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some(connections) = connections
                                    .filter(|connections| wall_state_is_valid(*connections))
                            {
                                let materials = [west, east, down, up, north, south];
                                let key = [west, east, down, up, north, south, connections];
                                let template =
                                    if let Some(&template) = wall_template_by_key.get(&key) {
                                        template
                                    } else {
                                        let quads = wall_quads(materials, connections);
                                        let template = u32::try_from(model_templates.len())
                                            .map_err(|_| AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            })?;
                                        let quad_start =
                                            u32::try_from(model_quads.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                }
                                            })?;
                                        let quad_count =
                                            u32::try_from(quads.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model quad count",
                                                }
                                            })?;
                                        model_templates.push(ModelTemplate {
                                            quad_start,
                                            quad_count,
                                            flags: MODEL_TEMPLATE_FLAG_WALL,
                                        });
                                        model_quads.extend(quads);
                                        wall_template_by_key.insert(key, template);
                                        template
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_pressure_plate(record) {
                            const PRESSED: u32 = 1 << 1;
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            let flags = record.model_state.get(ModelStateField::Flags);
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some(flags @ (0 | PRESSED)) = flags
                            {
                                let materials = [west, east, down, up, north, south];
                                let pressed = flags == PRESSED;
                                let key = PressurePlateTemplateKey { materials, pressed };
                                let template = if let Some(&template) =
                                    pressure_plate_template_by_key.get(&key)
                                {
                                    template
                                } else {
                                    let template =
                                        u32::try_from(model_templates.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            }
                                        })?;
                                    let quad_start =
                                        u32::try_from(model_quads.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model quad",
                                            }
                                        })?;
                                    model_templates.push(ModelTemplate {
                                        quad_start,
                                        quad_count: 6,
                                        flags: 0,
                                    });
                                    model_quads.extend(pressure_plate_quads(materials, pressed));
                                    pressure_plate_template_by_key.insert(key, template);
                                    template
                                };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_button(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some((orientation, pressed)) = button_state(record)
                            {
                                let materials = [west, east, down, up, north, south];
                                let key = ButtonTemplateKey {
                                    materials,
                                    orientation,
                                    pressed,
                                };
                                let template =
                                    if let Some(&template) = button_template_by_key.get(&key) {
                                        template
                                    } else {
                                        let quads = button_quads(materials, orientation, pressed);
                                        let template = u32::try_from(model_templates.len())
                                            .map_err(|_| AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            })?;
                                        let quad_start =
                                            u32::try_from(model_quads.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                }
                                            })?;
                                        model_templates.push(ModelTemplate {
                                            quad_start,
                                            quad_count: 6,
                                            flags: 0,
                                        });
                                        model_quads.extend(quads);
                                        button_template_by_key.insert(key, template);
                                        template
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_carpet(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            let state = carpet_state(record);
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some(state) = state
                            {
                                let materials = [west, east, down, up, north, south];
                                let template = match state {
                                    CarpetState::Ordinary => intern_cuboid_template(
                                        materials,
                                        [0, 0, 0],
                                        [256, 16, 256],
                                        &mut cuboid_template_by_key,
                                        &mut model_templates,
                                        &mut model_quads,
                                    )?,
                                    CarpetState::Pale(state) => {
                                        let side_materials =
                                            pale_moss_carpet_side_material_descriptors(pack).map(
                                                |descriptors| {
                                                    descriptors.map(|(descriptor, _)| {
                                                        material_by_descriptor
                                                            .get(&descriptor)
                                                            .copied()
                                                    })
                                                },
                                            );
                                        let Some([Some(tall), Some(short)]) = side_materials else {
                                            visuals[record.sequential_id as usize] = visual;
                                            hashed
                                                .push((record.network_hash, record.sequential_id));
                                            continue;
                                        };
                                        let side_materials = [tall, short];
                                        let key = PaleMossCarpetTemplateKey {
                                            materials,
                                            side_materials,
                                            state,
                                        };
                                        if let Some(&template) =
                                            pale_moss_carpet_template_by_key.get(&key)
                                        {
                                            template
                                        } else {
                                            let quads = pale_moss_carpet_quads(
                                                materials,
                                                side_materials,
                                                state,
                                            );
                                            let template = u32::try_from(model_templates.len())
                                                .map_err(|_| AssetError::BlobSizeOverflow {
                                                    section: "model template",
                                                })?;
                                            let quad_start = u32::try_from(model_quads.len())
                                                .map_err(|_| AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                })?;
                                            let quad_count =
                                                u32::try_from(quads.len()).map_err(|_| {
                                                    AssetError::BlobSizeOverflow {
                                                        section: "model quad count",
                                                    }
                                                })?;
                                            model_templates.push(ModelTemplate {
                                                quad_start,
                                                quad_count,
                                                flags: 0,
                                            });
                                            model_quads.extend(quads);
                                            pale_moss_carpet_template_by_key.insert(key, template);
                                            template
                                        }
                                    }
                                };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_gate(record) {
                            const IN_WALL: u32 = 1 << 6;
                            const GATE_STATE_MASK: u8 = 0x85;
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            let orientation = record.model_state.get(ModelStateField::Orientation);
                            let open = record.model_state.get(ModelStateField::Open);
                            let flags = record.model_state.get(ModelStateField::Flags);
                            if record.model_state.mask() == GATE_STATE_MASK
                                && let [
                                    Some(west),
                                    Some(east),
                                    Some(down),
                                    Some(up),
                                    Some(north),
                                    Some(south),
                                ] = materials
                                && let (
                                    Some(orientation @ 0..=3),
                                    Some(open @ 0..=1),
                                    Some(flags @ (0 | IN_WALL)),
                                ) = (orientation, open, flags)
                            {
                                let materials = [west, east, down, up, north, south];
                                let key = GateTemplateKey {
                                    materials,
                                    orientation: orientation as u8,
                                    open: open != 0,
                                    in_wall: flags != 0,
                                    bamboo: record.name.as_ref() == "minecraft:bamboo_fence_gate",
                                };
                                let template =
                                    if let Some(&template) = gate_template_by_key.get(&key) {
                                        template
                                    } else {
                                        let [head, tail] = gate_quads(
                                            materials,
                                            orientation,
                                            open != 0,
                                            flags != 0,
                                            key.bamboo,
                                        );
                                        let template = u32::try_from(model_templates.len())
                                            .map_err(|_| AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            })?;
                                        let gate_axis = if orientation & 1 == 0 {
                                            MODEL_TEMPLATE_FLAG_GATE_AXIS_Z
                                        } else {
                                            MODEL_TEMPLATE_FLAG_GATE_AXIS_X
                                        };
                                        for (part, template_flags) in [
                                            (head, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | gate_axis),
                                            (tail, 0),
                                        ] {
                                            let quad_start = u32::try_from(model_quads.len())
                                                .map_err(|_| AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                })?;
                                            let quad_count =
                                                u32::try_from(part.len()).map_err(|_| {
                                                    AssetError::BlobSizeOverflow {
                                                        section: "model quad count",
                                                    }
                                                })?;
                                            debug_assert!(quad_count <= 32);
                                            model_templates.push(ModelTemplate {
                                                quad_start,
                                                quad_count,
                                                flags: template_flags,
                                            });
                                            model_quads.extend(part);
                                        }
                                        gate_template_by_key.insert(key, template);
                                        template
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = materials;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_slab(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some(half @ 0..=2) =
                                    record.model_state.get(ModelStateField::Half)
                            {
                                let faces = [west, east, down, up, north, south];
                                let key = [west, east, down, up, north, south, half];
                                let template =
                                    if let Some(&template) = slab_template_by_key.get(&key) {
                                        template
                                    } else {
                                        let template = u32::try_from(model_templates.len())
                                            .map_err(|_| AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            })?;
                                        let quad_start =
                                            u32::try_from(model_quads.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                }
                                            })?;
                                        model_templates.push(ModelTemplate {
                                            quad_start,
                                            quad_count: 6,
                                            flags: 0,
                                        });
                                        model_quads.extend(slab_quads(faces, half));
                                        slab_template_by_key.insert(key, template);
                                        template
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.flags.set(BlockFlags::OCCLUDES_FULL_FACE, half == 2);
                                visual.faces = faces;
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_stair(record) {
                            let materials = BlockFace::ALL.map(|face| {
                                descriptor_for(pack, record, face).and_then(|(descriptor, _)| {
                                    material_by_descriptor.get(&descriptor).copied()
                                })
                            });
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                                && let Some(orientation @ 0..=3) =
                                    record.model_state.get(ModelStateField::Orientation)
                                && let Some(upside @ 0..=1) =
                                    record.model_state.get(ModelStateField::Half)
                            {
                                let faces = [west, east, down, up, north, south];
                                let rotation = (orientation + 2) & 3;
                                let canonical_faces = canonical_stair_materials(faces, rotation);
                                let key = [
                                    canonical_faces[0],
                                    canonical_faces[1],
                                    canonical_faces[2],
                                    canonical_faces[3],
                                    canonical_faces[4],
                                    canonical_faces[5],
                                    upside,
                                ];
                                let base =
                                    if let Some(&base) = stair_template_by_key.get(&key) {
                                        base
                                    } else {
                                        let base =
                                            u32::try_from(model_templates.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model template",
                                                }
                                            })?;
                                        for shape in 0..5 {
                                            let quads =
                                                stair_quads(canonical_faces, 2, upside != 0, shape);
                                            let quad_start = u32::try_from(model_quads.len())
                                                .map_err(|_| AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                })?;
                                            let quad_count =
                                                u32::try_from(quads.len()).map_err(|_| {
                                                    AssetError::BlobSizeOverflow {
                                                        section: "model quad count",
                                                    }
                                                })?;
                                            model_templates.push(ModelTemplate {
                                                quad_start,
                                                quad_count,
                                                flags: MODEL_TEMPLATE_FLAG_STAIR,
                                            });
                                            model_quads.extend(quads);
                                        }
                                        stair_template_by_key.insert(key, base);
                                        base
                                    };
                                visual.flags.remove(
                                    BlockFlags::AIR
                                        | BlockFlags::CUBE_GEOMETRY
                                        | BlockFlags::OCCLUDES_FULL_FACE
                                        | BlockFlags::LEAF_MODEL,
                                );
                                visual.faces = faces;
                                visual.kind = VisualKind::Model;
                                visual.model_template = base;
                                visual.variant = rotation | (upside << 2);
                            }
                        } else if is_kelp(record) {
                            let descriptors =
                                BlockFace::ALL.map(|face| descriptor_for(pack, record, face));
                            let materials = descriptors.each_ref().map(|descriptor| {
                                descriptor
                                    .as_ref()
                                    .and_then(|(descriptor, _)| {
                                        material_by_descriptor.get(descriptor)
                                    })
                                    .copied()
                            });
                            if let [
                                Some(west),
                                Some(east),
                                Some(down),
                                Some(up),
                                Some(north),
                                Some(south),
                            ] = materials
                            {
                                let ordered = [north, south, up, down, east, west];
                                let template = if let Some(&template) =
                                    kelp_template_by_material.get(&ordered)
                                {
                                    template
                                } else {
                                    let template =
                                        u32::try_from(model_templates.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            }
                                        })?;
                                    let quad_start =
                                        u32::try_from(model_quads.len()).map_err(|_| {
                                            AssetError::BlobSizeOverflow {
                                                section: "model quad",
                                            }
                                        })?;
                                    model_templates.push(ModelTemplate {
                                        quad_start,
                                        quad_count: 6,
                                        flags: MODEL_TEMPLATE_FLAG_KELP,
                                    });
                                    model_quads.extend(kelp_quads(ordered));
                                    kelp_template_by_material.insert(ordered, template);
                                    template
                                };
                                visual.faces = [west, east, down, up, north, south];
                                visual.kind = VisualKind::Model;
                                visual.model_template = template;
                            }
                        } else if is_cross_visual(record) {
                            let faces = if is_aquatic_cross(record) {
                                aquatic_cross_faces(record)
                            } else {
                                Some([cross_texture_face(record); 2])
                            };
                            if let Some(faces) = faces
                                && let Some((descriptor_a, _)) =
                                    descriptor_for(pack, record, faces[0])
                                && let Some((descriptor_b, _)) =
                                    descriptor_for(pack, record, faces[1])
                                && let Some(&material_a) = material_by_descriptor.get(&descriptor_a)
                                && let Some(&material_b) = material_by_descriptor.get(&descriptor_b)
                                && let Some(variant) = model_variant(pack, record, faces[0])
                            {
                                let materials = [material_a, material_b];
                                let template =
                                    if let Some(&template) = template_by_material.get(&materials) {
                                        template
                                    } else {
                                        let template = u32::try_from(model_templates.len())
                                            .map_err(|_| AssetError::BlobSizeOverflow {
                                                section: "model template",
                                            })?;
                                        let quad_start =
                                            u32::try_from(model_quads.len()).map_err(|_| {
                                                AssetError::BlobSizeOverflow {
                                                    section: "model quad",
                                                }
                                            })?;
                                        model_templates.push(ModelTemplate {
                                            quad_start,
                                            quad_count: 2,
                                            flags: 0,
                                        });
                                        model_quads.extend(crossed_quads(materials));
                                        template_by_material.insert(materials, template);
                                        template
                                    };
                                visual.faces = [material_a; 6];
                                visual.kind = VisualKind::Cross;
                                visual.model_template = template;
                                visual.variant = variant;
                            }
                        } else if record.flags.contains(BlockFlags::CUBE_GEOMETRY)
                            && !record_has_deferred_material(pack, record)
                        {
                            let mut faces = [DIAGNOSTIC_MATERIAL; 6];
                            let mut supported = true;
                            for face in BlockFace::ALL {
                                let Some((descriptor, _)) = descriptor_for(pack, record, face)
                                else {
                                    supported = false;
                                    break;
                                };
                                let Some(&material) = material_by_descriptor.get(&descriptor)
                                else {
                                    supported = false;
                                    break;
                                };
                                faces[face as usize] = material;
                            }
                            if supported {
                                visual.faces = faces;
                                visual.kind = VisualKind::Cube;
                            }
                        }
                        CompileRuleResult::Compiled(visual)
                    }
                    outcome => outcome,
                }
            }
            outcome => outcome,
        };
        visuals[record.sequential_id as usize] = match rule_result {
            CompileRuleResult::Compiled(visual) => visual,
            CompileRuleResult::NoMatch | CompileRuleResult::Reject => {
                BlockVisual::diagnostic(record.flags, record.contributor_role)
            }
        };
        hashed.push((record.network_hash, record.sequential_id));
    }
    hashed.sort_unstable_by_key(|entry| entry.0);
    Ok((
        visuals.into_boxed_slice(),
        hashed.into_boxed_slice(),
        model_templates.into_boxed_slice(),
        model_quads.into_boxed_slice(),
    ))
}
