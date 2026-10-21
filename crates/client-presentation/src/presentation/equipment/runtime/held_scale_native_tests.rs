use super::*;

#[test]
fn owner_held_scale_applies_once_to_native_weapon_stages() {
    let identifier = "minecraft:bow";
    let (mut runtime, pages) =
        startup_runtime(fixture(identifier, "geometry.bow_standby", 10, true));
    install(
        &mut runtime,
        pages,
        fixture(identifier, "geometry.bow_standby", 80, true),
    );
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem"), Box::from("leftItem")];
    let mut rig = owner_rig(&owner, &names);
    for off in [false, true] {
        let samples = [1.0, 2.0].map(|scale| {
            rig.held_item_scale = Some(scale);
            let stack = item(identifier);
            let input = ActorEquipmentInput {
                main: (!off).then_some(stack.clone()),
                off: off.then_some(stack),
                ..Default::default()
            };
            let layers = runtime.layers_for(
                &body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 0.5,
                    delta_seconds: 0.016,
                }),
            );
            assert_eq!(layers.len(), 1);
            layers[0].submission.input.current_bones[0]
        });
        let factor = if off { 1.0 } else { 2.0 };
        assert_eq!(
            samples[0].translation_scale[..3],
            samples[1].translation_scale[..3]
        );
        assert!(
            (samples[1].translation_scale[3] - samples[0].translation_scale[3] * factor).abs()
                < 1e-6
        );
    }
    for elapsed in [None, Some(15)] {
        let samples = [1.0, 2.0].map(|scale| {
            runtime
                .first_person_item_with_use(
                    &body,
                    &item(identifier),
                    FirstPersonHand {
                        swing: 0.3,
                        equip: 0.8,
                        consume: None,
                        aspect: 1.0,
                    },
                    render_api::DEFAULT_COMBAT_VISUALS,
                    AttachableAnimationInput {
                        first_person: true,
                        held_item_scale: Some(scale),
                        use_elapsed_ticks: elapsed,
                        max_use_ticks: 100,
                        ..Default::default()
                    },
                )
                .unwrap()
                .presentation
                .submission
                .input
                .current_bones[0]
        });
        assert_eq!(
            samples[0].translation_scale[..3],
            samples[1].translation_scale[..3]
        );
        assert!(
            (samples[1].translation_scale[3] - samples[0].translation_scale[3] * 2.0).abs() < 1e-6
        );
    }
}
