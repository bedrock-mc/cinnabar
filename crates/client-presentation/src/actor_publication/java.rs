//! Java 1.7 player animation at publication: Java's pose, body yaw and first-person hand
//! replace vanilla's wherever Java has the posture.

use std::sync::Arc;
use view_presentation::equipment_display::{FirstPersonArms, FirstPersonHand};

use bevy::math::Vec3;
use chunk_pipeline::WorldStream;
use client_world::{ActorRigSnapshot, ActorSnapshot, BoneTransform, JavaHeldItem, SkinRenderLayer};
use render_model::{
    RenderBoneTransform,
    java_animation::{
        self as java, JavaBiped, JavaBipedInput, JavaCapeInput, JavaHand, JavaUse, is_java_sword,
    },
};

use super::hand::{HandInputs, HandSource, hand_progress, item_atlas, vanilla_hand_source};
use crate::presentation::{
    actors::{ActorRigPresentation, convert_bones, lerp_degrees, wrap_degrees},
    equipment::{
        ActorEquipmentInput, EquipmentRuntime, WornItem, java_draws_attachable, remote_input,
    },
};

const BOW: &str = "minecraft:bow";
const BOW_USE_DURATION: u32 = 72_000;
const FILLED_MAP: &str = "minecraft:filled_map";
/// Pose bones Java's parts drive, in [`JavaBiped::parts`] order.
const PART_BONES: [&str; 6] = ["head", "body", "rightarm", "leftarm", "rightleg", "leftleg"];
const RIGHT_ARM: usize = 2;
const REMOTE_SNEAK_DROP: f32 = 0.125;
const LOCAL_SNEAK_DROP: f32 = 0.2 * 0.4;
/// Java lifts the model this many pixels above the feet.
const MODEL_LIFT_PIXELS: f32 = 0.125;

mod mounted;

#[cfg(test)]
#[path = "java/mounted_tests.rs"]
mod mounted_tests;

#[cfg(test)]
#[path = "java/clock_tests.rs"]
mod clock_tests;

/// Java's use of the main-hand `item` at the frame, from the use flag's tick count.
pub(super) fn java_use(
    item: &str,
    selected: Option<&str>,
    use_ticks: u32,
    consume_ticks: Option<u32>,
    alpha: f32,
) -> Option<JavaUse> {
    if selected != Some(item) || use_ticks == 0 {
        return None;
    }
    if is_java_sword(item) {
        Some(JavaUse::Block)
    } else if item == BOW {
        Some(JavaUse::Bow {
            pull: BOW_USE_DURATION as f32 - use_remaining(BOW_USE_DURATION, use_ticks, alpha),
        })
    } else {
        consume_ticks.map(|duration| JavaUse::Consume {
            remaining: use_remaining(duration, use_ticks, alpha),
            duration: duration as f32,
        })
    }
}

fn use_remaining(duration: u32, use_ticks: u32, alpha: f32) -> f32 {
    let count = duration.saturating_sub(use_ticks.saturating_sub(1));
    // Keep the integer countdown and float operation order, including bow rounding.
    count as f32 - alpha + 1.0
}

/// Java's bow frame by whole draw ticks: standby, then its three pull frames.
pub(super) fn java_bow_frame(use_ticks: u32) -> u32 {
    match use_ticks.saturating_sub(1) {
        0 => 0,
        1..=13 => 1,
        14..=17 => 2,
        _ => 3,
    }
}

/// Java's first-person hand at the frame.
pub(super) fn first_person_hand(
    rig: &ActorRigSnapshot<'_>,
    main: Option<&str>,
    selected: Option<&str>,
    consume_ticks: Option<u32>,
    alpha: f32,
) -> JavaHand {
    let [previous, current] = rig.java.equip;
    JavaHand {
        swing: rig.java.swing_progress(alpha),
        equip: previous + (current - previous) * alpha,
        using: main
            .and_then(|item| java_use(item, selected, rig.hand[1].use_ticks, consume_ticks, alpha)),
    }
}

