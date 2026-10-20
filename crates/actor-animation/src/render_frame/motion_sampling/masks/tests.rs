use super::*;

/// Creates named custom channels covering dependent axes and conditional override clips.
fn catalog(
    override_previous: bool,
    entity_rotation: bool,
    property: EntityAnimationProperty,
) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("fixture:clip_masks");
    compiled.animation_clips[0].override_previous = override_previous;
    compiled.animation_channels[0].property = property;
    compiled.animation_channels[0].bone_name = Some("rightitem".into());
    compiled.animation_channels[0].rotation_relative_to_entity = entity_rotation;
    compiled.animation_keyframes[0].expressions = [Some(3), None, Some(1)];
    compiled.animation_keyframes[0].value[1] = crate::test_support::scalar(2.0);
    compiled.rig_animations[0].weight = Some(3);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn prepared_masks_match_conditional_named_override_rotation_and_scale_unions() {
    for override_previous in [false, true] {
        for entity_rotation in [false, true] {
            for property in [
                EntityAnimationProperty::Translation,
                EntityAnimationProperty::Rotation,
                EntityAnimationProperty::Scale,
            ] {
                let assets = catalog(override_previous, entity_rotation, property);
                let layout = VariableLayout::new(&assets);
                let sampling = MotionSampling::new(&assets, &layout, 0, 0, &[], &[]);
                let (_, names) = resolve_bones(&assets, 0).unwrap();
                for clips in [vec![], vec![weighted(0)], vec![weighted(0), weighted(0)]] {
                    for geometry in [None, Some(0)] {
                        assert_eq!(
                            sampling.mask(&assets, &names, clips.iter().copied(), geometry),
                            sampling.mask_unbound(&assets, &names, clips.iter().copied(), geometry)
                        );
                    }
                }
                let before = pose::name_lookups();
                let _ = sampling.mask(&assets, &names, [weighted(0)], None);
                assert_eq!(
                    pose::name_lookups(),
                    before,
                    "a bound union must not resolve channel names"
                );
            }
        }
    }
}

#[test]
fn dynamic_unbound_clip_unions_keep_the_reference_path() {
    let assets = catalog(false, false, EntityAnimationProperty::Translation);
    let layout = VariableLayout::new(&assets);
    let sampling = MotionSampling::new(&assets, &layout, 0, 0, &[], &[]).reference_masks();
    let (_, names) = resolve_bones(&assets, 0).unwrap();
    assert_eq!(
        sampling.mask(&assets, &names, [weighted(0)], None),
        sampling.mask_unbound(&assets, &names, [weighted(0)], None)
    );
}
