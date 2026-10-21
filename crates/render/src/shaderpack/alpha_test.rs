//! Iris's alpha test, applied to a translated fragment stage: the fragment is discarded unless
//! `outColor0.a <function> reference` holds, as Iris appends to every geometry program.
//!
//! The test is injected into the naga module before each return of the fragment entry point,
//! so one translation serves every draw that uses the program with a different test.

use naga::{
    BinaryOperator, Block, Expression, Handle, Literal, Module, ScalarKind, ShaderStage, Span,
    Statement, TypeInner, UnaryOperator, VectorSize,
};
use shaderpack::{AlphaTest, CompareFunction};

/// Iris's default alpha test of cutout geometry: `alpha > 0.1`.
pub(crate) const ONE_TENTH_ALPHA: AlphaTest = AlphaTest {
    function: CompareFunction::Greater,
    reference: 0.1,
};

fn operator(function: CompareFunction) -> Option<BinaryOperator> {
    Some(match function {
        CompareFunction::Less => BinaryOperator::Less,
        CompareFunction::Equal => BinaryOperator::Equal,
        CompareFunction::LessEqual => BinaryOperator::LessEqual,
        CompareFunction::Greater => BinaryOperator::Greater,
        CompareFunction::NotEqual => BinaryOperator::NotEqual,
        CompareFunction::GreaterEqual => BinaryOperator::GreaterEqual,
        CompareFunction::Never | CompareFunction::Always => return None,
    })
}

/// How the entry point's result reaches location 0.
#[derive(Clone, Copy)]
enum Output {
    /// The result itself is the vec4 at location 0.
    Whole,
    /// Member `index` of the result struct is.
    Member(u32),
}

fn is_vec4_float(module: &Module, ty: Handle<naga::Type>) -> bool {
    matches!(
        module.types[ty].inner,
        TypeInner::Vector {
            size: VectorSize::Quad,
            scalar: naga::Scalar {
                kind: ScalarKind::Float,
                ..
            },
        }
    )
}

fn location_zero(binding: Option<&naga::Binding>) -> bool {
    matches!(binding, Some(naga::Binding::Location { location: 0, .. }))
}

/// Applies `test` to the fragment entry point of `module`. Returns `false`, leaving the module
/// unchanged, when the module has no fragment entry point writing a `vec4` at location 0 (the
/// test then has nothing to read, as in GL without `gl_FragData[0]`). `Always` changes nothing.
pub(crate) fn inject_alpha_test(module: &mut Module, test: AlphaTest) -> bool {
    if test.function == CompareFunction::Always {
        return true;
    }
    let Some(entry) = module
        .entry_points
        .iter()
        .position(|e| e.stage == ShaderStage::Fragment)
    else {
        return false;
    };
    let Some(result) = module.entry_points[entry].function.result.as_ref() else {
        return false;
    };
    let output = match &module.types[result.ty].inner {
        TypeInner::Struct { members, .. } => {
            let Some(index) = members
                .iter()
                .position(|m| location_zero(m.binding.as_ref()) && is_vec4_float(module, m.ty))
            else {
                return false;
            };
            Output::Member(index as u32)
        }
        _ if location_zero(result.binding.as_ref()) && is_vec4_float(module, result.ty) => {
            Output::Whole
        }
        _ => return false,
    };
    let function = &mut module.entry_points[entry].function;
    let body = std::mem::take(&mut function.body);
    function.body = guard_returns(body, &mut function.expressions, output, test);
    true
}

/// Rebuilds `block` with the alpha test before every `return`, recursing into nested blocks.
fn guard_returns(
    block: Block,
    expressions: &mut naga::Arena<Expression>,
    output: Output,
    test: AlphaTest,
) -> Block {
    let mut out = Block::new();
    let nested = |block: Block, expressions: &mut naga::Arena<Expression>| {
        guard_returns(block, expressions, output, test)
    };
    for (statement, span) in block.span_into_iter() {
        let statement = match statement {
            Statement::Return { value: Some(value) } => {
                push_test(&mut out, expressions, value, output, test);
                Statement::Return { value: Some(value) }
            }
            Statement::Block(inner) => Statement::Block(nested(inner, expressions)),
            Statement::If {
                condition,
                accept,
                reject,
            } => Statement::If {
                condition,
                accept: nested(accept, expressions),
                reject: nested(reject, expressions),
            },
            Statement::Switch { selector, cases } => Statement::Switch {
                selector,
                cases: cases
                    .into_iter()
                    .map(|case| naga::SwitchCase {
                        body: nested(case.body, expressions),
                        ..case
                    })
                    .collect(),
            },
            Statement::Loop {
                body,
                continuing,
                break_if,
            } => Statement::Loop {
                body: nested(body, expressions),
                continuing,
                break_if,
            },
            other => other,
        };
        out.push(statement, span);
    }
    out
}

