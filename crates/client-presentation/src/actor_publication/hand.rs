//! First-person hand frame publication, including independent main/offhand artwork.

use super::*;
use crate::presentation::equipment::ActorEquipmentInput;

#[cfg(test)]
mod lighting_tests;
mod native_pose;
#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod third_person_tests;
pub(super) use native_pose::NativePoseCache;

/// Builds and publishes the local player's first-person rig for the near-camera pass, or clears
/// it when not in first person, when the look FOV is unavailable, or when no skin resolved.
pub(super) fn publish_hand_rig(
    builder: &mut ActorRigFrameBuilder,
    scene: &mut HandRigScene,
    revision: &mut u64,
    source: Option<HandSource>,
    fov_radians: Option<f32>,
    mut light: HandRigLight,
    partial_tick: f32,
) {
    let (Some(source), Some(fov)) = (source, fov_radians) else {
        scene.clear();
        return;
    };
    let Some(skin) = source.presentation.skin_rgba8.clone() else {
        scene.clear();
        return;
    };
    let submissions = source.submissions();
    let mut atlases = [None, None];
    for (index, entry) in source.items.into_iter().enumerate() {
        if let Some((layer, atlas)) = entry {
            light.java_normal_axes[index + 1] = layer.java_normal_axis.extend(0.0).to_array();
            atlases[index] = Some(atlas);
        }
    }
    let frame = builder.build(partial_tick, None, submissions.into_iter().flatten());
    *revision = revision.wrapping_add(1).max(1);
    if scene.publish(frame, skin, light, fov, *revision) {
        scene.set_item_atlases(atlases);
    }
}

/// The arm's state at `partial_tick` between the rig's last two ticks, as vanilla's first-person
/// pass interpolates it: the swing wraps forward past its end, and an eat or drink use of
/// `consume_ticks` counts from its first using tick.
pub(super) fn hand_progress(
    hand: [client_world::HandPhase; 2],
    consume_ticks: Option<u32>,
    partial_tick: f32,
) -> FirstPersonHand {
    view_presentation::equipment_display::hand_progress(
        hand.map(|phase| view_presentation::equipment_display::HandProgress {
            attack_time: phase.attack_time,
            arm_height: phase.arm_height,
            use_ticks: phase.use_ticks,
        }),
        consume_ticks,
        partial_tick,
    )
}

/// What the first-person pass draws: arm-masked body pose and/or a held item with its atlas page
/// and whether its bone is in camera space.
pub(super) struct HandSource {
    pub(super) presentation: ActorRigPresentation,
    pub(super) body: Option<ActorRigSubmission>,
    pub(super) items: [Option<(FirstPersonItem, HandItemAtlas)>; 2],
    /// View-space hurt tilt, walk bob and sway applied before the rig placement.
    pub(super) motion: Mat4,
    /// Camera from the body's rig frame under Java's empty-hand stack.
    pub(super) java_body_camera: Option<Mat4>,
}

impl HandSource {
    /// Places the exact body/item submissions shared by validation and final construction.
    fn submissions(&self) -> [Option<ActorRigSubmission>; 3] {
        let placement = hand_camera_from_rig(
            self.presentation.authored_scale,
            crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS,
            self.motion,
        );
        let body = self.body.clone().map(|mut body| {
            body.world_from_actor = self.java_body_camera.map_or(placement, |camera| {
                view_presentation::equipment_display::hand_view_placement(self.motion * camera)
            });
            body.texture_layer = 0;
            body
        });
        let items = std::array::from_fn::<_, 2, _>(|index| {
            let (layer, _) = self.items[index].as_ref()?;
            let mut item = layer.presentation.submission.clone();
            item.world_from_actor = match layer.java_camera {
                Some(camera) => {
                    view_presentation::equipment_display::hand_view_placement(self.motion * camera)
                }
                None if layer.camera_space => {
                    view_presentation::equipment_display::hand_view_placement(self.motion)
                }
                None => placement,
            };
            item.texture_layer = layer.presentation.location.layer()
                | render::HAND_ITEM_LAYER_FLAG
                | layer.alpha_mode.texture_layer_flag()
                | if index == 1 {
                    render::HAND_OFFHAND_LAYER_FLAG
                } else {
                    0
                };
            Some(item)
        });
        let [main, off] = items;
        [body, main, off]
    }
}