/// Model-space targets for `parts` of `pose` on a skeleton, by pose index. A `partial`
/// skeleton (a skin layer) skips parts it lacks; otherwise a missing part yields `None`.
fn targets(
    names: &[Box<str>],
    rest: &[BoneTransform],
    pose: &JavaBiped,
    parts: &[usize],
    partial: bool,
) -> Option<Vec<Option<BoneTransform>>> {
    let mut targets = vec![None; rest.len()];
    let all = pose.parts();
    for &part in parts {
        let found = names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(PART_BONES[part]));
        let index = match found {
            Some(index) => index,
            None if partial => continue,
            None => return None,
        };
        let (posed, rest_part) = all[part];
        let pivot = Vec3::from_slice(&rest.get(index)?.translation_scale[..3]);
        let (rotation, translation) = posed.rig_bone(rest_part, pivot);
        *targets.get_mut(index)? = Some(BoneTransform {
            rotation: rotation.to_array(),
            translation_scale: [translation.x, translation.y, translation.z, 1.0],
            axis_scale: [1.0; 3],
        });
    }
    Some(targets)
}

fn retargeted(
    stream: &WorldStream,
    rig: &ActorRigSnapshot<'_>,
    pose: &JavaBiped,
    parts: &[usize],
    alpha: f32,
) -> Option<Arc<[RenderBoneTransform]>> {
    let targets = targets(rig.bone_names, rig.rest, pose, parts, false)?;
    convert_bones(&stream.authority().actor_retargeted_pose(
        rig.actor.runtime_id,
        alpha,
        &targets,
    )?)
}

/// Java's body-yaw rig, pose and animated skin layers for a player at the frame, unless
/// vanilla keeps its posture; `local` holds the client's own equipment.
pub(super) struct ThirdPerson<'a> {
    pub(super) rig: ActorRigSnapshot<'a>,
    pub(super) bones: Arc<[RenderBoneTransform]>,
    pub(super) posed: Posed,
}

/// What later layers of a player Java posed this frame read.
pub(super) struct Posed {
    runtime_id: u64,
    pub(super) skin_layers: Vec<SkinRenderLayer>,
    pub(super) cape: JavaCapeInput,
}

pub(super) fn third_person<'a>(
    stream: &WorldStream,
    rig: &ActorRigSnapshot<'a>,
    actor: &ActorSnapshot,
    local: Option<&ActorEquipmentInput>,
    alpha: f32,
) -> Option<ThirdPerson<'a>> {
    if rig.java.vanilla_posture
        || render_model::is_pack_rig_id(render_model::EntityRigId(rig.rig.0))
    {
        return None;
    }
    let mut java_rig = ActorRigSnapshot {
        previous_body_yaw: rig.java.body_yaw[0],
        body_yaw: rig.java.body_yaw[1],
        ..*rig
    };
    if rig.java.body_frame_alpha.is_some() {
        let body_yaw = rig.java.body_yaw_at(alpha);
        java_rig.previous_body_yaw = body_yaw;
        java_rig.body_yaw = body_yaw;
    }
    let head_yaw = head_look(actor, alpha, local.is_some()).0;
    if let Some(body_yaw) = mounted::body_yaw(stream, actor, head_yaw, alpha) {
        java_rig.previous_body_yaw = body_yaw;
        java_rig.body_yaw = body_yaw;
    }
    let main = match local {
        Some(equipment) => equipment.main.clone(),
        None => remote_input(stream, actor.runtime_id).main,
    }
    .map(|item| item.identifier);
    let pose = java::java_biped(&third_person_input(
        &java_rig,
        actor,
        main.as_deref(),
        alpha,
        local.is_some(),
    ));
    let parts = [0, 1, 2, 3, 4, 5];
    let bones = retargeted(stream, &java_rig, &pose, &parts, alpha)?;
    let skin_layers = if rig.skin_layers.is_empty() {
        Vec::new()
    } else {
        stream
            .authority()
            .actor_retargeted_layers(actor.runtime_id, alpha, |names, rest| {
                targets(names, rest, &pose, &parts, true)
            })?
    };
    let motion = rig.java;
    let lerp = |[from, to]: [f32; 2]| from + (to - from) * alpha;
    let [chase_from, chase_to] = motion.cape.map(Vec3::from_array);
    let cape = JavaCapeInput {
        chase: chase_from.lerp(chase_to, alpha),
        body_yaw: motion.body_yaw_at(alpha),
        bob: lerp(motion.bob),
        walked: lerp(motion.walked),
        sneaking: actor.is_sneaking(),
    };
    Some(ThirdPerson {
        rig: java_rig,
        bones,
        posed: Posed {
            runtime_id: actor.runtime_id,
            skin_layers,
            cape,
        },
    })
}

