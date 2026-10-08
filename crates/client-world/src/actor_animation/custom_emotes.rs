//! The owned emote channels reuse native named skeleton and hierarchy composition.
use super::{
    RuntimeBone, geometry,
    pose::{
        compose_pose, compose_pose_with_targets, quat_from_euler, quat_multiply, rotate_vector,
    },
};
use crate::{
    ActorRigSnapshot, BoneTransform,
    custom_emotes::{CustomEmote, CustomEmotePose},
};
use std::sync::Arc;

type PosePair = (Arc<[BoneTransform]>, Arc<[BoneTransform]>);
mod articulated;

pub(crate) fn sample(
    rig: &ActorRigSnapshot<'_>,
    emote: CustomEmote,
    previous_seconds: f64,
    current_seconds: f64,
) -> Option<CustomEmotePose> {
    if ![previous_seconds, current_seconds]
        .into_iter()
        .all(|time| time.is_finite() && time >= 0.0)
    {
        return None;
    }
    let catalog = rig.geometry_source();
    let (bones, names) = if let Some(skin) = rig.skin_geometry {
        geometry::skeleton(&skin.bones)?
    } else {
        let (assets, geometry) = catalog?;
        geometry::resolve_bones(assets, geometry)?
    };
    if names.as_slice() != rig.bone_names
        || bones.len() != rig.current.len()
        || !["body", "head", "leftarm", "rightarm", "leftleg", "rightleg"]
            .into_iter()
            .all(|part| names.iter().any(|name| name.as_ref() == part))
    {
        return None;
    }
    let leg_height =
        bones[names.iter().position(|name| name.as_ref() == "leftleg")?].pivot[1].abs();
    let articulated_geometry = rig.skin_geometry.and_then(articulated::model);
    let (bones, names) = if let Some(model) = &articulated_geometry {
        geometry::skeleton(&model.bones)?
    } else {
        (bones, names)
    };
    let articulated = if let Some(geometry) = articulated_geometry {
        Some(crate::custom_emotes::CustomEmoteRig {
            geometry,
            names: names.clone().into(),
            rest: compose_pose(&bones, &[])?.into(),
        })
    } else {
        None
    };
    let (previous, current) = pair(
        &bones,
        &names,
        emote,
        previous_seconds,
        current_seconds,
        leg_height,
    )?;
    let mut render = rig.render.to_vec();
    for layer in &mut render {
        if let Some(index) = layer.geometry {
            let (assets, _) = catalog?;
            let (bones, names) = geometry::resolve_bones(assets, index as usize)?;
            let (previous, current) = pair(
                &bones,
                &names,
                emote,
                previous_seconds,
                current_seconds,
                leg_height,
            )?;
            layer.previous_pose = previous;
            layer.pose = current;
        }
    }
    let mut skin_layers = rig.skin_layers.to_vec();
    for layer in &mut skin_layers {
        if let Some(model) = articulated::model(&layer.geometry) {
            layer.geometry = model;
        }
        let (bones, names) = geometry::skeleton(&layer.geometry.bones)?;
        let (previous, current) = pair(
            &bones,
            &names,
            emote,
            previous_seconds,
            current_seconds,
            leg_height,
        )?;
        layer.rest = compose_pose(&bones, &[])?.into();
        layer.previous = previous;
        layer.current = current;
    }
    Some(CustomEmotePose {
        previous,
        current,
        render,
        skin_layers,
        articulated,
    })
}

fn pair(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    emote: CustomEmote,
    previous: f64,
    current: f64,
    leg_height: f32,
) -> Option<PosePair> {
    let rest = compose_pose(bones, &[])?;
    let previous_time = previous;
    let previous: Arc<[BoneTransform]> = compose_pose_with_targets(
        bones,
        &[],
        &targets(bones, names, &rest, emote, previous, leg_height),
    )?
    .into();
    let current = if previous_time == current {
        Arc::clone(&previous)
    } else {
        compose_pose_with_targets(
            bones,
            &[],
            &targets(bones, names, &rest, emote, current, leg_height),
        )?
        .into()
    };
    Some((previous, current))
}

