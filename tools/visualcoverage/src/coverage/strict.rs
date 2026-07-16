use super::*;

/// Validates the complete semantic draw graph for decoded records.
///
/// `enforce_protocol_1001` is false only for bounded synthetic fixtures. All
/// production callers must use [`strict_bytes`], which enforces the exact
/// reviewed protocol inventory and baseline.
pub fn strict_records(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
    enforce_protocol_1001: bool,
) -> Result<StrictReport, CoverageError> {
    let ratchet_report = if enforce_protocol_1001 {
        ratchet_protocol_1001(snapshot, baseline)?
    } else {
        ratchet(snapshot, baseline)?
    };

    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|record| record.sequential_id);
    let mut routes = Vec::with_capacity(ordered.len());
    let mut states_by_stream = BTreeMap::from([
        (RenderStream::NoDraw, 0),
        (RenderStream::Cube, 0),
        (RenderStream::Model, 0),
        (RenderStream::Liquid, 0),
    ]);

    for record in ordered {
        if runtime.sequential_id_for_hash(record.network_hash) != Some(record.sequential_id) {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        if !sequential.is_known() || !hashed.is_known() || sequential != hashed {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }

        let state = StateIdentity::from_record(record);
        let kind = visual_kind_name(sequential.kind()).to_owned();
        if record.model_family == ModelFamily::Unknown {
            return Err(CoverageError::UnsupportedModelFamily {
                state,
                family: model_family_name(record.model_family).to_owned(),
            });
        }

        if record.flags.contains(BlockFlags::AIR) {
            if sequential.kind() != VisualKind::Invisible
                || sequential.contributor_role() != ContributorRole::Air
                || !sequential.flags().contains(BlockFlags::AIR)
                || sequential.model_template().is_some()
                || sequential.animation().is_some()
                || BlockFace::ALL
                    .into_iter()
                    .any(|face| sequential.face(face).material_id() != DIAGNOSTIC_MATERIAL)
            {
                return Err(CoverageError::InvalidAirRoute { state, kind });
            }
            push_strict_route(
                &mut routes,
                &mut states_by_stream,
                StrictStateRoute {
                    state,
                    visual_kind: kind,
                    render_stream: RenderStream::NoDraw,
                    material_ids: Vec::new(),
                    model_template: None,
                    animation_ids: Vec::new(),
                },
            );
            continue;
        }

        if sequential.kind() == VisualKind::Diagnostic {
            return Err(CoverageError::NonAirDiagnostic { state });
        }

        let mut material_ids = BTreeSet::new();
        let mut animation_ids = BTreeSet::new();
        let (render_stream, model_template) = match sequential.kind() {
            VisualKind::Invisible => {
                if sequential.contributor_role() == ContributorRole::Air
                    || sequential.flags().contains(BlockFlags::AIR)
                    || sequential.model_template().is_some()
                    || sequential.animation().is_some()
                    || BlockFace::ALL
                        .into_iter()
                        .any(|face| sequential.face(face).material_id() != DIAGNOSTIC_MATERIAL)
                {
                    return Err(CoverageError::InvalidInvisibleRoute { state, kind });
                }
                (RenderStream::NoDraw, None)
            }
            VisualKind::Cube => {
                for face in BlockFace::ALL {
                    let material_id = sequential.face(face).material_id();
                    if material_id == DIAGNOSTIC_MATERIAL {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    }
                    material_ids.insert(material_id);
                }
                (RenderStream::Cube, None)
            }
            VisualKind::Cross | VisualKind::Model => {
                let Some(template_id) = sequential.model_template() else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let template_start = template_id as usize;
                let Some(base_template) = runtime.model_templates().get(template_start) else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let (template_count, allowed_empty_offset) =
                    if base_template.flags & MODEL_TEMPLATE_FLAG_STAIR != 0 {
                        (5, None)
                    } else if base_template.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                        (2, None)
                    } else if base_template.flags & MODEL_TEMPLATE_FLAG_PANE != 0 {
                        (16, None)
                    } else if base_template.flags
                        & (MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER)
                        != 0
                    {
                        (17, Some(1))
                    } else {
                        (1, None)
                    };
                let Some(template_end) = template_start.checked_add(template_count) else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let Some(templates) = runtime.model_templates().get(template_start..template_end)
                else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                for (offset, template) in templates.iter().enumerate() {
                    if template.quad_count == 0 && allowed_empty_offset != Some(offset) {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    }
                    let start = template.quad_start as usize;
                    let Some(end) = start.checked_add(template.quad_count as usize) else {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    };
                    let Some(quads) = runtime.model_quads().get(start..end) else {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    };
                    for quad in quads {
                        if quad.material == DIAGNOSTIC_MATERIAL {
                            return Err(CoverageError::DiagnosticMaterialReference {
                                state,
                                material_id: quad.material,
                            });
                        }
                        material_ids.insert(quad.material);
                    }
                }
                (RenderStream::Model, Some(template_id))
            }
            VisualKind::Liquid => {
                if sequential.variant() > 15 {
                    return Err(CoverageError::InvalidLiquidDepth {
                        state,
                        variant: sequential.variant(),
                    });
                }
                for face in BlockFace::ALL {
                    let material_id = sequential.face(face).material_id();
                    if material_id == DIAGNOSTIC_MATERIAL {
                        return Err(CoverageError::DiagnosticMaterialReference {
                            state,
                            material_id,
                        });
                    }
                    material_ids.insert(material_id);
                }
                let all_water = material_ids
                    .iter()
                    .all(|&id| material_is_water(runtime, id));
                let all_lava = material_ids
                    .iter()
                    .all(|&id| material_is_depth_writing_liquid(runtime, id));
                if !all_water && !all_lava {
                    return Err(CoverageError::InvalidLiquidMaterials {
                        state,
                        material_ids: material_ids.iter().copied().collect(),
                    });
                }
                (RenderStream::Liquid, None)
            }
            VisualKind::Diagnostic => unreachable!("diagnostic handled above"),
        };

        if let Some(animation_id) = sequential.animation() {
            animation_ids.insert(animation_id);
        }
        for &material_id in &material_ids {
            validate_reached_material(runtime, &state, material_id, &mut animation_ids)?;
        }
        for &animation_id in &animation_ids {
            validate_reached_animation(runtime, &state, animation_id)?;
        }

        push_strict_route(
            &mut routes,
            &mut states_by_stream,
            StrictStateRoute {
                state,
                visual_kind: kind,
                render_stream,
                material_ids: material_ids.into_iter().collect(),
                model_template,
                animation_ids: animation_ids.into_iter().collect(),
            },
        );
    }

    Ok(StrictReport {
        schema: STRICT_REPORT_SCHEMA,
        protocol: ratchet_report.protocol,
        registry_sha256: ratchet_report.registry_sha256,
        assets_sha256: ratchet_report.assets_sha256,
        counts: ratchet_report.counts,
        routes,
        invisible_decisions: ratchet_report.invisible_decisions,
        states_by_stream,
    })
}

