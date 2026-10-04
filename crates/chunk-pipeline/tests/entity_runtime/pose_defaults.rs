//! Native BoneOrientation defaults are part of Molang `this`, not animation deltas.
use super::*;

fn with_this_channel(
    compiled: &mut CompiledEntityAssets,
    bone: u32,
    property: EntityAnimationProperty,
    target: [f32; 3],
) {
    let first_expression = compiled.molang_expressions.len() as u32;
    let mut expressions = compiled.molang_expressions.to_vec();
    let mut ops = compiled.molang_ops.to_vec();
    for value in target {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 3,
            max_stack: 2,
        });
        ops.extend([
            MolangOp::Push(scalar(value)),
            MolangOp::LoadThis,
            MolangOp::Subtract,
        ]);
    }
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.animation_channels[0].bone = bone;
    compiled.animation_channels[0].property = property;
    for frame in &mut compiled.animation_keyframes {
        frame.value = [scalar(0.0); 3];
        frame.expressions = std::array::from_fn(|axis| Some(first_expression + axis as u32));
    }
}

#[test]
fn native_this_keeps_polar_bear_body_and_children_at_the_authored_idle_pivots() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    // Pinned polar-bear body pivot and idle target from animation.polarbear.move.
    compiled.geometries[0].bones[0].pivot = Some([scalar(-2.0), scalar(15.0), scalar(12.0)]);
    compiled.geometries[1].bones[0].pivot = Some([scalar(0.0), scalar(14.0), scalar(-16.0)]);
    with_this_channel(
        &mut compiled,
        0,
        EntityAnimationProperty::Translation,
        [-2.0, -9.0, 12.0],
    );
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    let rest = stream.actor_rig(42).unwrap().rest.to_vec();
    stream.advance_actor_interpolation_ticks(2);
    let rig = stream.actor_rig(42).unwrap();
    assert_eq!(
        rig.current, rest,
        "idle body expression must not sink the whole hierarchy"
    );
    assert_eq!(rig.current[0].translation_scale[..3], [2.0, 15.0, 12.0]);
    assert_eq!(rig.current[1].translation_scale[..3], [0.0, 14.0, -16.0]);
}

#[test]
fn native_this_reads_parent_relative_position_and_authored_x_before_reflection() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.geometries[0].bones[0].pivot = Some([scalar(2.0), scalar(15.0), scalar(12.0)]);
    compiled.geometries[1].bones[0].pivot = Some([scalar(5.0), scalar(14.0), scalar(-16.0)]);
    with_this_channel(
        &mut compiled,
        1,
        EntityAnimationProperty::Translation,
        [3.0, -1.0, -28.0],
    );
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    let rest = stream.actor_rig(42).unwrap().rest.to_vec();
    stream.advance_actor_interpolation_ticks(2);
    assert_eq!(stream.actor_rig(42).unwrap().current, rest);
}

#[test]
fn native_this_includes_the_contribution_from_an_earlier_channel() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    with_this_channel(
        &mut compiled,
        0,
        EntityAnimationProperty::Translation,
        [1.0, -24.0, 0.0],
    );
    let mut target_channel = compiled.animation_channels[0];
    target_channel.first_keyframe = 1;
    let mut frames = vec![EntityAnimationKeyframe {
        time_seconds: scalar(0.0),
        value: [scalar(0.0), scalar(3.0), scalar(0.0)],
        interpolation: EntityAnimationInterpolation::Linear,
        expressions: [None; 3],
    }];
    frames.extend(compiled.animation_keyframes.iter().cloned());
    compiled.animation_keyframes = frames.into();
    compiled.animation_channels = vec![
        EntityAnimationChannel {
            bone: 0,
            property: EntityAnimationProperty::Translation,
            first_keyframe: 0,
            keyframe_count: 1,
            rotation_relative_to_entity: false,
        },
        target_channel,
    ]
    .into();
    compiled.animation_clips[0].channel_count = 2;
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    let rest = stream.actor_rig(42).unwrap().rest.to_vec();
    stream.advance_actor_interpolation_ticks(2);
    assert_eq!(stream.actor_rig(42).unwrap().current, rest);
}

#[test]
fn native_this_cancels_default_rotation_without_resetting_parent_position() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.geometries[0].bones[0].rotation = Some([scalar(35.0), scalar(-20.0), scalar(12.0)]);
    with_this_channel(
        &mut compiled,
        0,
        EntityAnimationProperty::Rotation,
        [0.0; 3],
    );
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(2);
    let rig = stream.actor_rig(42).unwrap();
    assert_eq!(rig.current[0].rotation, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(rig.current[1].translation_scale[..3], [0.0, 2.0, 0.0]);
}
