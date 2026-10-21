//! Orders parsed functions before validation, including helpers introduced by the rewrite.

use naga::{Block, Expression, Function, Handle, Module, Span, Statement};
use std::collections::VecDeque;

/// Places callees before callers and remaps every function reference, preserving spans.
/// A cycle leaves the module unchanged and returns a blocked function's source span.
pub(super) fn functions(module: &mut Module) -> Result<(), Span> {
    let mut dependencies = vec![0usize; module.functions.len()];
    let mut callers = vec![Vec::new(); module.functions.len()];
    let mut ordered = true;
    for (caller, function) in module.functions.iter_mut() {
        visit_calls(function, |callee| {
            dependencies[caller.index()] += 1;
            callers[callee.index()].push(caller);
            ordered &= callee.index() < caller.index();
        });
    }
    if ordered {
        return Ok(());
    }
    let mut ready: VecDeque<_> = module
        .functions
        .iter()
        .filter_map(|(handle, _)| (dependencies[handle.index()] == 0).then_some(handle))
        .collect();
    let mut order = Vec::with_capacity(module.functions.len());
    while let Some(callee) = ready.pop_front() {
        order.push(callee);
        for &caller in &callers[callee.index()] {
            dependencies[caller.index()] -= 1;
            if dependencies[caller.index()] == 0 {
                ready.push_back(caller);
            }
        }
    }
    if order.len() != module.functions.len() {
        let blocked = module
            .functions
            .iter()
            .find(|(handle, _)| dependencies[handle.index()] != 0)
            .expect("a cycle leaves at least one function blocked")
            .0;
        return Err(module.functions.get_span(blocked));
    }

    let mut remap: Vec<_> = module.functions.iter().map(|(handle, _)| handle).collect();
    let mut original = module.functions.take();
    for handle in order {
        let span = original.get_span(handle);
        let function = std::mem::take(original.get_mut(handle));
        remap[handle.index()] = module.functions.append(function, span);
    }
    for (_, function) in module.functions.iter_mut() {
        visit_calls(function, |handle| *handle = remap[handle.index()]);
    }
    for entry in &mut module.entry_points {
        visit_calls(&mut entry.function, |handle| {
            *handle = remap[handle.index()]
        });
    }
    if let Some(comments) = &mut module.doc_comments {
        comments.functions = std::mem::take(&mut comments.functions)
            .into_iter()
            .map(|(handle, text)| (remap[handle.index()], text))
            .collect();
    }
    Ok(())
}

/// Visits value-returning and void calls in every expression and nested statement block.
fn visit_calls(function: &mut Function, mut visit: impl FnMut(&mut Handle<Function>)) {
    for (_, expression) in function.expressions.iter_mut() {
        if let Expression::CallResult(handle) = expression {
            visit(handle);
        }
    }
    let mut blocks: Vec<&mut Block> = vec![&mut function.body];
    while let Some(block) = blocks.pop() {
        for statement in block.iter_mut() {
            match statement {
                Statement::Call { function, .. } => visit(function),
                Statement::Block(body) => blocks.push(body),
                Statement::If { accept, reject, .. } => {
                    blocks.push(accept);
                    blocks.push(reject);
                }
                Statement::Switch { cases, .. } => {
                    blocks.extend(cases.iter_mut().map(|case| &mut case.body));
                }
                Statement::Loop {
                    body, continuing, ..
                } => {
                    blocks.push(body);
                    blocks.push(continuing);
                }
                _ => {}
            }
        }
    }
}
