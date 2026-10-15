//! Attachable scripts and inherited spear inputs remain owned by their authors.
use super::*;
use assets::{MolangSymbol, MolangSymbolKind, RuntimeEntityAssets};

#[test]
fn player_spear_completion_preserves_authored_and_inherited_attachable_poses() {
    let identifier = "minecraft:iron_spear";
    assert!(
        assets::facts::ItemFacts::builtin()
            .unwrap()
            .by_identifier(identifier)
            .is_some()
    );
    let variable = "variable.fp_melee_spear_attack_item_position_z";
    for inherited in [false, true] {
        let mut compiled = crate::test_support::compiled_fixture();
        compiled.symbols[4].identifier = identifier.into();
        compiled.molang_symbols[7].identifier = variable.into();
        if inherited {
            compiled.rig_bindings[0].pre_animation = None;
        }
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.melee_spear_equipped".into(),
        });
        compiled.molang_symbols = symbols.into();
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let layout = VariableLayout::new(&assets);
        let mut values = layout.fresh(1);
        values.set(layout.named_slot(&assets, variable), 16.0);
        values.set(
            layout.named_slot(&assets, "variable.melee_spear_equipped"),
            0.0,
        );
        let mut rig: ActorRigSnapshot<'_> = super::tests::owner_rig();
        rig.animation_variables = ActorAnimationVariables::new(Some(&assets), &values, 20, 0);
        let mut owner = crate::tests::actor_with_metadata(HashMap::new());
        owner.kind = protocol::ActorKind::Player {
            uuid: [0; 16],
            username: "Fixture".into(),
        };
        let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
        let snapshot = runtime
            .evaluate(
                identifier,
                &owner,
                &rig,
                AttachableAnimationInput {
                    first_person: true,
                    frame_alpha: 0.5,
                    use_elapsed_ticks: Some(5),
                    max_use_ticks: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        let expected = if inherited { 16.0 } else { 0.45 };
        assert!(
            (snapshot.pose[0].translation_scale[1] - expected).abs() < 1.0e-6,
            "inherited={inherited}: {:?}",
            snapshot.pose[0]
        );
    }
}
