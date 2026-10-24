//! Supplies deterministic vertex outputs for fragment varyings without a producer.

use crate::compile::CompiledStage;
use naga::{
    Arena, Binding, Block, Expression, Handle, Span, Statement, StructMember, Type, TypeInner,
};
use std::collections::BTreeMap;

/// Adds zero-valued outputs only at locations absent from the compiled vertex interface.
/// Existing outputs, including incompatible ones, remain for the normal linker to check.
pub(crate) fn supply_vertex_outputs(
    vertex: &mut CompiledStage,
    fragment: &CompiledStage,
) -> Result<(), String> {
    let Some(result) = vertex.module.entry_points[0].function.result.as_ref() else {
        return Ok(());
    };
    let mut outputs = BTreeMap::new();
    super::locations(
        &vertex.module,
        result.ty,
        result.binding.as_ref(),
        &mut outputs,
    )?;
    let mut missing = Vec::new();
    let mut pending: Vec<_> = fragment.module.entry_points[0]
        .function
        .arguments
        .iter()
        .map(|arg| (arg.name.clone(), arg.ty, arg.binding.clone()))
        .collect();
    while let Some((name, ty, binding)) = pending.pop() {
        match binding {
            Some(Binding::Location { location, .. }) if !outputs.contains_key(&location) => {
                missing.push((location, name, ty, binding));
            }
            None => {
                if let TypeInner::Struct { members, .. } = &fragment.module.types[ty].inner {
                    pending.extend(
                        members
                            .iter()
                            .map(|member| (member.name.clone(), member.ty, member.binding.clone())),
                    );
                }
            }
            _ => {}
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort_by_key(|(location, ..)| *location);
    let old_ty = result.ty;
    let (mut members, mut span) = match &vertex.module.types[old_ty].inner {
        TypeInner::Struct { members, span } if result.binding.is_none() => (members.clone(), *span),
        inner => (
            vec![StructMember {
                name: None,
                ty: old_ty,
                binding: result.binding.clone(),
                offset: 0,
            }],
            inner.size(vertex.module.to_ctx()),
        ),
    };
    let old_members = members.len();
    let mut zeros = Vec::new();
    for (_, name, ty, binding) in missing {
        let inner = &fragment.module.types[ty].inner;
        if !matches!(inner, TypeInner::Scalar(_) | TypeInner::Vector { .. }) {
            return Err("unsupported missing varying type".into());
        }
        let ty = vertex.module.types.insert(
            Type {
                name: None,
                inner: inner.clone(),
            },
            Span::UNDEFINED,
        );
        members.push(StructMember {
            name,
            ty,
            binding,
            offset: span,
        });
        span += vertex.module.types[ty].inner.size(vertex.module.to_ctx());
        zeros.push(
            vertex.module.entry_points[0]
                .function
                .expressions
                .append(Expression::ZeroValue(ty), Span::UNDEFINED),
        );
    }
    let ty = vertex.module.types.insert(
        Type {
            name: None,
            inner: TypeInner::Struct { members, span },
        },
        Span::UNDEFINED,
    );
    let old_struct = matches!(vertex.module.types[old_ty].inner, TypeInner::Struct { .. });
    let function = &mut vertex.module.entry_points[0].function;
    extend_returns(
        &mut function.body,
        &mut function.expressions,
        ty,
        old_struct.then_some(old_members),
        &zeros,
    );
    function.result.as_mut().unwrap().ty = ty;
    function.result.as_mut().unwrap().binding = None;
    vertex.info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&vertex.module)
    .map_err(|error| format!("cannot supply missing vertex outputs: {error}"))?;
    Ok(())
}

/// Extends every return while preserving its existing values and control flow.
fn extend_returns(
    block: &mut Block,
    expressions: &mut Arena<Expression>,
    ty: Handle<Type>,
    old_members: Option<usize>,
    zeros: &[Handle<Expression>],
) {
    for (mut statement, span) in std::mem::take(block).span_into_iter() {
        match &mut statement {
            Statement::Return { value: Some(value) } => {
                let start = expressions.len();
                let mut components = match old_members {
                    Some(count) => (0..count)
                        .map(|index| {
                            expressions.append(
                                Expression::AccessIndex {
                                    base: *value,
                                    index: index as u32,
                                },
                                span,
                            )
                        })
                        .collect(),
                    None => vec![*value],
                };
                components.extend_from_slice(zeros);
                *value = expressions.append(Expression::Compose { ty, components }, span);
                block.push(Statement::Emit(expressions.range_from(start)), span);
            }
            Statement::Block(body) => extend_returns(body, expressions, ty, old_members, zeros),
            Statement::If { accept, reject, .. } => {
                extend_returns(accept, expressions, ty, old_members, zeros);
                extend_returns(reject, expressions, ty, old_members, zeros);
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                extend_returns(body, expressions, ty, old_members, zeros);
                extend_returns(continuing, expressions, ty, old_members, zeros);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    extend_returns(&mut case.body, expressions, ty, old_members, zeros);
                }
            }
            _ => {}
        }
        block.push(statement, span);
    }
}
