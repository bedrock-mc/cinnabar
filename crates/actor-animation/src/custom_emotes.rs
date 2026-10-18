//! Original Cinnabar emotes; no Marketplace/Lunar animation data is included.
//! The owned emote channels reuse native named skeleton and hierarchy composition.
use super::{
    ActorRigSnapshot, BoneTransform, RenderTextureLayer, RuntimeBone, SkinRenderLayer, geometry,
    pose::{
        compose_pose, compose_pose_with_targets, quat_from_euler, quat_multiply, rotate_vector,
    },
};
use std::sync::Arc;

/// Composed endpoints and the body's shared model-space floor translations.
struct PosePair {
    rest: Arc<[BoneTransform]>,
    previous: Arc<[BoneTransform]>,
    current: Arc<[BoneTransform]>,
    floor: [f32; 2],
}

/// The sampled body drives matching joints in attached geometries.
struct AttachedPose<'a> {
    names: &'a [Box<str>],
    pose: &'a PosePair,
}
mod articulated;
mod catalog;
mod floor;

/// Stable playback identity for the built-in dance or an admitted catalog revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomEmote {
    Twerk,
    Catalog(cosmetics::emotes::Item),
}

impl CustomEmote {
    pub const ALL: &[Self] = &[Self::Twerk];
    /// Lists supported emotes owned by this account; free catalog clips are universally owned.
    pub fn available() -> Vec<Self> {
        Self::ALL
            .iter()
            .copied()
            .chain(
                cosmetics::emotes::Item::all()
                    .into_iter()
                    .filter(|item| item.descriptor().free)
                    .map(Self::Catalog),
            )
            .collect()
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Twerk => "cinnabar:twerk",
            Self::Catalog(item) => &item.descriptor().id,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Twerk => "Twerk",
            Self::Catalog(item) => &item.descriptor().display_name,
        }
    }
    pub fn duration_seconds(self) -> f64 {
        match self {
            Self::Twerk => 0.45,
            Self::Catalog(item) => item.clip().duration,
        }
    }
    pub fn looping(self) -> bool {
        match self {
            Self::Twerk => true,
            Self::Catalog(item) => item.clip().looping,
        }
    }
    /// Catalog and built-in custom playback may be shared with nearby clients.
    pub fn local_only(self) -> bool {
        false
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|emote| emote.id() == id)
            .or_else(|| cosmetics::emotes::Item::from_id(id).map(Self::Catalog))
    }
    /// The full articulation profile is shared by all catalog clips.
    fn articulated(self) -> bool {
        matches!(self, Self::Catalog(_))
    }
}

/// Render-only poses owned by this sample, including clothing/persona geometries.
/// Feed `snapshot` into the ordinary *third-person* body publication path only.
/// Keep the native snapshot for the first-person hand and actors without custom playback.
#[derive(Clone, Debug)]
pub struct CustomEmotePose {
    pub previous: Arc<[BoneTransform]>,
    pub current: Arc<[BoneTransform]>,
    pub render: Vec<RenderTextureLayer>,
    pub skin_layers: Vec<SkinRenderLayer>,
    pub(super) articulated: Option<CustomEmoteRig>,
}

#[derive(Clone, Debug)]
pub(super) struct CustomEmoteRig {
    pub geometry: Arc<assets::SkinGeometry>,
    pub mesh: render_model::ActorRigGeometry,
    pub names: Arc<[Box<str>]>,
    pub rest: Arc<[BoneTransform]>,
}

impl CustomEmotePose {
    pub fn snapshot<'a>(&'a self, mut original: ActorRigSnapshot<'a>) -> ActorRigSnapshot<'a> {
        original.previous = &self.previous;
        original.current = &self.current;
        original.render = &self.render;
        original.skin_layers = &self.skin_layers;
        if let Some(rig) = &self.articulated {
            original.skin_geometry = Some(&rig.geometry);
            original.skin_mesh = Some(&rig.mesh);
            original.bone_names = &rig.names;
            original.rest = &rig.rest;
        }
        original
    }
}

