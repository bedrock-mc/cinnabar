//! Draws player capes with prepared skin pixels and the shared cape pose.
use super::actors::ActorPresentationBatch;
use actor_animation::ActorRigSnapshot;
use assets::RuntimeEntityAssets;
use client_world::PlayerProfile;
use protocol::PlayerSkin;
use render_api::SkinRgba8;
use render_model::{
    ACTOR_LAYER_BODY, ActorRigRoute, ActorRigSubmission, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    cape::CapeRig, java_animation::JavaCapeInput,
};
use std::{borrow::Cow, sync::Arc};

/// Render layer of a player's cape, below the extra texture layers.
pub const ACTOR_LAYER_CAPE: u8 = 24;

/// The cape rig, resolved on first use; an absent geometry leaves capes undrawn.
#[derive(Default)]
pub struct CapeState {
    resolved: bool,
    rig: Option<CapeRig>,
    preparation: actor_animation::CapePreparationQueue,
}

impl CapeState {
    /// Resolves the rig once and borrows its worker readiness queue for this draw pass.
    pub fn rig(
        &mut self,
        assets: Option<&RuntimeEntityAssets>,
    ) -> Option<(&CapeRig, &mut actor_animation::CapePreparationQueue)> {
        if !self.resolved
            && let Some(assets) = assets
        {
            self.resolved = true;
            self.rig = CapeRig::resolve(assets);
        }
        Some((self.rig.as_ref()?, &mut self.preparation))
    }
}

/// Converts one immutable body rest bone from model pixels to block coordinates.
fn rest_pose(
    bones: &[actor_animation::BoneTransform],
    index: usize,
) -> Option<RenderBoneTransform> {
    let bone = bones.get(index)?;
    RenderBoneTransform::from_model_space_scaled(
        bone.rotation,
        bone.translation_scale,
        bone.axis_scale,
    )
}

/// Queues a published profile's cape and reads only its ready worker result.
fn cape_of(
    profile: &PlayerProfile,
    preparation: &mut actor_animation::CapePreparationQueue,
) -> Option<SkinRgba8> {
    let PlayerSkin::Standard(skin) = &profile.skin else {
        return None;
    };
    preparation.request(skin.cape.as_ref()?)
}

/// Uses cape art on worn wings, otherwise appends a cape with Java inputs when supplied.
/// Capes beyond the skin-layer budget are dropped without invalidating the frame.
pub fn apply_capes<'a>(
    batch: &mut ActorPresentationBatch,
    cape: &CapeRig,
    preparation: &mut actor_animation::CapePreparationQueue,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    profile_of: impl Fn(u64) -> Option<&'a PlayerProfile>,
    java_cape: impl Fn(u64) -> Option<JavaCapeInput>,
    mut elytra_of: impl FnMut(u64) -> bool,
) {
    preparation.begin_frame();
    let mut capes: Vec<(SkinRgba8, usize)> = Vec::new();
    let mut extras = Vec::new();
    for index in 0..batch.submissions.len() {
        let body = &batch.submissions[index];
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY
            || body.route == ActorRigRoute::NoDraw
            || batch.artwork.contains_key(&identity)
        {
            continue;
        }
        let Some(cape_pixels) =
            profile_of(identity.runtime_id).and_then(|profile| cape_of(profile, preparation))
        else {
            continue;
        };
        let layer = match capes.iter().find(|(known, _)| *known == cape_pixels) {
            Some((_, layer)) => *layer,
            None if batch.skin_layers.len() < MAX_RENDERED_PLAYERS => {
                batch.skin_layers.push(cape_pixels.clone());
                capes.push((cape_pixels, batch.skin_layers.len() - 1));
                batch.skin_layers.len() - 1
            }
            None => continue,
        };
        if elytra_of(identity.runtime_id) {
            replace_elytra_texture(batch, identity.runtime_id, layer as u32);
            continue;
        }
        let Some(rig) = rig_of(identity.runtime_id) else {
            continue;
        };
        let bindings = rig
            .attachments
            .and_then(|bindings| bindings.cape_for(cape))
            .map_or_else(|| Cow::Owned(cape.bind(rig.bone_names)), Cow::Borrowed);
        let body = &batch.submissions[index];
        let mut submission: ActorRigSubmission = body.clone();
        submission.input.identity.layer = ACTOR_LAYER_CAPE;
        submission.input.rig = cape.geometry.id;
        match java_cape(identity.runtime_id) {
            Some(input) => {
                let pose = cape.pose_bound(
                    &bindings,
                    |index| rest_pose(rig.rest, index),
                    &body.input.current_bones,
                    Some(&input),
                );
                submission.input.previous_bones = Arc::clone(&pose);
                submission.input.current_bones = pose;
            }
            None => {
                let current = cape.pose_bound(
                    &bindings,
                    |index| rest_pose(rig.rest, index),
                    &body.input.current_bones,
                    None,
                );
                // Shared body endpoints also share their cape pose and cached matrices.
                submission.input.previous_bones =
                    if Arc::ptr_eq(&body.input.previous_bones, &body.input.current_bones) {
                        Arc::clone(&current)
                    } else {
                        cape.pose_bound(
                            &bindings,
                            |index| rest_pose(rig.rest, index),
                            &body.input.previous_bones,
                            None,
                        )
                    };
                submission.input.current_bones = current;
            }
        }
        submission.texture_layer = layer as u32;
        submission.tint = 0;
        submission.overlay_rgba8 = 0;
        extras.push(submission);
    }
    preparation.submit();
    batch.submissions.extend(extras);
}

/// The cape replaces the worn wings' base image while retaining their geometry and glint.
fn replace_elytra_texture(batch: &mut ActorPresentationBatch, runtime_id: u64, layer: u32) {
    for submission in &mut batch.submissions {
        let identity = submission.input.identity;
        if identity.runtime_id == runtime_id && super::equipment::is_elytra_layer(identity.layer) {
            submission.texture_layer = layer;
            batch.artwork.remove(&identity);
        }
    }
}

#[cfg(test)]
#[path = "cape_elytra_tests.rs"]
mod elytra_tests;

#[cfg(test)]
mod tests {
    /// Builds an original cape rig for pose and wing-routing tests.
    pub(super) fn fixture_cape() -> super::CapeRig {
        let source = serde_json::json!({
            "format_version":"1.12.0",
            "minecraft:geometry":[{
                "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},
                "bones":[{"name":"body","pivot":[0,24,0]},
                    {"name":"cape","parent":"body","pivot":[0,24,3],"bind_pose_rotation":[0,180,0],
                    "cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]
            }]
        });
        let model = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.fixture"}}"#,
            &source.to_string(),
        )
        .unwrap()
        .unwrap();
        let geometry = render_model::skin_geometry(&model, render_model::EntityRigId(1)).unwrap();
        super::CapeRig::new(geometry, &model.bones)
    }
}
