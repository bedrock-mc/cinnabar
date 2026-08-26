use super::super::*;
use super::context::{
    ButtonTemplateKey, CuboidTemplateKey, GateTemplateKey, ModelStorage, PaleMossCarpetTemplateKey,
    PressurePlateTemplateKey, RuleInputs, SignTemplateKey, diagnostic_visual,
};
use super::fallback::FallbackInventory;

#[derive(Clone, Copy)]
pub(in crate::compiler) struct ExactAdmissions {
    pub(in crate::compiler) mineral_cubes: bool,
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
/// Wire identity of the one record that resolves as canonical air for a
/// compiled registry. Derived from registry content at compile time instead
/// of pinning one legacy protocol's sequential ID and network hash, so a
/// protocol-2168 triple whose air sits at different identities compiles the
/// same exact Invisible route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) struct CanonicalAirIdentity {
    pub(in crate::compiler) sequential_id: u32,
    pub(in crate::compiler) network_hash: u32,
}

fn air_candidate(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:air"
        && record.canonical_state.as_ref() == "{}"
        && record.flags.contains(BlockFlags::AIR)
        && record.contributor_role == ContributorRole::Air
}

/// Resolves the unique canonical-air identity from registry content.
///
/// Fails closed when no record satisfies the full predicate, when more than
/// one does, or when any other record wearing the `minecraft:air` name fails
/// the predicate (a decoy that would make the canonical identity ambiguous).
/// Air-flagged records under other names stay non-fatal decoys and keep the
/// established diagnostic-stripping route.
pub(in crate::compiler) fn resolve_canonical_air(
    records: &[RegistryRecord],
) -> Result<CanonicalAirIdentity, AssetError> {
    let mut canonical = None;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:air")
    {
        if !air_candidate(record) {
            return Err(AssetError::InvalidCompiledAssets {
                detail: format!(
                    "registry record {} names minecraft:air but fails the canonical-air state/flags/role contract",
                    record.sequential_id
                )
                .into(),
            });
        }
        if canonical.replace(record).is_some() {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "registry declares more than one canonical minecraft:air record".into(),
            });
        }
    }
    let canonical = canonical.ok_or_else(|| AssetError::InvalidCompiledAssets {
        detail: "registry declares no canonical minecraft:air record".into(),
    })?;
    Ok(CanonicalAirIdentity {
        sequential_id: canonical.sequential_id,
        network_hash: canonical.network_hash,
    })
}

fn is_canonical_air(record: &RegistryRecord, canonical_air: CanonicalAirIdentity) -> bool {
    record.sequential_id == canonical_air.sequential_id
        && record.network_hash == canonical_air.network_hash
        && air_candidate(record)
}

fn diagnostic_for_unmatched_record(
    record: &RegistryRecord,
    canonical_air: CanonicalAirIdentity,
) -> BlockVisual {
    let mut visual = diagnostic_visual(record);
    if record.flags.contains(BlockFlags::AIR)
        && record.contributor_role == ContributorRole::Air
        && !is_canonical_air(record, canonical_air)
    {
        visual.flags.remove(BlockFlags::AIR);
        visual.contributor_role = ContributorRole::Primary;
    }
    visual
}

#[derive(Default)]
struct VisualCompiler {
    model_templates: Vec<ModelTemplate>,
    model_quads: Vec<ModelQuad>,
    cross_templates: BTreeMap<[u32; 2], u32>,
    kelp_templates: BTreeMap<[u32; 6], u32>,
    transparent_cube_templates: BTreeMap<[u32; 6], u32>,
    flowerbed_templates: BTreeMap<[u32; 4], u32>,
    slab_templates: BTreeMap<[u32; 7], u32>,
    stair_templates: BTreeMap<[u32; 7], u32>,
    vine_templates: BTreeMap<[u32; 2], u32>,
    multiface_templates: BTreeMap<[u32; 3], u32>,
    cuboid_templates: BTreeMap<CuboidTemplateKey, u32>,
    wall_templates: BTreeMap<[u32; 7], u32>,
    pressure_plate_templates: BTreeMap<PressurePlateTemplateKey, u32>,
    button_templates: BTreeMap<ButtonTemplateKey, u32>,
    pale_moss_carpet_templates: BTreeMap<PaleMossCarpetTemplateKey, u32>,
    gate_templates: BTreeMap<GateTemplateKey, u32>,
    pane_templates: BTreeMap<[u32; 2], u32>,
    fence_templates: BTreeMap<[u32; 2], u32>,
    sign_templates: BTreeMap<SignTemplateKey, u32>,
    chiseled_bookshelf_templates: BTreeMap<[u32; 5], u32>,
}

