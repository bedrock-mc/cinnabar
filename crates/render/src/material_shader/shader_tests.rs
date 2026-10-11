use crate::material_shader::*;
use naga::{Expression, Function, Handle, Literal, Module, Statement};

/// Evaluates integer texture addressing from the parsed production shader.
fn integer(
    module: &Module,
    function: &Function,
    expression: Handle<Expression>,
    reference: u32,
) -> u32 {
    match function.expressions[expression] {
        Expression::Literal(Literal::U32(value)) => value,
        Expression::Literal(Literal::I32(value)) => value as u32,
        Expression::FunctionArgument(_) => reference,
        Expression::As { expr, .. } => integer(module, function, expr, reference),
        Expression::Load { pointer } => integer(module, function, pointer, reference),
        Expression::LocalVariable(variable) => integer(
            module,
            function,
            function.local_variables[variable]
                .init
                .expect("initialized address"),
            reference,
        ),
        Expression::Binary { op, left, right } => {
            let left = integer(module, function, left, reference);
            let right = integer(module, function, right, reference);
            match op {
                naga::BinaryOperator::And => left & right,
                naga::BinaryOperator::ShiftRight => left >> right,
                _ => panic!("unsupported address operator {op:?}"),
            }
        }
        Expression::CallResult(callee) => {
            let argument =
                call_argument(&function.body, expression).expect("address call argument");
            let value = integer(module, function, argument, reference);
            let callee = &module.functions[callee];
            let result = callee
                .body
                .iter()
                .find_map(|statement| match statement {
                    Statement::Return { value } => *value,
                    _ => None,
                })
                .expect("address return value");
            integer(module, callee, result, value)
        }
        ref other => panic!("unsupported address expression {other:?}"),
    }
}

/// Finds a layer decoder's argument inside nested sampling branches.
fn call_argument(
    block: &naga::Block,
    expression: Handle<Expression>,
) -> Option<Handle<Expression>> {
    block.iter().find_map(|statement| match statement {
        Statement::Call {
            arguments,
            result: Some(result),
            ..
        } if *result == expression => Some(arguments[0]),
        Statement::If { accept, reject, .. } => {
            call_argument(accept, expression).or_else(|| call_argument(reject, expression))
        }
        Statement::Block(block) => call_argument(block, expression),
        _ => None,
    })
}

/// Checks each production array index against the carrier layer on both pages.
fn assert_world_shader_addresses(source: &str) {
    for definitions in [
        &[][..],
        &["ENHANCED"][..],
        &["ALPHA_TO_COVERAGE"][..],
        &["ENHANCED", "ALPHA_TO_COVERAGE"][..],
    ] {
        let source = crate::shader_source::standalone(source, definitions);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let mut samples = 0;
        for (_, function) in module.functions.iter() {
            // Terrain sampling functions receive their texture reference directly.
            if !function.name.as_deref().is_some_and(|name| {
                name.starts_with("sample_") || name.ends_with("alpha_footprint")
            }) {
                continue;
            }
            for (_, expression) in function.expressions.iter() {
                let index = match expression {
                    Expression::ImageSample {
                        array_index: Some(index),
                        ..
                    }
                    | Expression::ImageLoad {
                        array_index: Some(index),
                        ..
                    } => *index,
                    _ => continue,
                };
                samples += 1;
                for page in 0..assets::MAX_TEXTURE_PAGES as u32 {
                    for layer in [1, 2049, assets::TEXTURE_LAYER_MASK] {
                        let reference = assets::TextureRef::new(page, layer).unwrap();
                        for grid in [0, 1, 3] {
                            let encoded = gpu_grid_texture_ref(reference, [32; 2], 256, grid);
                            assert_eq!(
                                integer(&module, function, index, encoded),
                                layer,
                                "{:?}: page {page}, layer {layer}, grid {grid}",
                                function.name
                            );
                        }
                    }
                }
            }
        }
        assert!(samples > 0, "production array samples must be covered");
    }
}

#[test]
fn cube_samples_preserve_layers_and_exclude_metadata() {
    assert_world_shader_addresses(include_str!("../chunk.wesl"));
}

#[test]
fn model_samples_preserve_layers_and_exclude_metadata() {
    assert_world_shader_addresses(include_str!("../model.wesl"));
}

#[test]
fn liquid_samples_preserve_layers_and_exclude_metadata() {
    assert_world_shader_addresses(include_str!("../liquid.wesl"));
}