/// Checks the same bounded near-camera draws without building a second frame.
pub(super) fn is_ready(
    builder: &ActorRigFrameBuilder,
    source: Option<&HandSource>,
    fov: Option<f32>,
) -> bool {
    let (Some(source), Some(fov)) = (source, fov) else {
        return false;
    };
    let Some(skin) = source.presentation.skin_rgba8.as_ref() else {
        return false;
    };
    HandRigScene::accepts_skin_and_fov(skin, fov)
        && source
            .submissions()
            .iter()
            .flatten()
            .any(|draw| builder.can_draw_submission(draw))
}

/// Inputs shared by the built-in hand stacks and authored attachables.
pub(super) struct HandInputs<'a> {
    pub(super) stream: &'a WorldStream,
    pub(super) presentation: ActorRigPresentation,
    pub(super) equipment_input: &'a ActorEquipmentInput,
    pub(super) owner_equipment: &'a ActorEquipmentInput,
    pub(super) consume_ticks: Option<u32>,
    pub(super) item_animation: Option<client_world::AttachableAnimationInput<'static>>,
    pub(super) alpha: f32,
    pub(super) artwork: &'a render::ActorArtworkPages,
    pub(super) motion: Mat4,
    pub(super) sampling_camera: Option<([f32; 2], [f32; 3])>,
}

/// Builds vanilla arms and attachables with the displayed stack and its actual owner.
pub(super) fn vanilla_hand_source(
    mut inputs: HandInputs<'_>,
    equipment: &mut EquipmentRuntime,
    hand: FirstPersonHand,
    native_pose: &mut NativePoseCache,
) -> Option<HandSource> {
    native_pose.apply(&mut inputs);
    let HandInputs {
        stream,
        presentation,
        equipment_input,
        owner_equipment,
        item_animation,
        alpha,
        artwork,
        motion,
        ..
    } = inputs;
    let runtime_id = presentation.submission.input.identity.runtime_id;
    let rig = stream.authority().actor_rig(runtime_id)?;
    let items = std::array::from_fn(|index| {
        let item = [equipment_input.main.as_ref(), equipment_input.off.as_ref()][index]?;
        let modern = item_animation.and_then(|mut render_input| {
            render_input.frame_alpha = alpha;
            let render_input =
                attachable_hand_input(equipment_input, owner_equipment, render_input, index == 1);
            equipment.first_person_attachable(
                &presentation.submission,
                item,
                stream.authority().actor(runtime_id)?,
                &rig,
                render_input,
                None,
            )
        });
        let layer = modern.or_else(|| {
            if index == 0 {
                equipment.first_person_item(&presentation.submission, item, hand)
            } else {
                equipment.first_person_offhand(&presentation.submission, item)
            }
        })?;
        let atlas = item_atlas(&layer, artwork)?;
        Some((layer, atlas))
    });
    let arms = FirstPersonArms::for_hands(
        equipment_input
            .main
            .as_ref()
            .map(|item| item.identifier.as_ref()),
        equipment_input
            .off
            .as_ref()
            .map(|item| item.identifier.as_ref()),
    )
    .with_undrawn_main(items[0].is_some());
    let body = equipment.mask_first_person(&presentation.submission, arms);
    (body.is_some() || items.iter().any(Option::is_some)).then_some(HandSource {
        presentation,
        body,
        items,
        motion,
        java_body_camera: None,
    })
}

/// An outgoing main draw stays idle in its retained item frame; the offhand reads the actual
/// owner's equipment and main-hand use timing.
pub(super) fn attachable_hand_input<'a>(
    rendered: &'a ActorEquipmentInput,
    owner: &'a ActorEquipmentInput,
    timing: client_world::AttachableAnimationInput<'a>,
    off_hand: bool,
) -> client_world::AttachableAnimationInput<'a> {
    let selected = match (rendered.main.as_ref(), owner.main.as_ref()) {
        (Some(rendered), Some(owner)) => {
            rendered.identifier == owner.identifier
                && rendered.damage.unwrap_or(rendered.metadata)
                    == owner.damage.unwrap_or(owner.metadata)
        }
        (None, None) => true,
        _ => false,
    };
    let timing = if off_hand || selected {
        timing
    } else {
        client_world::AttachableAnimationInput {
            animation_frame: 0,
            use_elapsed_ticks: None,
            hand_charged: false,
            ..timing
        }
    };
    let equipment = if off_hand { owner } else { rendered };
    equipment.attachable_input(timing.for_hand(off_hand))
}

