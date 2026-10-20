//! Generic catalog clip playback on named articulated player skeletons.
use super::{
    ActorRigSnapshot, BoneTransform, RenderTextureLayer, RuntimeBone, SkinRenderLayer, geometry,
    pose::{compose_pose, compose_pose_with_targets, quat_multiply, rotate_vector},
};
use cosmetics::emotes::Item;
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
            // Generated joints use this render-only model instead of the owner's bindings.
            original.attachments = None;
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
    emote: Item,
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
    let articulated_model = rig.skin_geometry.and_then(articulated::model);
    let (bones, names) = if let Some(model) = &articulated_model {
        geometry::skeleton(&model.geometry.bones)?
    } else {
        (bones, names)
    };
    let contacts = floor::contacts(rig, articulated_model.as_ref(), &names, leg_height);
    let body = pair(
        &bones,
        &names,
        emote,
        [previous_seconds, current_seconds],
        leg_height,
        Some(&contacts),
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
    let attached = AttachedPose {
        names: &names,
        pose: &body,
    };
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
                Some(&attached),
            )?;
            layer.previous_pose = pose.previous;
            layer.pose = pose.current;
        }
    }
    let mut skin_layers = rig.skin_layers.to_vec();
    for layer in &mut skin_layers {
        let bounds = if let Some(model) = articulated::model(&layer.geometry) {
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
            Some(&attached),
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
    emote: Item,
    [previous, current]: [f64; 2],
    leg_height: f32,
    bounds: Option<&[Option<articulated::BoneBounds>]>,
    attached: Option<&AttachedPose<'_>>,
) -> Option<PosePair> {
    let rest: Arc<[_]> = compose_pose(bones, &[])?.into();
    let sample = |seconds, endpoint: usize| {
        let sampled = catalog::targets(names, &rest, emote.clip(), seconds, leg_height);
        let mut targets = sampled.bones;
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
        let floor = if attached.is_none() {
            floor::ground_pose(&mut pose, bounds.unwrap_or(&[]), sampled.root_height)
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

#[cfg(test)]
mod tests;
