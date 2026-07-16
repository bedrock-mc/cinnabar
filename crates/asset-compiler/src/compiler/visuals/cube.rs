use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    if !record.flags.contains(BlockFlags::CUBE_GEOMETRY)
        || record_has_deferred_material(inputs.pack, record)
    {
        return CompileRuleResult::NoMatch;
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        visual.faces = materials;
        visual.kind = VisualKind::Cube;
    }
    CompileRuleResult::Compiled(visual)
}