/// The player Java posed this frame, if it was.
pub(super) fn posed(posed: &[Posed], runtime_id: u64) -> Option<&Posed> {
    posed.iter().find(|posed| posed.runtime_id == runtime_id)
}

/// Replaces the presentation's pose with Java's and lifts it as Java draws players.
pub(super) fn apply_pose(
    presentation: &mut ActorRigPresentation,
    bones: &Arc<[RenderBoneTransform]>,
    local: bool,
    actor: &ActorSnapshot,
    alpha: f32,
) {
    let submission = &mut presentation.submission;
    if let Some(progress) = actor.death_rotation_progress(alpha) {
        submission.world_from_actor = death_tilt(
            submission.world_from_actor,
            progress,
            f32::from(actor.status.death_time) + alpha,
        );
    }
    submission.input.previous_bones = Arc::clone(bones);
    submission.input.current_bones = Arc::clone(bones);
    // The local body is placed at its render feet afterwards, which then lifts it.
    if !local {
        lift(&mut submission.world_from_actor, actor.is_sneaking(), false);
    }
}

/// Replaces the native body's death angle with Java's faster sideways fall.
fn death_tilt(mut rows: [[f32; 4]; 3], native_progress: f32, death_ticks: f32) -> [[f32; 4]; 3] {
    let java_progress =
        ((death_ticks - 1.0) / f32::from(client_world::DEATH_DURATION_TICKS) * 1.6).clamp(0.0, 1.0);
    let angle = (java_progress.sqrt() - native_progress.clamp(0.0, 1.0).sqrt())
        * std::f32::consts::FRAC_PI_2;
    let (sine, cosine) = angle.sin_cos();
    for row in &mut rows {
        let (x, y) = (row[0], row[1]);
        row[0] = x * cosine + y * sine;
        row[1] = -x * sine + y * cosine;
    }
    rows
}

/// Reapplies death rotation after local visibility rebuilds the world transform.
pub(super) fn local_death_tilt(
    rows: [[f32; 4]; 3],
    native_progress: Option<f32>,
    java_ticks: Option<f32>,
) -> [[f32; 4]; 3] {
    let tilted = crate::presentation::actors::death_tilted(rows, native_progress);
    match (native_progress, java_ticks) {
        (Some(progress), Some(ticks)) => death_tilt(tilted, progress, ticks),
        _ => tilted,
    }
}

/// Java's pose inputs at the frame for a player holding `main_hand`.
fn third_person_input(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    main_hand: Option<&str>,
    alpha: f32,
    local: bool,
) -> JavaBipedInput {
    let motion = rig.java;
    let lerp = |[from, to]: [f32; 2]| from + (to - from) * alpha;
    let body_yaw = lerp_degrees(rig.previous_body_yaw, rig.body_yaw, alpha);
    let (head_yaw, head_pitch) = head_look(actor, alpha, local);
    let using = actor.is_using_item();
    JavaBipedInput {
        limb_swing: motion.limb_swing[1] - motion.limb_amount[1] * (1.0 - alpha),
        limb_amount: lerp(motion.limb_amount).min(1.0),
        age: actor.status.age_ticks as f32 + alpha,
        head_yaw: wrap_degrees(head_yaw - body_yaw),
        head_pitch,
        swing: rig.java.swing_progress(alpha),
        sneaking: actor.is_sneaking(),
        riding: motion.riding,
        held_right: match main_hand {
            None => 0,
            Some(item) if using && is_java_sword(item) => 3,
            Some(_) => 1,
        },
        aimed_bow: using && main_hand == Some(BOW),
    }
}

fn head_look(actor: &ActorSnapshot, alpha: f32, local: bool) -> (f32, f32) {
    // Local look arrives every frame; current actor angles advance only at fixed ticks.
    if local {
        (actor.received_pose.head_yaw, actor.received_pose.pitch)
    } else {
        (
            lerp_degrees(actor.previous_pose.head_yaw, actor.head_yaw, alpha),
            actor.previous_pose.pitch + (actor.pitch - actor.previous_pose.pitch) * alpha,
        )
    }
}