pub fn strict_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
    baseline: &Baseline,
) -> Result<StrictReport, CoverageError> {
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    let snapshot = analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )?;
    strict_records(&records, &runtime, snapshot, baseline, true)
}

/// Compiles the exact protocol-1001 target inventory consumed by later gallery
/// placement and screenshot stages. Diagnostics are retained as explicit
/// targets. Acceptance requires both zero diagnostics and a fully valid strict
/// semantic draw graph.
pub(super) fn assemble_gallery_inventory(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
    baseline_sha256: &str,
    strict_semantics_valid: bool,
) -> Result<GalleryInventory, CoverageError> {
    let report = ratchet_protocol_1001(snapshot, baseline)?;
    let diagnostic_ids = report
        .diagnostic_states
        .iter()
        .map(|state| state.sequential_id)
        .collect::<BTreeSet<_>>();
    let invisible_ids = report
        .invisible_decisions
        .iter()
        .map(|decision| decision.state.sequential_id)
        .collect::<BTreeSet<_>>();
    let targets = report
        .states
        .into_iter()
        .map(|state| {
            let status = if diagnostic_ids.contains(&state.sequential_id) {
                GalleryTargetStatus::Diagnostic
            } else if invisible_ids.contains(&state.sequential_id) {
                GalleryTargetStatus::Invisible
            } else {
                GalleryTargetStatus::Drawable
            };
            GalleryTarget {
                sequential_id: state.sequential_id,
                network_hash: state.network_hash,
                name: state.name,
                canonical_state: state.canonical_state,
                model_family: state.model_family,
                is_air: state.is_air,
                status,
            }
        })
        .collect::<Vec<_>>();
    let pages = targets
        .chunks(GALLERY_PAGE_CAPACITY)
        .enumerate()
        .map(|(index, targets)| GalleryPage {
            index: index as u32,
            first_sequential_id: targets
                .first()
                .expect("canonical protocol inventory is non-empty")
                .sequential_id,
            last_sequential_id: targets
                .last()
                .expect("canonical protocol inventory is non-empty")
                .sequential_id,
            targets: targets.to_vec(),
        })
        .collect();
    let diagnostic_targets = diagnostic_ids.len();
    Ok(GalleryInventory {
        schema: GALLERY_INVENTORY_SCHEMA.to_owned(),
        protocol: report.protocol,
        registry_sha256: report.registry_sha256,
        assets_sha256: report.assets_sha256,
        baseline_sha256: baseline_sha256.to_owned(),
        accepting: diagnostic_targets == 0 && strict_semantics_valid,
        diagnostic_targets,
        target_count: PROTOCOL_1001_COUNTS.states,
        pages,
    })
}

