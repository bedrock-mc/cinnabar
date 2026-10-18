use super::*;

#[test]
fn owner_held_scale_composes_with_authored_models_in_each_view() {
    let (mut runtime, _) = pack_runtime(held_pack());
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem"), Box::from("leftItem")];
    let mut rig = owner_rig(&owner, &names);
    let item = held("test:held").main.unwrap();
    for first_person in [false, true] {
        for off_hand in [false, true] {
            let samples = [1.0, 2.0].map(|scale| {
                rig.held_item_scale = Some(scale);
                let draw = runtime
                    .first_person_attachable(
                        &body,
                        &item,
                        &owner,
                        &rig,
                        actor_animation::AttachableAnimationInput {
                            first_person,
                            off_hand,
                            frame_alpha: 0.5,
                            ..Default::default()
                        },
                        None,
                    )
                    .unwrap();
                draw.presentation.submission.input.current_bones[0]
            });
            let factor = if off_hand { 1.0 } else { 2.0 };
            for axis in 0..3 {
                assert!(
                    (samples[1].translation_scale[axis]
                        - samples[0].translation_scale[axis] * factor)
                        .abs()
                        < 1e-6
                );
                assert!(
                    (samples[1].axis_scale[axis] - samples[0].axis_scale[axis] * factor).abs()
                        < 1e-6
                );
            }
        }
    }
}