impl VisualCompiler {
    fn compile_record(
        &mut self,
        record: &RegistryRecord,
        inputs: &RuleInputs<'_>,
        admissions: ExactAdmissions,
        canonical_air: CanonicalAirIdentity,
    ) -> Result<CompileRuleResult, AssetError> {
        macro_rules! ordered_rule {
            ($rule:expr) => {
                match $rule? {
                    CompileRuleResult::NoMatch => {}
                    outcome => return Ok(outcome),
                }
            };
        }
        if is_canonical_air(record, canonical_air) {
            let mut visual = diagnostic_visual(record);
            visual.kind = VisualKind::Invisible;
            visual.support = VisualSupport::Exact;
            return Ok(CompileRuleResult::Compiled(visual));
        }

        ordered_rule!(super::fallback::compile_rule(
            record,
            inputs,
            &mut self.cuboid_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        let mut exact_visual = diagnostic_visual(record);
        ordered_rule!(super::exact::compile_exact_families(
            record,
            &mut super::exact::ExactRuleContext {
                pack: inputs.pack,
                material_by_descriptor: inputs.material_by_descriptor,
                admissions,
                visual: &mut exact_visual,
                cuboid_template_by_key: &mut self.cuboid_templates,
                chiseled_bookshelf_template_by_key: &mut self.chiseled_bookshelf_templates,
                model_templates: &mut self.model_templates,
                model_quads: &mut self.model_quads,
            },
        ));

        let mut surface_visual = diagnostic_visual(record);
        ordered_rule!(super::surfaces::compile_surface_rule(
            record,
            &mut super::surfaces::SurfaceRuleContext {
                pack: inputs.pack,
                material_by_descriptor: inputs.material_by_descriptor,
                fallback: inputs.fallback,
                visual: &mut surface_visual,
                transparent_cube_template_by_material: &mut self.transparent_cube_templates,
                flowerbed_template_by_key: &mut self.flowerbed_templates,
                vine_template_by_key: &mut self.vine_templates,
                multiface_template_by_key: &mut self.multiface_templates,
                model_templates: &mut self.model_templates,
                model_quads: &mut self.model_quads,
            },
        ));

        ordered_rule!(super::signs::compile_rule(
            record,
            inputs,
            &mut self.sign_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::doors::compile_rule(
            record,
            inputs,
            &mut self.cuboid_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::panes::compile_rule(
            record,
            inputs,
            &mut self.pane_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::fences::compile_rule(
            record,
            inputs,
            &mut self.fence_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::walls::compile_rule(
            record,
            inputs,
            &mut self.wall_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::pressure_plates::compile_rule(
            record,
            inputs,
            &mut self.pressure_plate_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::button::compile_rule(
            record,
            inputs,
            &mut self.button_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::carpets::compile_rule(
            record,
            inputs,
            &mut super::carpets::CarpetRuleTemplates {
                cuboids: &mut self.cuboid_templates,
                pale: &mut self.pale_moss_carpet_templates,
            },
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::gates::compile_rule(
            record,
            inputs,
            &mut self.gate_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::slabs::compile_rule(
            record,
            inputs,
            &mut self.slab_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::stairs::compile_rule(
            record,
            inputs,
            &mut self.stair_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::kelp::compile_rule(
            record,
            inputs,
            &mut self.kelp_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        ordered_rule!(super::cross::compile_rule(
            record,
            inputs,
            &mut self.cross_templates,
            &mut ModelStorage {
                templates: &mut self.model_templates,
                quads: &mut self.model_quads,
            },
        ));
        Ok(super::cube::compile_rule(record, inputs))
    }
}

pub(in crate::compiler) fn compile_visuals(
    records: &[RegistryRecord],
    pack: &PackSources,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
    vanilla_fallback_material: u32,
    fallback: &'static FallbackInventory<'static>,
    admissions: ExactAdmissions,
) -> Result<CompiledVisuals, AssetError> {
    let canonical_air = resolve_canonical_air(records)?;
    let visual_count = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap_or(0);
    let mut visuals =
        vec![BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary); visual_count];
    let mut hashed = Vec::with_capacity(records.len());
    let mut compiler = VisualCompiler::default();
    let inputs = RuleInputs {
        pack,
        material_by_descriptor,
        vanilla_fallback_material,
        fallback,
    };
    let mut ordered_records = records.iter().collect::<Vec<_>>();
    ordered_records.sort_unstable_by_key(|record| record.sequential_id);
    for record in ordered_records {
        visuals[record.sequential_id as usize] =
            match compiler.compile_record(record, &inputs, admissions, canonical_air)? {
                CompileRuleResult::Compiled(mut visual) => {
                    if visual.kind != VisualKind::Diagnostic
                        && visual.support == VisualSupport::Diagnostic
                    {
                        visual.support = VisualSupport::Exact;
                    }
                    visual
                }
                CompileRuleResult::NoMatch | CompileRuleResult::Reject => {
                    diagnostic_for_unmatched_record(record, canonical_air)
                }
            };
        hashed.push((record.network_hash, record.sequential_id));
    }
    hashed.sort_unstable_by_key(|entry| entry.0);
    Ok((
        visuals.into_boxed_slice(),
        hashed.into_boxed_slice(),
        compiler.model_templates.into_boxed_slice(),
        compiler.model_quads.into_boxed_slice(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::RegistryProvenance;

    #[test]
    fn resolution_pins_the_unique_pinned_registry_air_identity() {
        let records = assets::read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .expect("decode pinned registry");
        let identity = resolve_canonical_air(&records).expect("unique canonical air");

        // The pinned protocol-1001 registry must keep resolving today's
        // exact identity so default v1001 compilation stays byte-identical.
        assert_eq!(identity.sequential_id, 13_094);
        assert_eq!(identity.network_hash, 0xdbf4_4120);

        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("canonical air");
        assert!(is_canonical_air(air, identity));

        let mut custom = air.clone();
        custom.name = "custom:air".into();
        assert!(!is_canonical_air(&custom, identity));

        let mut wrong_state = air.clone();
        wrong_state.canonical_state = r#"{"custom":true}"#.into();
        assert!(!is_canonical_air(&wrong_state, identity));

        let mut wrong_hash = air.clone();
        wrong_hash.network_hash ^= 1;
        assert!(!is_canonical_air(&wrong_hash, identity));

        let mut wrong_id = air.clone();
        wrong_id.sequential_id -= 1;
        assert!(!is_canonical_air(&wrong_id, identity));
    }

    fn synthetic_record(name: &str, state: &str, id: u32, hash: u32) -> RegistryRecord {
        RegistryRecord {
            sequential_id: id,
            network_hash: hash,
            name: name.into(),
            canonical_state: state.into(),
            flags: BlockFlags::AIR,
            model_family: ModelFamily::Air,
            contributor_role: ContributorRole::Air,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn resolution_fails_closed_with_distinct_errors_for_zero_multiple_and_decoy_air() {
        let zero = [synthetic_record("minecraft:stone", "{}", 1, 0x0000_0001)];
        let multiple = [
            synthetic_record("minecraft:air", "{}", 10, 0x0000_000a),
            synthetic_record("minecraft:air", "{}", 11, 0x0000_000b),
        ];
        let decoy = [synthetic_record(
            "minecraft:air",
            r#"{"legacy":true}"#,
            12,
            0x0000_000c,
        )];

        let zero_error = resolve_canonical_air(&zero).expect_err("no canonical air record");
        let multiple_error =
            resolve_canonical_air(&multiple).expect_err("conflicting canonical air records");
        let decoy_error = resolve_canonical_air(&decoy).expect_err("decoy canonical air name");

        for error in [&zero_error, &multiple_error, &decoy_error] {
            assert!(matches!(error, AssetError::InvalidCompiledAssets { .. }));
        }
        assert_ne!(zero_error.to_string(), multiple_error.to_string());
        assert_ne!(multiple_error.to_string(), decoy_error.to_string());
        assert_ne!(zero_error.to_string(), decoy_error.to_string());
    }

    #[test]
    fn resolution_rejects_a_decoy_even_after_a_valid_candidate() {
        let records = [
            synthetic_record("minecraft:air", "{}", 20, 0x0000_0014),
            synthetic_record("minecraft:air", r#"{"legacy":true}"#, 21, 0x0000_0015),
        ];
        let error = resolve_canonical_air(&records).expect_err("decoy after valid candidate");
        assert!(
            error.to_string().contains("fails the canonical-air"),
            "unexpected error: {error}"
        );
    }
}
