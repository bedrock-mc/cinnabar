//! Local headwear follows the published head endpoints, without changing network appearance.

use std::sync::Arc;

use render_api::Hat;
use render_model::{
    ACTOR_LAYER_BODY, ActorRigRoute, MAX_RENDERED_PLAYERS,
    hats::{self, HatPoseCache},
};

use crate::presentation::actors::ActorPresentationBatch;

#[derive(Default)]
pub(super) struct HatState {
    hat: Option<Hat>,
    wind: hats::EarWind,
    pub(super) bend: f32,
    poses: [HatPoseCache; 2],
}

impl HatState {
    /// Advances once at the actor clock boundary, including while the body is hidden.
    pub(super) fn advance(&mut self, hat: Option<Hat>, forward: f32, seconds: f32) {
        if self.hat != hat {
            *self = Self::default();
            self.hat = hat;
        }
        self.bend = self
            .wind
            .advance(if hat.is_some() { forward } else { 0.0 }, seconds);
    }

    /// Adds a separately textured attachment only when the local player's body is admitted.
    pub(super) fn attach(
        &mut self,
        batch: &mut ActorPresentationBatch,
        stream: Option<&chunk_pipeline::WorldStream>,
        scene: &mut render_scene::ActorRenderScene,
    ) {
        let (Some(hat), Some(stream)) = (self.hat, stream) else {
            return;
        };
        let id = stream.local_player_runtime_id();
        let Some(body) = batch.submissions.iter().find(|body| {
            body.input.identity.runtime_id == id
                && body.input.identity.layer == ACTOR_LAYER_BODY
                && body.route != ActorRigRoute::NoDraw
        }) else {
            return;
        };
        let Some(head) = stream.authority().actor_rig(id).and_then(|rig| {
            rig.bone_names
                .iter()
                .position(|name| name.eq_ignore_ascii_case("head"))
        }) else {
            return;
        };
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(head),
            body.input.current_bones.get(head),
        ) else {
            return;
        };
        if batch.skin_layers.len() >= MAX_RENDERED_PLAYERS {
            return;
        }
        let model = hats::rig(hat);
        if !scene.contains_geometry(model.geometry.id) {
            let _ = scene.insert_geometry(model.geometry.clone());
        }
        let mut submission = body.clone();
        submission.input.identity.layer = hats::ACTOR_LAYER_HAT;
        submission.input.rig = model.geometry.id;
        submission.input.current_bones = self.poses[1].pose(hat, *current, self.bend);
        submission.input.previous_bones = if previous == current {
            Arc::clone(&submission.input.current_bones)
        } else {
            self.poses[0].pose(hat, *previous, self.bend)
        };
        submission.texture_layer = batch.skin_layers.len() as u32;
        submission.tint = 0;
        batch.skin_layers.push(model.pixels.clone());
        batch.submissions.push(submission);
    }
}