/// Samples original authored channels before the native skeleton composition.
/// Pass the same render-time phase twice for an already-interpolated local body;
/// otherwise pass the two fixed-tick phases that the render alpha interpolates.
/// These allocations must bypass a conversion cache keyed only by pose pointers:
/// a dropped sample's address can be reused while its actor tick stays unchanged.
pub fn sample_custom_emote(
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
    let articulated_model = rig
        .skin_geometry
        .and_then(|skin| articulated::model(skin, emote));
    let (bones, names) = if let Some(model) = &articulated_model {
        geometry::skeleton(&model.geometry.bones)?
    } else {
        (bones, names)
    };
    let contacts = emote
        .articulated()
        .then(|| floor::contacts(rig, articulated_model.as_ref(), &names, leg_height));
    let body = pair(
        &bones,
        &names,
        emote,
        [previous_seconds, current_seconds],
        leg_height,
        contacts.as_deref(),
        None,
    )?;
    let mut render = rig.render.to_vec();
    let articulated = if let Some(model) = articulated_model {
        for layer in render.iter_mut().filter(|layer| layer.geometry.is_none()) {
            layer.hidden_bones = model.hidden_bones(&layer.hidden_bones);
        }
        Some(CustomEmoteRig {
            geometry: model.geometry,
            mesh: model.mesh,
            names: names.clone().into(),
            rest: Arc::clone(&body.rest),
        })
    } else {
        None
    };
    let attached = emote.articulated().then_some(AttachedPose {
        names: &names,
        pose: &body,
    });
    for layer in &mut render {
        if let Some(index) = layer.geometry {
            let (assets, _) = catalog?;
            let (bones, names) = geometry::resolve_bones(assets, index as usize)?;
            let pose = pair(
                &bones,
                &names,
                emote,
                [previous_seconds, current_seconds],
                leg_height,
                None,
                attached.as_ref(),
            )?;
            layer.previous_pose = pose.previous;
            layer.pose = pose.current;
        }
    }
    let mut skin_layers = rig.skin_layers.to_vec();
    for layer in &mut skin_layers {
        let bounds = if let Some(model) = articulated::model(&layer.geometry, emote) {
            layer.hidden_bones = model.hidden_bones(&layer.hidden_bones);
            layer.geometry = model.geometry;
            layer.mesh = Some(model.mesh);
            Some(model.bounds)
        } else {
            None
        };
        let (bones, names) = geometry::skeleton(&layer.geometry.bones)?;
        let pose = pair(
            &bones,
            &names,
            emote,
            [previous_seconds, current_seconds],
            leg_height,
            bounds.as_deref(),
            attached.as_ref(),
        )?;
        layer.rest = pose.rest;
        layer.previous = pose.previous;
        layer.current = pose.current;
    }
    Some(CustomEmotePose {
        previous: body.previous,
        current: body.current,
        render,
        skin_layers,
        articulated,
    })
}

/// Samples a body or attached geometry, sharing the body's contacts and named joint motion.
fn pair(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    emote: CustomEmote,
    [previous, current]: [f64; 2],
    leg_height: f32,
    bounds: Option<&[Option<articulated::BoneBounds>]>,
    attached: Option<&AttachedPose<'_>>,
) -> Option<PosePair> {
    let rest: Arc<[_]> = compose_pose(bones, &[])?.into();
    let sample = |seconds, endpoint: usize| {
        let mut targets = targets(bones, names, &rest, emote, seconds, leg_height, bounds);
        if let Some(attached) = attached {
            for target in targets.iter_mut().flatten() {
                target.translation_scale[1] += attached.pose.floor[endpoint];
            }
            let body = [&attached.pose.previous, &attached.pose.current][endpoint];
            for ((name, rest), target) in names.iter().zip(rest.iter()).zip(&mut targets) {
                let Some(index) = attached.names.iter().position(|body| body == name) else {
                    continue;
                };
                let bind = attached.pose.rest[index];
                let inverse = [
                    -bind.rotation[0],
                    -bind.rotation[1],
                    -bind.rotation[2],
                    bind.rotation[3],
                ];
                let rotation = quat_multiply(body[index].rotation, inverse);
                let offset = rotate_vector(
                    rotation,
                    std::array::from_fn(|axis| {
                        rest.translation_scale[axis] - bind.translation_scale[axis]
                    }),
                );
                let mut shared = *rest;
                shared.rotation = quat_multiply(rotation, rest.rotation);
                for (axis, offset) in offset.into_iter().enumerate() {
                    shared.translation_scale[axis] = body[index].translation_scale[axis] + offset;
                }
                *target = Some(shared);
            }
        }
        let mut pose = compose_pose_with_targets(bones, &[], &targets)?;
        let floor = if emote.articulated() && attached.is_none() {
            floor::ground_pose(&mut pose, bounds.unwrap_or(&[]))
        } else {
            attached.map_or(0.0, |attached| attached.pose.floor[endpoint])
        };
        Some((Arc::<[BoneTransform]>::from(pose), floor))
    };
    let (previous_pose, previous_floor) = sample(previous, 0)?;
    let (current_pose, current_floor) = if previous == current {
        (Arc::clone(&previous_pose), previous_floor)
    } else {
        sample(current, 1)?
    };
    Some(PosePair {
        rest,
        previous: previous_pose,
        current: current_pose,
        floor: [previous_floor, current_floor],
    })
}

