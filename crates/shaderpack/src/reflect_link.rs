//! Checks the cross-stage contract that per-module validation cannot establish.

use crate::compile::CompiledStage;
use crate::programs::StageKind;
use crate::reflect::value_type;
use crate::reflection::ValueType;
use naga::{Binding, TypeInner};
use std::collections::BTreeMap;

/// Requires every fragment input to have a compatible vertex output at its location.
pub(crate) fn validate(stages: &[&CompiledStage]) -> Result<(), String> {
    let Some(vertex) = stages.iter().find(|stage| stage.stage == StageKind::Vertex) else {
        return Ok(());
    };
    let Some(fragment) = stages
        .iter()
        .find(|stage| stage.stage == StageKind::Fragment)
    else {
        return Ok(());
    };
    let mut outputs = BTreeMap::new();
    if let Some(entry) = vertex.module.entry_points.first()
        && let Some(result) = &entry.function.result
    {
        locations(
            &vertex.module,
            result.ty,
            result.binding.as_ref(),
            &mut outputs,
        )?;
    }
    let mut inputs = BTreeMap::new();
    if let Some(entry) = fragment.module.entry_points.first() {
        for argument in &entry.function.arguments {
            locations(
                &fragment.module,
                argument.ty,
                argument.binding.as_ref(),
                &mut inputs,
            )?;
        }
    }
    for (location, input) in inputs {
        match outputs.get(&location) {
            None => {
                return Err(format!(
                    "fragment input location {location} has no vertex output"
                ));
            }
            Some(output) if output != &input => {
                return Err(format!(
                    "vertex output and fragment input at location {location} have different types"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Collects typed user locations from direct values or entry-point interface structs.
fn locations(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    binding: Option<&Binding>,
    out: &mut BTreeMap<u32, ValueType>,
) -> Result<(), String> {
    match (binding, &module.types[ty].inner) {
        (Some(Binding::Location { location, .. }), inner) => {
            let ty = value_type(inner).ok_or_else(|| {
                format!("unsupported stage interface type at location {location}")
            })?;
            if out.insert(*location, ty).is_some() {
                return Err(format!("duplicate stage interface location {location}"));
            }
        }
        (None, TypeInner::Struct { members, .. }) => {
            for member in members {
                locations(module, member.ty, member.binding.as_ref(), out)?;
            }
        }
        _ => {}
    }
    Ok(())
}
