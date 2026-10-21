use super::*;
use actor_animation::{ActorAnimationStore, ActorTickContext};
use assets::{EntityAnimationProperty, EntityAssetKind, EntityGeometryScalar, MolangOp};
use render_api::CombatVisuals;
use render_model::{
    RenderBoneTransform,
    equipment::{ItemDisplay, attach_to_bone, held_sprite_display},
    java_animation::{self as java, JavaBipedInput, JavaHeldItem, JavaItemMesh},
};
use std::sync::Arc;

const FRACTIONS: [f32; 4] = [0.0, 0.25, 0.5, 0.75];
const ARM: usize = 1;
const ITEM: usize = 2;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Legacy112,
    Modern,
    Java17,
}

#[derive(Clone, Copy)]
struct Frame {
    progress: f32,
    arm: [f32; 4],
    item: [f32; 4],
}

/// Builds a small authored forward strike with an arm-parented held-item bone.
fn swing_assets() -> Arc<assets::RuntimeEntityAssets> {
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    let mut compiled = actor_animation::test_support::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols[1].identifier = "animation.player.attack.rotations".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = Some(3);
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 2;
    let bone = compiled.geometries[0].bones[0].clone();
    compiled.geometries[0].bones = [
        ("body", None, [0.0, 24.0, 0.0]),
        ("rightarm", Some("body"), [5.0, 22.0, 0.0]),
        ("rightitem", Some("rightarm"), [6.0, 15.0, 1.0]),
    ]
    .into_iter()
    .map(|(name, parent, pivot)| assets::EntityGeometryBone {
        name: name.into(),
        parent: parent.map(Into::into),
        pivot: Some(pivot.map(scalar)),
        ..bone.clone()
    })
    .collect();
    compiled.animation_channels[0].bone_name = Some("rightarm".into());
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].value = [scalar(0.0); 3];
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    compiled.molang_symbols = [
        (assets::MolangSymbolKind::Name, "wield"),
        (assets::MolangSymbolKind::Variable, "variable.attack_time"),
    ]
    .into_iter()
    .map(|(kind, identifier)| assets::MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(1),
        MolangOp::Push(scalar(180.0)),
        MolangOp::Multiply,
        MolangOp::Call(assets::MolangFunction::Sin),
        MolangOp::Push(scalar(-80.0)),
        MolangOp::Multiply,
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![assets::CompiledMolangExpression {
        first_op: 0,
        op_count: compiled.molang_ops.len() as u16,
        max_stack: 2,
    }]
    .into_boxed_slice();
    Arc::new(
        assets::RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    )
}