pub fn gallery_inventory_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
    baseline_bytes: &[u8],
) -> Result<GalleryInventory, CoverageError> {
    let baseline = parse_baseline(baseline_bytes)?;
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    let snapshot = analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )?;
    let strict_semantics_valid =
        strict_records(&records, &runtime, snapshot.clone(), &baseline, true).is_ok();
    assemble_gallery_inventory(
        snapshot,
        &baseline,
        &sha256(baseline_bytes),
        strict_semantics_valid,
    )
}

fn push_strict_route(
    routes: &mut Vec<StrictStateRoute>,
    states_by_stream: &mut BTreeMap<RenderStream, usize>,
    route: StrictStateRoute,
) {
    *states_by_stream.entry(route.render_stream).or_default() += 1;
    routes.push(route);
}

fn validate_reached_material(
    runtime: &RuntimeAssets,
    state: &StateIdentity,
    material_id: u32,
    animation_ids: &mut BTreeSet<u32>,
) -> Result<(), CoverageError> {
    if material_id == DIAGNOSTIC_MATERIAL {
        return Err(CoverageError::DiagnosticMaterialReference {
            state: state.clone(),
            material_id,
        });
    }
    let material = runtime.material(material_id);
    if material.texture == TextureRef::DIAGNOSTIC {
        return Err(CoverageError::DiagnosticTextureReference {
            state: state.clone(),
            material_id,
        });
    }
    if material.animation != assets::NO_ANIMATION {
        animation_ids.insert(material.animation);
    }
    Ok(())
}

fn validate_reached_animation(
    runtime: &RuntimeAssets,
    state: &StateIdentity,
    animation_id: u32,
) -> Result<(), CoverageError> {
    let Some(animation) = runtime.animations().get(animation_id as usize) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    if animation.frame_count == 0 {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    }
    let start = animation.frame_start as usize;
    let Some(end) = start.checked_add(animation.frame_count as usize) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    let Some(frames) = runtime.animation_frames().get(start..end) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    if frames.contains(&TextureRef::DIAGNOSTIC) {
        return Err(CoverageError::DiagnosticAnimationFrameReference {
            state: state.clone(),
            animation_id,
        });
    }
    Ok(())
}

fn material_is_water(runtime: &RuntimeAssets, material_id: u32) -> bool {
    let required = MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT;
    runtime.material(material_id).flags & required == required
}

fn material_is_depth_writing_liquid(runtime: &RuntimeAssets, material_id: u32) -> bool {
    runtime.material(material_id).flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0
}
