//! Molang program validation: function arity and stack contracts.

use super::{
    entity::{self, CompiledMolangExpression, MolangOp},
    suite::carrier_v4_fixture,
};

#[test]
fn every_math_function_and_collection_selection_validates_at_its_arity() {
    use entity::{MolangEaseCurve, MolangEaseMode, MolangFunction};
    let mut functions = vec![
        MolangFunction::Abs,
        MolangFunction::Acos,
        MolangFunction::Asin,
        MolangFunction::Atan,
        MolangFunction::Atan2,
        MolangFunction::Ceil,
        MolangFunction::Clamp,
        MolangFunction::CopySign,
        MolangFunction::Cos,
        MolangFunction::DieRoll,
        MolangFunction::DieRollInteger,
        MolangFunction::Exp,
        MolangFunction::Floor,
        MolangFunction::HermiteBlend,
        MolangFunction::InverseLerp,
        MolangFunction::Lerp,
        MolangFunction::LerpRotate,
        MolangFunction::Ln,
        MolangFunction::Max,
        MolangFunction::Min,
        MolangFunction::MinAngle,
        MolangFunction::Mod,
        MolangFunction::Pow,
        MolangFunction::Random,
        MolangFunction::RandomInteger,
        MolangFunction::Round,
        MolangFunction::Sign,
        MolangFunction::Sin,
        MolangFunction::Sqrt,
        MolangFunction::Trunc,
    ];
    functions.push(MolangFunction::Ease(
        MolangEaseCurve::Elastic,
        MolangEaseMode::InOut,
    ));
    let one = entity::EntityGeometryScalar::new(1.0).unwrap();
    for function in functions {
        let arity = function.arity();
        let mut compiled = carrier_v4_fixture();
        let mut ops = vec![MolangOp::Push(one); arity];
        ops.push(MolangOp::Call(function));
        compiled.molang_expressions[0].op_count = ops.len() as u16;
        compiled.molang_expressions[0].max_stack = arity.max(1) as u8;
        compiled.molang_ops = ops.into_boxed_slice();
        assert!(compiled.validate().is_ok(), "function {function:?}");
        let mut short = compiled.clone();
        short.molang_ops = short.molang_ops[1..].into();
        short.molang_expressions[0].op_count -= 1;
        assert!(short.validate().is_err(), "underfed {function:?}");
    }
    let mut compiled = carrier_v4_fixture();
    compiled.molang_ops = vec![MolangOp::Push(one), MolangOp::SelectCollection(0)].into();
    compiled.molang_expressions[0].op_count = 2;
    assert!(compiled.validate().is_ok());
}

#[test]
fn carrier_v4_requires_exact_valid_molang_program_stack_contracts() {
    let mut zero = carrier_v4_fixture();
    zero.molang_expressions[0] = CompiledMolangExpression {
        first_op: 0,
        op_count: 0,
        max_stack: 0,
    };
    zero.molang_ops = Box::new([]);
    assert!(zero.validate().is_err());

    let mut underflow = carrier_v4_fixture();
    underflow.molang_ops = vec![MolangOp::Add].into_boxed_slice();
    underflow.molang_expressions[0].max_stack = 0;
    assert!(underflow.validate().is_err());

    let mut final_two = carrier_v4_fixture();
    final_two.molang_ops = vec![final_two.molang_ops[0]; 2].into_boxed_slice();
    final_two.molang_expressions[0].op_count = 2;
    final_two.molang_expressions[0].max_stack = 2;
    assert!(final_two.validate().is_err());

    let mut dishonest = carrier_v4_fixture();
    dishonest.molang_expressions[0].max_stack = 2;
    assert!(dishonest.validate().is_err());

    let mut exact_depth = carrier_v4_fixture();
    exact_depth.molang_ops = std::iter::repeat_n(exact_depth.molang_ops[0], 32)
        .chain(std::iter::repeat_n(MolangOp::Add, 31))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    exact_depth.molang_expressions[0].op_count = 63;
    exact_depth.molang_expressions[0].max_stack = 32;
    assert!(exact_depth.validate().is_ok());

    let mut dishonest_depth = exact_depth;
    dishonest_depth.molang_expressions[0].max_stack = 31;
    assert!(dishonest_depth.validate().is_err());
}