fn targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    emote: CustomEmote,
    seconds: f64,
    leg_height: f32,
    _bounds: Option<&[Option<articulated::BoneBounds>]>,
) -> Vec<Option<BoneTransform>> {
    if emote == CustomEmote::Twerk
        && let Some(joints) = ArticulatedJoints::resolve(names)
    {
        return bent_knee_targets(bones, names, rest, emote, seconds, leg_height, joints);
    }
    match emote {
        CustomEmote::Twerk => twerk_targets(bones, names, rest, emote, seconds, leg_height),
        CustomEmote::Catalog(item) => {
            catalog::targets(names, rest, item.clip(), seconds, leg_height)
        }
    }
}

/// Indices required by the bent-knee solver, resolved independently for each skeleton.
struct ArticulatedJoints {
    body: usize,
    legs: [[usize; 3]; 2],
}

impl ArticulatedJoints {
    /// Declines partial authored markers so they use the ordinary named-bone pose.
    fn resolve(names: &[Box<str>]) -> Option<Self> {
        let find = |name: &str| names.iter().position(|part| part.as_ref() == name);
        Some(Self {
            body: find("body")?,
            legs: [
                [
                    find("leftleg")?,
                    find("leftleg.cinnabar_knee")?,
                    find("leftleg.cinnabar_ankle")?,
                ],
                [
                    find("rightleg")?,
                    find("rightleg.cinnabar_knee")?,
                    find("rightleg.cinnabar_ankle")?,
                ],
            ],
        })
    }
}

fn bent_knee_targets(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    rest: &[BoneTransform],
    emote: CustomEmote,
    seconds: f64,
    height: f32,
    joints: ArticulatedJoints,
) -> Vec<Option<BoneTransform>> {
    let angle = (seconds.rem_euclid(emote.duration_seconds()) / emote.duration_seconds()
        * std::f64::consts::TAU) as f32;
    let hip_y = height * (0.625 + 0.10 * angle.cos());
    let hip_z = height * (0.33 + 0.10 * angle.cos());
    let torso_height = rest[joints.body].translation_scale[1] - height;
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
    for [leg, knee, ankle] in joints.legs {
        let x = rest[leg].translation_scale[0];
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

#[cfg(test)]
mod catalog_tests {
    use super::CustomEmote;

    #[test]
    fn owned_custom_emote_catalog_has_round_trip_ids_and_loop_periods() {
        for emote in CustomEmote::ALL {
            assert_eq!(CustomEmote::from_id(emote.id()), Some(*emote));
            assert!(!emote.label().is_empty());
            assert!(emote.duration_seconds().is_finite() && emote.duration_seconds() > 0.0);
            assert!(emote.looping() && !emote.local_only());
            assert_eq!(
                CustomEmote::ALL
                    .iter()
                    .filter(|entry| entry.id() == emote.id())
                    .count(),
                1
            );
        }
        assert_eq!(CustomEmote::from_id("unknown"), None);
    }
}