/// Java's lift above the feet, less its sneaking drop: other players draw 0.125 lower, the
/// local player by its eased 0.2 · 0.4 step offset.
pub(super) fn lift(world_from_actor: &mut [[f32; 4]; 3], sneaking: bool, local: bool) {
    for row in world_from_actor.iter_mut() {
        row[3] += row[1] * MODEL_LIFT_PIXELS / 16.0;
    }
    if sneaking {
        world_from_actor[1][3] -= if local {
            LOCAL_SNEAK_DROP
        } else {
            REMOTE_SNEAK_DROP
        };
    }
}

type ArmKey = (client_world::ActorLifetimeId, u32, u64);

/// Frame-to-frame state of Java's first-person hand.
#[derive(Default)]
pub(super) struct HandCache {
    /// The main-hand item still drawn through an equip dip.
    shown: Option<WornItem>,
    pub(super) native_pose: super::hand::NativePoseCache,
    /// The empty hand's rest pose, by actor lifetime, rig and rest generation.
    arm: Option<(ArmKey, Arc<[RenderBoneTransform]>)>,
}

/// Matches the item and data value retained by the equip animation.
fn is_held(item: &WornItem, held: &JavaHeldItem) -> bool {
    item.identifier == held.identifier && item.damage.unwrap_or(item.metadata) == held.metadata
}

impl HandCache {
    /// Remembers the held item once Java's equip adopts it, in any perspective.
    pub(super) fn remember(&mut self, rig: &ActorRigSnapshot<'_>, main: Option<&WornItem>) {
        if let (Some(item), Some(equipped)) = (main, rig.java_equipped)
            && is_held(item, equipped)
        {
            self.shown = Some(item.clone());
        }
    }

    /// The main-hand stack the first-person hand shows: the one Java's equip still holds
    /// through a dip, which becomes `selected` at its bottom.
    pub(super) fn displayed_main(
        &self,
        equipped: Option<&JavaHeldItem>,
        selected: Option<&WornItem>,
    ) -> Option<WornItem> {
        let equipped = equipped?;
        match selected {
            Some(item) if is_held(item, equipped) => Some(item.clone()),
            _ => self
                .shown
                .clone()
                .filter(|old| is_held(old, equipped))
                .or_else(|| selected.cloned()),
        }
    }
}

/// Vanilla's own hand draws a shown item Java never had (crossbows, shields, maps in either
/// hand); deciding on the shown item swaps hands at the bottom of the dip, where both are lowest.
fn vanilla_draws(
    main: Option<&WornItem>,
    off: Option<&WornItem>,
    vanilla_attachable: impl Fn(&str) -> bool,
) -> bool {
    let map = |item: &WornItem| &*item.identifier == FILLED_MAP;
    main.is_some_and(|item| map(item) || vanilla_attachable(&item.identifier))
        || off.is_some_and(map)
}