/// Appends `if (!(colour.a <op> reference)) discard;` reading the returned `value`.
fn push_test(
    out: &mut Block,
    expressions: &mut naga::Arena<Expression>,
    value: Handle<Expression>,
    output: Output,
    test: AlphaTest,
) {
    let Some(op) = operator(test.function) else {
        // `Never`: every fragment is discarded.
        out.push(Statement::Kill, Span::UNDEFINED);
        return;
    };
    // Literals are not emitted; everything after them is one emit range.
    let reference = expressions.append(
        Expression::Literal(Literal::F32(test.reference)),
        Span::UNDEFINED,
    );
    let start = expressions.len();
    let colour = match output {
        Output::Whole => value,
        Output::Member(index) => expressions.append(
            Expression::AccessIndex { base: value, index },
            Span::UNDEFINED,
        ),
    };
    let alpha = expressions.append(
        Expression::AccessIndex {
            base: colour,
            index: 3,
        },
        Span::UNDEFINED,
    );
    let passes = expressions.append(
        Expression::Binary {
            op,
            left: alpha,
            right: reference,
        },
        Span::UNDEFINED,
    );
    let fails = expressions.append(
        Expression::Unary {
            op: UnaryOperator::LogicalNot,
            expr: passes,
        },
        Span::UNDEFINED,
    );
    out.push(
        Statement::Emit(expressions.range_from(start)),
        Span::UNDEFINED,
    );
    out.push(
        Statement::If {
            condition: fails,
            accept: Block::from_vec(vec![Statement::Kill]),
            reject: Block::new(),
        },
        Span::UNDEFINED,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAGMENT: &str = "#version 450
layout(location = 0) out vec4 colour;
layout(location = 1) out vec4 data;
layout(location = 0) in vec4 tint;
void main() {
    colour = tint;
    data = vec4(1.0);
}
";

    fn module() -> Module {
        naga::front::glsl::Frontend::default()
            .parse(
                &naga::front::glsl::Options::from(ShaderStage::Fragment),
                FRAGMENT,
            )
            .expect("fixture fragment parses")
    }

    fn validate(module: &Module) {
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .expect("injected module validates");
    }

    fn kills(block: &Block) -> usize {
        block
            .iter()
            .map(|statement| match statement {
                Statement::Kill => 1,
                Statement::If { accept, reject, .. } => kills(accept) + kills(reject),
                Statement::Block(inner) => kills(inner),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn the_test_discards_before_the_fragment_returns_and_validates() {
        let mut module = module();
        assert!(inject_alpha_test(&mut module, ONE_TENTH_ALPHA));
        validate(&module);
        assert_eq!(kills(&module.entry_points[0].function.body), 1);
        let wgsl = naga::back::wgsl::write_string(
            &module,
            &naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap(),
            naga::back::wgsl::WriterFlags::empty(),
        )
        .unwrap();
        assert!(wgsl.contains("discard"), "{wgsl}");
        assert!(wgsl.contains("> 0.1f"), "{wgsl}");
    }

    #[test]
    fn always_and_never_need_no_comparison() {
        let mut always = module();
        assert!(inject_alpha_test(
            &mut always,
            AlphaTest {
                function: CompareFunction::Always,
                reference: 0.0,
            }
        ));
        assert_eq!(kills(&always.entry_points[0].function.body), 0);
        let mut never = module();
        assert!(inject_alpha_test(
            &mut never,
            AlphaTest {
                function: CompareFunction::Never,
                reference: 0.0,
            }
        ));
        validate(&never);
        assert_eq!(kills(&never.entry_points[0].function.body), 1);
    }
}
