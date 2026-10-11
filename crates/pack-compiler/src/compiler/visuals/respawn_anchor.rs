//! Respawn anchors use six cube faces at the selected charge variant.

use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

/// Compiles supported charge states without promoting unrelated unknown blocks.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    if !crate::respawn_anchor::is_record(record) {
        return CompileRuleResult::NoMatch;
    }
    if crate::respawn_anchor::charge(record).is_none()
        || record_has_deferred_material(inputs.pack, record)
    {
        return CompileRuleResult::Reject;
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        visual.faces = materials;
        visual.kind = VisualKind::Cube;
        visual.flags.insert(BlockFlags::CUBE_GEOMETRY);
        visual.flags.remove(BlockFlags::OCCLUDES_FULL_FACE);
    }
    CompileRuleResult::Compiled(visual)
}
