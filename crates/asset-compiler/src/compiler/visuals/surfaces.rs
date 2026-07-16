use super::super::*;
use super::context::push_model_template;

pub(in crate::compiler) struct SurfaceRuleContext<'a> {
    pub(in crate::compiler) pack: &'a PackSources,
    pub(in crate::compiler) material_by_descriptor: &'a BTreeMap<Descriptor, u32>,
    pub(in crate::compiler) visual: &'a mut BlockVisual,
    pub(in crate::compiler) transparent_cube_template_by_material: &'a mut BTreeMap<[u32; 6], u32>,
    pub(in crate::compiler) flowerbed_template_by_key: &'a mut BTreeMap<[u32; 4], u32>,
    pub(in crate::compiler) vine_template_by_key: &'a mut BTreeMap<[u32; 2], u32>,
    pub(in crate::compiler) multiface_template_by_key: &'a mut BTreeMap<[u32; 3], u32>,
    pub(in crate::compiler) model_templates: &'a mut Vec<ModelTemplate>,
    pub(in crate::compiler) model_quads: &'a mut Vec<ModelQuad>,
}

pub(in crate::compiler) fn compile_surface_rule(
    record: &RegistryRecord,
    context: &mut SurfaceRuleContext<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let SurfaceRuleContext {
        pack,
        material_by_descriptor,
        visual,
        transparent_cube_template_by_material,
        flowerbed_template_by_key,
        vine_template_by_key,
        multiface_template_by_key,
        model_templates,
        model_quads,
    } = context;
    if (is_ordinary_stained_glass_name(&record.name) && !is_stained_glass_cube(record))
        || (is_copper_grate_name(&record.name) && !is_copper_grate(record))
    {
        // Exact names fail closed when any admission fact disagrees.
    } else if is_stained_glass_cube(record) || is_copper_grate(record) {
        let materials = BlockFace::ALL.map(|face| {
            descriptor_for(pack, record, face)
                .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
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
            let materials = [west, east, down, up, north, south];
            let template =
                if let Some(&template) = transparent_cube_template_by_material.get(&materials) {
                    template
                } else {
                    let template = push_model_template(
                        cuboid_quads(materials, [0, 0, 0], [256, 256, 256]).to_vec(),
                        MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
                        model_templates,
                        model_quads,
                    )?;
                    transparent_cube_template_by_material.insert(materials, template);
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
    } else if is_supported_liquid(record) {
        let materials = BlockFace::ALL.map(|face| {
            descriptor_for(pack, record, face)
                .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        let liquid_depth = record
            .model_state
            .get(ModelStateField::LiquidDepth)
            .or_else(|| canonical_state_u32(&record.canonical_state, "liquid_depth"));
        if let [
            Some(west),
            Some(east),
            Some(down),
            Some(up),
            Some(north),
            Some(south),
        ] = materials
            && let Some(liquid_depth) = liquid_depth.filter(|depth| *depth <= 15)
        {
            visual.flags.remove(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL,
            );
            visual.faces = [west, east, down, up, north, south];
            visual.kind = VisualKind::Liquid;
            visual.variant = liquid_depth;
        }
    } else if is_flowerbed(record) {
        let growth = record.model_state.get(ModelStateField::Growth);
        let orientation = record.model_state.get(ModelStateField::Orientation);
        let materials = flowerbed_material_descriptors(pack, record).map(|descriptors| {
            descriptors.map(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied())
        });
        if let (Some([Some(flower), Some(stem)]), Some(growth @ 0..=7), Some(orientation @ 0..=3)) =
            (materials, growth, orientation)
        {
            const LAYOUT_BY_GROWTH: [u32; 8] = [0, 1, 2, 3, 3, 3, 3, 3];
            let layout = LAYOUT_BY_GROWTH[growth as usize];
            let key = [flower, stem, layout, orientation];
            let template = if let Some(&template) = flowerbed_template_by_key.get(&key) {
                template
            } else {
                let quads = flowerbed_quads([flower, stem], layout, orientation)?;
                let template = u32::try_from(model_templates.len()).map_err(|_| {
                    AssetError::BlobSizeOverflow {
                        section: "model template",
                    }
                })?;
                let quad_start =
                    u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
                        section: "model quad",
                    })?;
                let quad_count =
                    u32::try_from(quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
                        section: "model quad count",
                    })?;
                model_templates.push(ModelTemplate {
                    quad_start,
                    quad_count,
                    flags: 0,
                });
                model_quads.extend(quads);
                flowerbed_template_by_key.insert(key, template);
                template
            };
            visual.faces = [flower; 6];
            visual.kind = VisualKind::Model;
            visual.model_template = template;
        }
    } else if is_vine(record) {
        let material = descriptor_for(pack, record, BlockFace::South)
            .and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied());
        let connections = record.model_state.get(ModelStateField::Connections);
        if let (Some(material), Some(connections @ 0..=15)) = (material, connections) {
            let key = [material, connections];
            let template = if let Some(&template) = vine_template_by_key.get(&key) {
                template
            } else {
                let quads = vine_quads(material, connections);
                let template = u32::try_from(model_templates.len()).map_err(|_| {
                    AssetError::BlobSizeOverflow {
                        section: "model template",
                    }
                })?;
                let quad_start =
                    u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
                        section: "model quad",
                    })?;
                model_templates.push(ModelTemplate {
                    quad_start,
                    quad_count: connections.count_ones(),
                    flags: 0,
                });
                model_quads.extend(quads);
                vine_template_by_key.insert(key, template);
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
    } else if is_multiface(record) {
        let descriptor = if record.model_family == ModelFamily::ResinClump {
            resin_clump_material_descriptor(pack)
        } else {
            descriptor_for(pack, record, BlockFace::South)
        };
        let material =
            descriptor.and_then(|(descriptor, _)| material_by_descriptor.get(&descriptor).copied());
        let connections = record.model_state.get(ModelStateField::Connections);
        if let (Some(material), Some(connections @ 0..=63)) = (material, connections) {
            // Bedrock retains state zero in its canonical palette even
            // though placement never creates it. Vanilla renders that
            // state as the all-face form, unlike vine mask zero.
            let effective_connections = if connections == 0 { 63 } else { connections };
            let family_key = match record.model_family {
                ModelFamily::GlowLichen => 0,
                ModelFamily::SculkVein => 1,
                ModelFamily::ResinClump => 2,
                _ => unreachable!("multiface predicate admitted an unrelated family"),
            };
            let key = [material, effective_connections, family_key];
            let template = if let Some(&template) = multiface_template_by_key.get(&key) {
                template
            } else {
                let quads = multiface_quads(material, effective_connections, record.model_family);
                let template = u32::try_from(model_templates.len()).map_err(|_| {
                    AssetError::BlobSizeOverflow {
                        section: "model template",
                    }
                })?;
                let quad_start =
                    u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
                        section: "model quad",
                    })?;
                model_templates.push(ModelTemplate {
                    quad_start,
                    quad_count: effective_connections.count_ones(),
                    flags: 0,
                });
                model_quads.extend(quads);
                multiface_template_by_key.insert(key, template);
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
    } else {
        return Ok(CompileRuleResult::NoMatch);
    }
    Ok(CompileRuleResult::Compiled(**visual))
}