/// The artwork page an item layer samples, as the hand pass binds it.
pub(super) fn item_atlas(
    layer: &FirstPersonItem,
    artwork: &render::ActorArtworkPages,
) -> Option<HandItemAtlas> {
    let page = usize::from(layer.presentation.location.page()).checked_sub(1)?;
    let page = artwork.pages().get(page)?;
    if layer.presentation.location.layer() >= page.layers() {
        return None;
    }
    let (width, height) = page.dimensions();
    Some(HandItemAtlas {
        width,
        height,
        layers: page.layers(),
        rgba8: page.shared_pixels(),
    })
}

/// Vanilla's hand stack order: hurt tilt, walk bob, then sway about X and Y.
pub(super) fn hand_motion_matrix(
    motion: &view_presentation::camera::FirstPersonHandMotion,
) -> Mat4 {
    motion.matrix()
}

/// Java sets fixed lights after the camera effects and world look, before the lagging hand sway.
pub(super) fn java_light_matrix(
    motion: Option<&view_presentation::camera::FirstPersonHandMotion>,
    look: bevy::math::Quat,
) -> Mat4 {
    let (yaw, pitch, _) = look.to_euler(bevy::math::EulerRot::YXZ);
    let effects = motion.map_or(Mat4::IDENTITY, |motion| motion.hurt * motion.bob.matrix());
    effects * Mat4::from_rotation_x(-pitch) * Mat4::from_rotation_y(std::f32::consts::PI - yaw)
}

/// Selects the same arm and item layers for readiness and final first-person publication.
pub(super) fn source(
    inputs: HandInputs<'_>,
    java_mode: bool,
    equipment: &mut EquipmentRuntime,
    cache: &mut java::HandCache,
) -> Option<HandSource> {
    if java_mode
        && let Some(source) = java::hand_source(
            HandInputs {
                presentation: inputs.presentation.clone(),
                ..inputs
            },
            equipment,
            cache,
        )
    {
        return Some(source);
    }
    let runtime_id = inputs.presentation.submission.input.identity.runtime_id;
    let hand = inputs.stream.authority().actor_rig(runtime_id).map_or(
        FirstPersonHand {
            swing: 0.0,
            equip: 1.0,
            consume: None,
        },
        |rig| {
            let mut hand = hand_progress(rig.hand, inputs.consume_ticks, inputs.alpha);
            let alpha = rig.java.local_swing_alpha.unwrap_or(inputs.alpha);
            hand.swing = hand_progress(rig.hand, None, alpha).swing;
            hand
        },
    );
    vanilla_hand_source(inputs, equipment, hand, &mut cache.native_pose)
}

type AnimationFrameKey = (u32, Option<u32>, u32, bool);

/// Local hand source inputs whose changes require reselection after interaction admission.
#[derive(PartialEq)]
pub(super) struct SourceKey {
    lifetime: client_world::ActorLifetimeId,
    bones: [(usize, usize); 2],
    hand: [client_world::HandPhase; 2],
    java: client_world::JavaMotion,
    sampled_swing: [f32; 2],
    sampled_body_yaw: f32,
    equipped: Option<client_world::JavaHeldItem>,
    use_frame: (Option<u32>, Option<AnimationFrameKey>),
}

/// Retains unchanged hand layers while admitted actions update their sampled swing and equip.
pub(super) fn source_key(
    stream: &WorldStream,
    consume: Option<u32>,
    animation: Option<client_world::AttachableAnimationInput<'_>>,
    alpha: f32,
) -> Option<SourceKey> {
    let rig = stream
        .authority()
        .actor_rig(stream.local_player_runtime_id())?;
    let mut java = rig.java;
    java.local_swing_alpha = None;
    java.body_frame_alpha = None;
    let swing_alpha = rig.java.local_swing_alpha.unwrap_or(alpha);
    Some(SourceKey {
        sampled_body_yaw: rig.java.body_yaw_at(alpha),
        sampled_swing: [
            hand_progress(rig.hand, None, swing_alpha).swing,
            rig.java.swing_progress(alpha),
        ],
        lifetime: rig.actor,
        bones: [rig.previous, rig.current].map(|bones| (bones.as_ptr() as usize, bones.len())),
        hand: rig.hand,
        java,
        equipped: rig.java_equipped.cloned(),
        use_frame: (
            consume,
            animation.map(|input| {
                (
                    input.animation_frame,
                    input.use_elapsed_ticks,
                    input.max_use_ticks,
                    input.hand_charged,
                )
            }),
        ),
    })
}