fn targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    emote: CustomEmote,
    seconds: f64,
    leg_height: f32,
) -> Vec<Option<BoneTransform>> {
    if names
        .iter()
        .any(|name| name.as_ref() == "leftleg.cinnabar_knee")
    {
        return bent_knee_targets(bones, names, rest, emote, seconds, leg_height);
    }
    match emote {
        CustomEmote::Twerk => twerk_targets(bones, names, rest, emote, seconds, leg_height),
    }
}

fn bent_knee_targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    emote: CustomEmote,
    seconds: f64,
    height: f32,
) -> Vec<Option<BoneTransform>> {
    let angle = (seconds.rem_euclid(emote.duration_seconds()) / emote.duration_seconds()
        * std::f64::consts::TAU) as f32;
    let hip_y = height * (0.625 + 0.10 * angle.cos());
    let hip_z = height * (0.33 + 0.10 * angle.cos());
    let body = names
        .iter()
        .position(|name| name.as_ref() == "body")
        .unwrap();
    let torso_height = rest[body].translation_scale[1] - height;
    let shoulder_y = height * 0.625 + torso_height * 37.0_f32.to_radians().cos();
    let lean = ((shoulder_y - hip_y) / torso_height)
        .clamp(-1.0, 1.0)
        .acos();
    let twist = 6.0 * angle.sin();
    let torso = quat_from_euler([-lean.to_degrees(), twist, 0.0]);
    let offset = [0.0, hip_y - height, hip_z];
    let mut targets: Vec<_> = bones
        .iter()
        .zip(names)
        .zip(rest)
        .map(|((bone, name), rest)| {
            let rotation = match name.as_ref() {
                "waist" | "body" => torso,
                "head" => [0.0, 0.0, 0.0, 1.0],
                "leftarm" => quat_from_euler([-16.0, twist, -3.0]),
                "rightarm" => quat_from_euler([-16.0, twist, 3.0]),
                _ if bone.parent.is_none() => [0.0, 0.0, 0.0, 1.0],
                _ => return None,
            };
            let mut target = *rest;
            let pivot = std::array::from_fn(|axis| rest.translation_scale[axis]);
            let relative = [pivot[0], pivot[1] - height, pivot[2]];
            let pivot = if matches!(
                name.as_ref(),
                "waist" | "body" | "head" | "leftarm" | "rightarm"
            ) {
                let tilted = rotate_vector(torso, relative);
                [tilted[0], height + tilted[1], tilted[2]]
            } else {
                pivot
            };
            target.rotation = quat_multiply(rotation, rest.rotation);
            for axis in 0..3 {
                target.translation_scale[axis] = pivot[axis] + offset[axis];
            }
            Some(target)
        })
        .collect();
    for name in ["leftleg", "rightleg"] {
        let leg = names.iter().position(|part| part.as_ref() == name).unwrap();
        let knee = names
            .iter()
            .position(|part| part.as_ref() == format!("{name}.cinnabar_knee"))
            .unwrap();
        let x = rest[leg].translation_scale[0];
        let ankle = names
            .iter()
            .position(|part| part.as_ref() == format!("{name}.cinnabar_ankle"))
            .unwrap();
        let foot_height = rest[ankle].translation_scale[1];
        let thigh_length = height - rest[knee].translation_scale[1];
        let shin_length = rest[knee].translation_scale[1] - foot_height;
        let hip = [x, hip_y, hip_z];
        let foot = [x + x.signum() * height * 0.16, foot_height, 0.0];
        let delta: [f32; 3] = std::array::from_fn(|axis| foot[axis] - hip[axis]);
        let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        let direction = delta.map(|v| v / distance);
        // Solve thigh/shin to the ankle; the separate foot keeps its sole level.
        let perpendicular = [
            direction[0] * direction[2],
            direction[1] * direction[2],
            -1.0 + direction[2] * direction[2],
        ];
        let length = perpendicular.iter().map(|v| v * v).sum::<f32>().sqrt();
        let along = (thigh_length * thigh_length - shin_length * shin_length + distance * distance)
            / (2.0 * distance);
        let bend = (thigh_length * thigh_length - along * along)
            .max(0.0)
            .sqrt();
        let joint: [f32; 3] = std::array::from_fn(|axis| {
            hip[axis] + direction[axis] * along + perpendicular[axis] * bend / length
        });
        for (index, from, to, segment_length) in [
            (leg, hip, joint, thigh_length),
            (knee, joint, foot, shin_length),
        ] {
            let direction: [f32; 3] =
                std::array::from_fn(|axis| (to[axis] - from[axis]) / segment_length);
            let quaternion = [-direction[2], 0.0, direction[0], 1.0 - direction[1]];
            let length = quaternion.iter().map(|v| v * v).sum::<f32>().sqrt();
            let mut target = rest[index];
            target.rotation = quaternion.map(|v| v / length);
            target.translation_scale[..3].copy_from_slice(&from);
            targets[index] = Some(target);
        }
        let mut target = rest[ankle];
        target.rotation = [0.0, 0.0, 0.0, 1.0];
        target.translation_scale[..3].copy_from_slice(&foot);
        targets[ankle] = Some(target);
    }
    targets
}

