use super::super::*;
use super::context::{
    ButtonTemplateKey, CuboidTemplateKey, GateTemplateKey, ModelStorage, PaleMossCarpetTemplateKey,
    PressurePlateTemplateKey, RuleInputs, SignTemplateKey, diagnostic_visual,
};

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
    ) -> Result<CompileRuleResult, AssetError> {
        macro_rules! ordered_rule {
            ($rule:expr) => {
                match $rule? {
                    CompileRuleResult::NoMatch => {}
                    outcome => return Ok(outcome),
                }
            };
        }

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
    let mut compiler = VisualCompiler::default();
    let inputs = RuleInputs {
        pack,
        material_by_descriptor,
    };
    let mut ordered_records = records.iter().collect::<Vec<_>>();
    ordered_records.sort_unstable_by_key(|record| record.sequential_id);
    for record in ordered_records {
        visuals[record.sequential_id as usize] =
            match compiler.compile_record(record, &inputs, admissions)? {
                CompileRuleResult::Compiled(visual) => visual,
                CompileRuleResult::NoMatch | CompileRuleResult::Reject => diagnostic_visual(record),
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
