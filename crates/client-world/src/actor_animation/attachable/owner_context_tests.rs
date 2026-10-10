//! Static owners retain equipment and property inputs for their attachables.
use super::*;
use crate::actor_store::properties::{PropertyDefinition, PropertyKind};
use assets::{MolangCall, MolangOp, MolangSymbol, MolangSymbolKind};

/// Selects one animation from an owning-entity query with a single argument.
fn query_fixture(property: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.molang_symbols[1].identifier = if property {
        "query.property".into()
    } else {
        "query.has_armor_slot".into()
    };
    let argument = if property {
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::String,
            identifier: "test:style".into(),
        });
        compiled.molang_symbols = symbols.into();
        MolangOp::PushString(4)
    } else {
        MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap())
    };
    compiled.molang_ops = [
        MolangOp::LoadVariable(3),
        MolangOp::Arrow(4),
        argument,
        MolangOp::CallQuery(MolangCall {
            symbol: 1,
            arguments: 1,
        }),
    ]
    .into();
    compiled.molang_expressions[0].op_count = 4;
    compiled.molang_expressions[0].max_stack = 1;
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

/// Completes a non-animated owner tick so equipment queries cannot rely on a pose frame.
fn owner_store(
    assets: &Arc<RuntimeEntityAssets>,
    owner: &ActorSnapshot,
    context: &ActorTickContext,
) -> ActorAnimationStore {
    let mut store = ActorAnimationStore::new(Some(Arc::clone(assets)));
    let lifetime = tests::owner_rig().actor;
    let state = resolve_binding(assets, &store.layout, owner, 0, 0).unwrap();
    store.rigs.insert(lifetime, state);
    store.runtime_to_lifetime.insert(owner.runtime_id, lifetime);
    store.evaluate_tick(
        &HashMap::from([(owner.runtime_id, owner.clone())]),
        None,
        None,
        PoseStep {
            evaluate: false,
            reset_motion_history: true,
            refresh_view: false,
        },
        |_| context.clone(),
    );
    store
}

/// Reads the owning-entity predicate through an actual worn animation weight.
fn translation(property: bool, context: &ActorTickContext, owner: &ActorSnapshot) -> f32 {
    let assets = query_fixture(property);
    let store = owner_store(&assets, owner, context);
    let owner_rig = store.snapshots().next().unwrap();
    let mut runtime = AttachablesRuntime::new(assets);
    runtime
        .evaluate(
            "minecraft:test_item",
            owner,
            &owner_rig,
            AttachableAnimationInput {
                worn: true,
                ..Default::default()
            },
        )
        .unwrap()
        .pose[0]
        .translation_scale[0]
}

#[test]
fn worn_attachable_reads_its_static_owners_armour() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut context = ActorTickContext::default();
    assert_eq!(translation(false, &context, &owner), 0.0);
    context.armor[0] = Some(crate::actor_animation::tick::WornArmor {
        item: Arc::from("test:helmet"),
        dye_rgb: None,
    });
    assert_eq!(translation(false, &context, &owner), -16.0);
}

#[test]
fn worn_attachable_reads_its_static_owners_synced_property() {
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let context = ActorTickContext {
        properties: Some(Arc::from([PropertyDefinition {
            name: "test:style".into(),
            kind: PropertyKind::Number,
            default: 0.0,
        }])),
        ..Default::default()
    };
    assert_eq!(translation(true, &context, &owner), 0.0);
    owner.int_properties.insert(0, 1);
    assert_eq!(translation(true, &context, &owner), -16.0);
}

#[test]
fn worn_attachable_recognizes_its_local_player_owner() {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.molang_symbols[1].identifier = "query.is_local_player".into();
    compiled.molang_ops = [
        MolangOp::LoadVariable(3),
        MolangOp::Arrow(3),
        MolangOp::LoadQuery(1),
    ]
    .into();
    compiled.molang_expressions[0].op_count = 3;
    compiled.molang_expressions[0].max_stack = 1;
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    owner.kind = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "fixture".into(),
    };
    let context = ActorTickContext {
        is_local_player: true,
        ..Default::default()
    };
    let store = owner_store(&assets, &owner, &context);
    let rig = store.snapshots().next().unwrap();
    let mut runtime = AttachablesRuntime::new(assets);
    let worn = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &rig,
            AttachableAnimationInput {
                worn: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(worn.pose[0].translation_scale[0], -16.0);
}
