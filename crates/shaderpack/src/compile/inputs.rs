//! Removes unused fragment inputs from validated compiled modules.

use naga::valid::{
    Capabilities, GlobalUse, ModuleInfo, ValidationError, ValidationFlags, Validator,
};
use naga::{AddressSpace, Arena, Block, Expression, GlobalVariable, Handle, Module, Statement};
use std::collections::HashSet;

/// Validates the module, removes dead private copies and remaps surviving entry arguments.
pub(super) fn validate(
    module: &mut Module,
) -> Result<ModuleInfo, Box<naga::WithSpan<ValidationError>>> {
    let mut info = module_info(module)?;
    if !module
        .entry_points
        .iter()
        .any(|entry| entry.stage == naga::ShaderStage::Fragment)
    {
        return Ok(info);
    }
    loop {
        let dead: HashSet<_> = module
            .global_variables
            .iter()
            .filter_map(|(handle, global)| {
                (global.space == AddressSpace::Private
                    && (0..module.entry_points.len())
                        .all(|i| !info.get_entry_point(i)[handle].contains(GlobalUse::READ)))
                .then_some(handle)
            })
            .collect();
        let mut changed = false;
        for (_, function) in module.functions.iter_mut() {
            changed |= prune_stores(&mut function.body, &function.expressions, &dead);
        }
        for entry in &mut module.entry_points {
            changed |= prune_stores(&mut entry.function.body, &entry.function.expressions, &dead);
        }
        if !changed {
            break;
        }
        naga::compact::compact(module, naga::compact::KeepUnused::No);
        info = module_info(module)?;
    }
    for entry in &mut module.entry_points {
        let function = &mut entry.function;
        let mut live = vec![false; function.arguments.len()];
        for (_, expression) in function.expressions.iter() {
            if let Expression::FunctionArgument(index) = *expression {
                live[index as usize] = true;
            }
        }
        let mut next = 0;
        let remap: Vec<_> = live
            .iter()
            .map(|&used| {
                if used {
                    let index = next;
                    next += 1;
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        let mut index = 0;
        function.arguments.retain(|_| {
            let keep = live[index];
            index += 1;
            keep
        });
        for (_, expression) in function.expressions.iter_mut() {
            if let Expression::FunctionArgument(index) = expression {
                *index = remap[*index as usize].expect("retained expressions mark live arguments");
            }
        }
    }
    naga::compact::compact(module, naga::compact::KeepUnused::No);
    module_info(module)
}

/// Regenerates validation and transitive usage information for the current module.
fn module_info(module: &Module) -> Result<ModuleInfo, Box<naga::WithSpan<ValidationError>>> {
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(module)
        .map_err(Box::new)
}

/// Removes stores into unread private globals, including synthetic varying-unpacking copies.
fn prune_stores(
    block: &mut Block,
    expressions: &Arena<Expression>,
    dead: &HashSet<Handle<GlobalVariable>>,
) -> bool {
    let mut changed = false;
    for (mut statement, span) in std::mem::take(block).span_into_iter() {
        match &mut statement {
            Statement::Store { pointer, .. }
                if root_global(expressions, *pointer)
                    .is_some_and(|global| dead.contains(&global)) =>
            {
                changed = true;
                continue;
            }
            Statement::Block(body) => changed |= prune_stores(body, expressions, dead),
            Statement::If { accept, reject, .. } => {
                changed |= prune_stores(accept, expressions, dead);
                changed |= prune_stores(reject, expressions, dead);
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                changed |= prune_stores(body, expressions, dead);
                changed |= prune_stores(continuing, expressions, dead);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    changed |= prune_stores(&mut case.body, expressions, dead);
                }
            }
            _ => {}
        }
        block.push(statement, span);
    }
    changed
}

/// Follows an access chain to its global storage, without treating local aliases as globals.
fn root_global(
    expressions: &Arena<Expression>,
    mut pointer: Handle<Expression>,
) -> Option<Handle<GlobalVariable>> {
    loop {
        match expressions[pointer] {
            Expression::GlobalVariable(global) => return Some(global),
            Expression::Access { base, .. } | Expression::AccessIndex { base, .. } => {
                pointer = base
            }
            _ => return None,
        }
    }
}