/// Retains the drawn stack through Java's dip while authored player rigs keep their whole hand.
/// Attachables read the selected owner's use timing only after their retained stack is adopted.
pub(super) fn hand_source(
    mut inputs: HandInputs<'_>,
    equipment: &mut EquipmentRuntime,
    cache: &mut HandCache,
) -> Option<HandSource> {
    let stream = inputs.stream;
    let equipment_input = inputs.equipment_input;
    let owner_equipment = inputs.owner_equipment;
    let consume_ticks = inputs.consume_ticks;
    let item_animation = inputs.item_animation;
    let alpha = inputs.alpha;
    let artwork = inputs.artwork;
    let motion = inputs.motion;
    let runtime_id = inputs.presentation.submission.input.identity.runtime_id;
    let rig = stream.authority().actor_rig(runtime_id)?;
    if render_model::is_pack_rig_id(render_model::EntityRigId(rig.rig.0)) {
        return None;
    }
    let main = cache.displayed_main(rig.java_equipped, equipment_input.main.as_ref());
    let selected = owner_equipment
        .main
        .as_ref()
        .filter(|item| rig.java_equipped.is_some_and(|held| is_held(item, held)))
        .map(|item| item.identifier.as_ref());
    let retained = main.as_ref().map(|item| item.identifier.as_ref());
    let hand = first_person_hand(&rig, retained, selected, consume_ticks, alpha);
    if vanilla_draws(main.as_ref(), equipment_input.off.as_ref(), |identifier| {
        equipment.is_vanilla_attachable(identifier)
    }) {
        let retained_equipment = ActorEquipmentInput {
            main: main.clone(),
            ..equipment_input.clone()
        };
        let progress = FirstPersonHand {
            swing: hand.swing,
            equip: hand.equip,
            consume: hand_progress(
                rig.hand,
                consume_ticks.filter(|_| retained == selected),
                alpha,
            )
            .consume,
        };
        let mut source = vanilla_hand_source(
            HandInputs {
                equipment_input: &retained_equipment,
                ..inputs
            },
            equipment,
            progress,
            &mut cache.native_pose,
        )?;
        let map = |item: &WornItem| &*item.identifier == FILLED_MAP;
        if main.as_ref().is_some_and(|item| !map(item))
            && !equipment_input.off.as_ref().is_some_and(map)
        {
            source.body = None;
        }
        return Some(source);
    }
    let main_layer = main.as_ref().and_then(|item| {
        let attachable =
            (retained == selected && java_draws_attachable(&item.identifier)).then(|| {
                let mut input = item_animation?;
                input.frame_alpha = alpha;
                input.animation_frame = java_bow_frame(rig.hand[1].use_ticks);
                let input = equipment_input.attachable_input(input.for_hand(false));
                let actor = stream.authority().actor(runtime_id)?;
                cache.native_pose.apply(&mut inputs);
                equipment.first_person_attachable(
                    &inputs.presentation.submission,
                    item,
                    actor,
                    &rig,
                    input,
                    Some(hand),
                )
            });
        let layer = attachable.flatten().or_else(|| {
            equipment.first_person_java_item(&inputs.presentation.submission, item, hand)
        })?;
        let atlas = item_atlas(&layer, artwork)?;
        Some((layer, atlas))
    });
    let off_layer = equipment_input.off.as_ref().and_then(|item| {
        let attachable = item_animation.and_then(|mut input| {
            input.frame_alpha = alpha;
            let input =
                super::hand::attachable_hand_input(equipment_input, owner_equipment, input, true);
            let actor = stream.authority().actor(runtime_id)?;
            cache.native_pose.apply(&mut inputs);
            equipment.first_person_attachable(
                &inputs.presentation.submission,
                item,
                actor,
                &rig,
                input,
                None,
            )
        });
        let layer = attachable
            .or_else(|| equipment.first_person_offhand(&inputs.presentation.submission, item))?;
        let atlas = item_atlas(&layer, artwork)?;
        Some((layer, atlas))
    });
    // A missing held-item image does not make the hand empty. The empty arm's fixed pose is
    // retargeted once per rig.
    let key = (rig.actor, rig.rig.0, rig.rest_reset_generation);
    let arm = main.is_none().then(|| {
        let bones = match &cache.arm {
            Some((cached, bones)) if *cached == key => Arc::clone(bones),
            _ => {
                let pose = java::java_biped(&JavaBipedInput::default());
                let bones = retargeted(stream, &rig, &pose, &[RIGHT_ARM], alpha)?;
                cache.arm = Some((key, Arc::clone(&bones)));
                bones
            }
        };
        Some((bones, java::first_person_arm(hand.swing, hand.equip)))
    });
    let (body, java_body_camera) = match arm.flatten() {
        Some((bones, camera)) => {
            let mut posed = inputs.presentation.submission.clone();
            posed.input.previous_bones = Arc::clone(&bones);
            posed.input.current_bones = bones;
            let arms = FirstPersonArms {
                right: true,
                left: false,
            };
            (equipment.mask_first_person(&posed, arms), Some(camera))
        }
        None => (None, None),
    };
    Some(HandSource {
        presentation: inputs.presentation,
        body,
        items: [main_layer, off_layer],
        motion,
        java_body_camera,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "java/hand_tests.rs"]
mod hand_tests;