fn twerk_targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    emote: CustomEmote,
    seconds: f64,
    leg_height: f32,
) -> Vec<Option<BoneTransform>> {
    let angle = (seconds.rem_euclid(emote.duration_seconds()) / emote.duration_seconds()
        * std::f64::consts::TAU) as f32;
    // Owned clip from the public video: a sustained deep squat, level head and
    // hands beside the thighs, with a hip pulse instead of a standing side sway.
    let wave = angle.cos();
    let base_pitch = 48.0_f32.to_radians();
    let base_spread = 15.0_f32.to_radians();
    let base_lean = 37.0_f32.to_radians();
    let leg_pitch = base_pitch + (8.0 * wave).to_radians();
    let twist = 6.0 * angle.sin();
    // Counter the leg pitch with spread so the stance does not slide sideways.
    let lateral = base_pitch.cos() * base_spread.sin();
    let spread = (lateral / leg_pitch.cos()).clamp(-1.0, 1.0).asin();
    let hip_height = leg_height * leg_pitch.cos() * spread.cos();
    let base_hip_height = leg_height * base_pitch.cos() * base_spread.cos();
    let torso_height = names
        .iter()
        .position(|name| name.as_ref() == "body")
        .map_or(leg_height, |index| {
            rest[index].translation_scale[1] - leg_height
        });
    // Keep shoulders/head at a fixed height. Tilting torso and legs in phase
    // lifts the whole upper body like a repeated jump instead of rocking hips.
    let shoulder_height = base_hip_height + torso_height * base_lean.cos();
    let lean = if torso_height > f32::EPSILON {
        ((shoulder_height - hip_height) / torso_height)
            .clamp(-1.0, 1.0)
            .acos()
    } else {
        base_lean
    };
    // Keep the leg bottom-face centers fixed in all three axes throughout the pulse.
    let offset = [0.0, hip_height - leg_height, leg_height * leg_pitch.sin()];
    let hips = [0.0, leg_height, 0.0];
    let torso = quat_from_euler([-lean.to_degrees(), twist, 0.0]);
    bones
        .iter()
        .zip(names)
        .zip(rest)
        .map(|((bone, name), rest)| {
            let (rotation, tilt_position) = match name.as_ref() {
                "waist" | "body" => (torso, true),
                "head" => ([0.0, 0.0, 0.0, 1.0], true),
                "leftarm" => (quat_from_euler([-16.0, twist, -3.0]), true),
                "rightarm" => (quat_from_euler([-16.0, twist, 3.0]), true),
                "leftleg" => (
                    quat_from_euler([leg_pitch.to_degrees(), 0.0, -spread.to_degrees()]),
                    false,
                ),
                "rightleg" => (
                    quat_from_euler([leg_pitch.to_degrees(), 0.0, spread.to_degrees()]),
                    false,
                ),
                _ if bone.parent.is_none() => ([0.0, 0.0, 0.0, 1.0], false),
                _ => return None,
            };
            let pivot: [f32; 3] = std::array::from_fn(|axis| rest.translation_scale[axis]);
            let pivot = if tilt_position {
                let relative = std::array::from_fn(|axis| pivot[axis] - hips[axis]);
                let rotated = rotate_vector(torso, relative);
                std::array::from_fn(|axis| hips[axis] + rotated[axis])
            } else {
                pivot
            };
            let mut target = *rest;
            target.rotation = quat_multiply(rotation, rest.rotation);
            for axis in 0..3 {
                target.translation_scale[axis] = pivot[axis] + offset[axis];
            }
            Some(target)
        })
        .collect()
}

#[cfg(test)]
mod tests;
