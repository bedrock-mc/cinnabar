//! Checks the cross-stage contract that per-module validation cannot establish.

use crate::compile::CompiledStage;
use crate::programs::StageKind;
use crate::reflect::value_type;
use crate::reflection::ValueType;
use naga::{Binding, TypeInner};
use std::collections::BTreeMap;

mod missing;
pub(crate) use missing::supply_vertex_outputs;

/// Requires compatible live buffer layouts and vertex outputs for every fragment input.
pub(crate) fn validate(stages: &[&CompiledStage]) -> Result<(), String> {
    validate_buffers(stages)?;
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

/// Rejects different memory layouts at a buffer binding used by more than one stage.
fn validate_buffers(stages: &[&CompiledStage]) -> Result<(), String> {
    let mut buffers = BTreeMap::new();
    for stage in stages {
        if stage.module.entry_points.is_empty() {
            continue;
        }
        let usage = stage.info.get_entry_point(0);
        for (handle, variable) in stage.module.global_variables.iter() {
            let Some(binding) = &variable.binding else {
                continue;
            };
            if usage[handle].is_empty()
                || !matches!(
                    variable.space,
                    naga::AddressSpace::Uniform | naga::AddressSpace::Storage { .. }
                )
            {
                continue;
            }
            if let Some((module, ty)) = buffers.insert(binding, (&stage.module, variable.ty))
                && !same_buffer_layout(module, ty, &stage.module, variable.ty)
            {
                return Err(format!(
                    "stages disagree on the buffer layout at group {} binding {}",
                    binding.group, binding.binding
                ));
            }
        }
    }
    Ok(())
}

/// Compares validated buffer types by offsets, strides and value types, ignoring names.
fn same_buffer_layout(
    left: &naga::Module,
    left_ty: naga::Handle<naga::Type>,
    right: &naga::Module,
    right_ty: naga::Handle<naga::Type>,
) -> bool {
    let mut pending = vec![(left_ty, right_ty)];
    let mut visited = std::collections::HashSet::new();
    while let Some((a, b)) = pending.pop() {
        if !visited.insert((a, b)) {
            continue;
        }
        match (&left.types[a].inner, &right.types[b].inner) {
            (
                TypeInner::Struct {
                    members: a,
                    span: a_span,
                },
                TypeInner::Struct {
                    members: b,
                    span: b_span,
                },
            ) if a_span == b_span && a.len() == b.len() => {
                for (a, b) in a.iter().zip(b) {
                    if a.offset != b.offset {
                        return false;
                    }
                    pending.push((a.ty, b.ty));
                }
            }
            (
                TypeInner::Array {
                    base: a,
                    size: a_size,
                    stride: a_stride,
                },
                TypeInner::Array {
                    base: b,
                    size: b_size,
                    stride: b_stride,
                },
            ) if a_size == b_size && a_stride == b_stride => pending.push((*a, *b)),
            (
                TypeInner::Scalar(a) | TypeInner::Atomic(a),
                TypeInner::Scalar(b) | TypeInner::Atomic(b),
            ) if a == b => {}
            (a, b) if a == b => {}
            _ => return false,
        }
    }
    true
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

#[cfg(test)]
mod tests {
    use super::same_buffer_layout;

    /// Builds a module with one scalar or atomic buffer leaf for layout comparisons.
    fn leaf(scalar: naga::Scalar, atomic: bool) -> (naga::Module, naga::Handle<naga::Type>) {
        let mut module = naga::Module::default();
        let ty = module.types.insert(
            naga::Type {
                name: None,
                inner: if atomic {
                    naga::TypeInner::Atomic(scalar)
                } else {
                    naga::TypeInner::Scalar(scalar)
                },
            },
            naga::Span::UNDEFINED,
        );
        (module, ty)
    }

    #[test]
    fn scalar_and_atomic_leaves_require_matching_kind_and_width() {
        for left_atomic in [false, true] {
            for right_atomic in [false, true] {
                let (left, left_ty) = leaf(naga::Scalar::U32, left_atomic);
                for right_scalar in [
                    naga::Scalar::U32,
                    naga::Scalar::I32,
                    naga::Scalar {
                        kind: naga::ScalarKind::Uint,
                        width: 8,
                    },
                ] {
                    let (right, right_ty) = leaf(right_scalar, right_atomic);
                    assert_eq!(
                        same_buffer_layout(&left, left_ty, &right, right_ty),
                        right_scalar == naga::Scalar::U32,
                        "left atomic={left_atomic}, right atomic={right_atomic}, right scalar={right_scalar:?}"
                    );
                }
            }
        }
    }
}
