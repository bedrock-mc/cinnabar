use super::*;
use assets::{CompiledMolangExpression, MolangOp, MolangSymbol, MolangSymbolKind};

fn scalar(value: f32) -> assets::EntityGeometryScalar {
    assets::EntityGeometryScalar::new(value).unwrap()
}

pub(super) fn compiled_fixture(owner_reference: bool) -> assets::CompiledEntityAssets {
    let mut compiled = tests::compiled_fixture();
    compiled.rig_bindings[0].pre_animation = None;
    compiled.rig_animations[0].weight = Some(0);
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.animation_keyframes[0].value = [scalar(16.0), scalar(0.0), scalar(0.0)];
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.is_shaking"),
        (MolangSymbolKind::Variable, "context.is_first_person"),
        (MolangSymbolKind::Variable, "context.owning_entity"),
    ]
    .into_iter()
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect();
    compiled.molang_expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: 5,
        max_stack: 2,
    }]
    .into_boxed_slice();
    compiled.molang_ops = vec![
        if owner_reference {
            MolangOp::LoadVariable(3)
        } else {
            MolangOp::Push(scalar(1.0))
        },
        MolangOp::Arrow(3),
        MolangOp::LoadQuery(1),
        MolangOp::LoadVariable(2),
        MolangOp::Multiply,
    ]
    .into_boxed_slice();
    compiled
}

fn fixture(owner_reference: bool) -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(compiled_fixture(owner_reference)).unwrap())
}

#[test]
fn attachable_owner_reference_selects_shaking_animation_only_in_first_person() {
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::from([(
        0,
        ActorMetadataValue::Flags(1 << 40),
    )]));
    let mut runtime = AttachablesRuntime::new(fixture(true));
    let rig = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..AttachableAnimationInput::default()
    };
    let pose = runtime
        .evaluate("minecraft:test_item", &owner, &rig, input)
        .unwrap();
    assert_eq!(pose.pose[0].translation_scale[0], -16.0);
    let pose = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &rig,
            AttachableAnimationInput {
                first_person: false,
                ..input
            },
        )
        .unwrap();
    assert_eq!(pose.pose[0].translation_scale[0], 0.0);
    owner.metadata.clear();
    let pose = runtime
        .evaluate("minecraft:test_item", &owner, &rig, input)
        .unwrap();
    assert_eq!(pose.pose[0].translation_scale[0], 0.0);
}

#[test]
fn attachable_numeric_arrow_operand_cannot_impersonate_its_owner() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::from([(
        0,
        ActorMetadataValue::Flags(1 << 40),
    )]));
    let mut runtime = AttachablesRuntime::new(fixture(false));
    let pose = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &tests::owner_rig(),
            AttachableAnimationInput {
                first_person: true,
                ..AttachableAnimationInput::default()
            },
        )
        .unwrap();
    assert_eq!(pose.pose[0].translation_scale[0], 0.0);
}