/// Samples remote movement and one swing tick, using each mode's production pose and grip.
fn frames(mode: Mode, interval: u32, wrapped: bool) -> Vec<Frame> {
    let wire_pitch = |pitch| {
        if wrapped && pitch < 0.0 {
            pitch + 360.0
        } else {
            pitch
        }
    };
    let mut store = player(wire_pitch(-15.0));
    let mut animation = ActorAnimationStore::with_assets(swing_assets());
    animation.insert(1, 0, store.get(1).unwrap());
    let mut sequence = 1;
    let mut frames = Vec::new();
    for tick in 0..48 {
        if tick % 8 == 0 {
            sequence += 1;
            let pitch = if tick % 16 == 0 { -15.0 } else { 15.0 };
            move_pitch(&mut store, sequence, wire_pitch(pitch), false, false);
        }
        if tick % interval == 0 {
            sequence += 1;
            assert_eq!(
                store.apply_item_actor(
                    1,
                    sequence,
                    protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
                        actor_runtime_ids: Arc::from([1]),
                        kind: protocol::ActorActionKind::SwingArm,
                        data: 0.0,
                        swing_source: None,
                    })
                ),
                ActorApplyResult::Updated
            );
            animation.start_swing(
                1,
                actor_animation::ACTOR_SWING_TICKS,
                actor_animation::ACTOR_SWING_TICKS,
            );
        }
        store.advance_interpolation_ticks(1);
        animation.advance_tick(&store.actors, None, None, true, true, |_| {
            ActorTickContext {
                combat_visuals: match mode {
                    Mode::Modern => CombatVisuals::Modern,
                    _ => CombatVisuals::Legacy112,
                },
                main_hand: Some("minecraft:diamond_sword".into()),
                ..Default::default()
            }
        });
        let rig = animation.get(1).unwrap();
        assert!(rig.pose_evaluated);
        let actor = store.get(1).unwrap();
        for alpha in FRACTIONS {
            let progress = match mode {
                Mode::Java17 => rig.java.swing_progress(alpha),
                _ => {
                    rig.item_animation[0]
                        .interpolate(rig.item_animation[1], alpha)
                        .attack_time
                }
            };
            let (arm, item) = match mode {
                Mode::Java17 => {
                    let pose = java::java_biped(&JavaBipedInput {
                        head_pitch: actor.previous_pose.pitch
                            + wrap_degrees(actor.pitch - actor.previous_pose.pitch) * alpha,
                        swing: progress,
                        held_right: 1,
                        age: tick as f32 + alpha,
                        ..Default::default()
                    });
                    let pivot = pose.right_arm.point * 0.0;
                    let (rotation, _) = pose
                        .right_arm
                        .rig_bone(java::JavaBiped::REST.right_arm, pivot);
                    let arm = RenderBoneTransform {
                        rotation: rotation.to_array(),
                        translation_scale: [0.0, 0.0, 0.0, 1.0],
                        axis_scale: render_model::UNIT_AXIS_SCALE,
                    };
                    let display = ItemDisplay::from_matrix(java::third_person_item(
                        JavaHeldItem::Tool {
                            rotate_around: false,
                            blocking: false,
                        },
                        JavaItemMesh::Sprite,
                    ));
                    (arm.rotation, attach_to_bone(arm, display).unwrap().rotation)
                }
                _ => {
                    let bones = animation.retargeted_pose(1, alpha, &[]).unwrap();
                    let item = bones[ITEM];
                    let hand = RenderBoneTransform::from_model_space_scaled(
                        item.rotation,
                        item.translation_scale,
                        item.axis_scale,
                    )
                    .unwrap();
                    (
                        bones[ARM].rotation,
                        attach_to_bone(hand, held_sprite_display(true))
                            .unwrap()
                            .rotation,
                    )
                }
            };
            frames.push(Frame {
                progress,
                arm,
                item,
            });
        }
    }
    frames
}

/// Measures the shortest angular distance between unit rotations, regardless of quaternion sign.
fn angle(from: [f32; 4], to: [f32; 4]) -> f32 {
    let dot = from.into_iter().zip(to).map(|(a, b)| a * b).sum::<f32>();
    2.0 * dot.abs().clamp(0.0, 1.0).acos()
}

#[test]
fn repeated_remote_sword_swings_keep_wrapped_pitch_on_the_same_forward_arc_in_every_mode() {
    for mode in [Mode::Legacy112, Mode::Modern, Mode::Java17] {
        for interval in 3..=6 {
            let expected = frames(mode, interval, false);
            let actual = frames(mode, interval, true);
            for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                assert_eq!(actual.progress, expected.progress);
                assert!((0.0..1.0).contains(&actual.progress));
                assert!(
                    angle(actual.arm, expected.arm) < 0.002,
                    "{mode:?}, interval {interval}, frame {index}: arm pitch changed the strike"
                );
                assert!(
                    angle(actual.item, expected.item) < 0.002,
                    "{mode:?}, interval {interval}, frame {index}: held sword changed its arc"
                );
            }
            for tick in actual.as_chunks::<{ FRACTIONS.len() }>().0 {
                assert!(
                    tick.windows(2)
                        .all(|pair| pair[1].progress >= pair[0].progress)
                );
            }
            for pair in actual.windows(2) {
                assert!(
                    pair[1].progress >= pair[0].progress || pair[1].progress == 0.0,
                    "{mode:?}: progress reversed within a swing"
                );
                assert!(
                    angle(pair[0].arm, pair[1].arm) < 1.25,
                    "{mode:?}: arm jumped between frames"
                );
                assert!(
                    angle(pair[0].item, pair[1].item) < 1.25,
                    "{mode:?}: sword jumped between frames"
                );
            }
            let rest = actual[0];
            for frame in actual {
                assert!(
                    angle(rest.arm, frame.arm) < 2.4,
                    "{mode:?}: arm left the forward strike range"
                );
                assert!(
                    angle(rest.item, frame.item) < 2.4,
                    "{mode:?}: sword left the forward strike range"
                );
            }
        }
    }
}
