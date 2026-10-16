//! Shared headwear follows each admitted player's head and current actor lifetime.

use std::{collections::HashMap, sync::Arc};

use super::RemoteHat;
use render_api::Hat;
use render_model::{
    ACTOR_LAYER_BODY, ActorRigRoute, MAX_RENDERED_PLAYERS,
    hats::{self, HatPoseCache},
};

use crate::presentation::actors::ActorPresentationBatch;

#[derive(Default)]
struct Attachment {
    hat: Option<Hat>,
    wind: hats::EarWind,
    bend: f32,
    poses: [HatPoseCache; 2],
}

impl Attachment {
    /// Resets the bend when the equipped model changes, retaining settled poses otherwise.
    fn advance(&mut self, hat: Option<Hat>, forward: f32, seconds: f32) {
        if self.hat != hat {
            *self = Self::default();
            self.hat = hat;
        }
        self.bend = self.wind.advance(
            if hat.is_some_and(|hat| hats::rig(hat).bends_in_wind()) {
                forward
            } else {
                0.0
            },
            seconds,
        );
    }
}

#[derive(Default)]
struct Remote {
    spawn: u64,
    seen: u64,
    attachment: Attachment,
}

impl Remote {
    /// Replaces the authenticated selection and discards state from recycled actor identifiers.
    fn refresh(&mut self, spawn: u64, hat: Option<Hat>) {
        if self.spawn != spawn {
            *self = Self {
                spawn,
                ..Default::default()
            };
        }
        self.attachment.advance(hat, 0.0, 0.0);
    }
}

#[derive(Default)]
pub(super) struct HatState {
    local: Attachment,
    remote: HashMap<u64, Remote>,
    texture_layers: Vec<Option<u32>>,
    seconds: f32,
    frame: u64,
    pub(super) bend: f32,
}

impl HatState {
    /// Advances the local wind once, including while its body is hidden, for the HUD paperdoll.
    pub(super) fn advance(&mut self, hat: Option<Hat>, forward: f32, seconds: f32) {
        self.local.advance(hat, forward, seconds);
        self.bend = self.local.bend;
        self.seconds = seconds;
        self.frame = self.frame.wrapping_add(1);
    }

    /// Adds original cosmetic layers to local and remote bodies using their current head poses.
    pub(super) fn attach(
        &mut self,
        batch: &mut ActorPresentationBatch,
        stream: Option<&chunk_pipeline::WorldStream>,
        hats: &[RemoteHat],
        scene: &mut render_scene::ActorRenderScene,
    ) {
        let Some(stream) = stream else {
            self.remote.clear();
            return;
        };
        self.texture_layers.resize(Hat::ALL.len(), None);
        self.texture_layers.fill(None);
        let count = batch.submissions.len();
        for index in 0..count {
            let body = &batch.submissions[index];
            let id = body.input.identity.runtime_id;
            if body.input.identity.layer != ACTOR_LAYER_BODY || body.route == ActorRigRoute::NoDraw
            {
                continue;
            }
            let attachment = if id == stream.local_player_runtime_id() {
                &mut self.local
            } else {
                let Some(actor) = stream.authority().actor(id) else {
                    continue;
                };
                let hat = hats
                    .iter()
                    .find(|hat| hat.runtime_id == id && hat.spawn_revision == actor.spawn_revision)
                    .map(|value| value.hat);
                let Some(hat) = hat else {
                    self.remote.remove(&id);
                    continue;
                };
                let remote = self.remote.entry(id).or_default();
                remote.refresh(actor.spawn_revision, Some(hat));
                remote.seen = self.frame;
                let speed = if actor.interpolation_ticks_remaining > 0 {
                    hats::forward_speed(actor.velocity, actor.head_yaw)
                        / simulation_clock::TICKS_PER_SECOND as f32
                } else {
                    0.0
                };
                remote
                    .attachment
                    .advance(remote.attachment.hat, speed, self.seconds);
                &mut remote.attachment
            };
            let Some(hat) = attachment.hat else { continue };
            let Some(head) = stream.authority().actor_rig(id).and_then(|rig| {
                rig.bone_names
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case("head"))
            }) else {
                continue;
            };
            let (Some(previous), Some(current)) = (
                body.input.previous_bones.get(head),
                body.input.current_bones.get(head),
            ) else {
                continue;
            };
            let slot = Hat::ALL
                .iter()
                .position(|entry| *entry == hat)
                .expect("catalogued hat");
            let model = hats::rig(hat);
            let texture_layer = if let Some(layer) = self.texture_layers[slot] {
                layer
            } else {
                if batch.skin_layers.len() >= MAX_RENDERED_PLAYERS {
                    continue;
                }
                let layer = batch.skin_layers.len() as u32;
                batch.skin_layers.push(model.pixels.clone());
                self.texture_layers[slot] = Some(layer);
                layer
            };
            if !scene.contains_geometry(model.geometry.id) {
                let _ = scene.insert_geometry(model.geometry.clone());
            }
            let mut submission = body.clone();
            submission.input.identity.layer = hats::ACTOR_LAYER_HAT;
            submission.input.rig = model.geometry.id;
            submission.input.current_bones =
                attachment.poses[1].pose(hat, *current, attachment.bend);
            submission.input.previous_bones = if previous == current {
                Arc::clone(&submission.input.current_bones)
            } else {
                attachment.poses[0].pose(hat, *previous, attachment.bend)
            };
            submission.texture_layer = texture_layer;
            submission.tint = 0;
            batch.submissions.push(submission);
        }
        self.remote.retain(|_, remote| remote.seen == self.frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_selections_remove_hats_and_recycled_ids_reset_wind() {
        let mut remote = Remote::default();
        remote.refresh(1, Some(Hat::BunnyEars));
        remote.attachment.advance(remote.attachment.hat, 6.0, 1.0);
        assert_eq!(remote.attachment.hat, Some(Hat::BunnyEars));
        assert!(remote.attachment.bend > 0.5);
        remote.refresh(2, Some(Hat::BunnyEars));
        assert_eq!(remote.attachment.bend, 0.0);
        remote.refresh(2, Some(Hat::Crown));
        remote.attachment.advance(remote.attachment.hat, 6.0, 1.0);
        assert_eq!(remote.attachment.hat, Some(Hat::Crown));
        assert_eq!(remote.attachment.bend, 0.0);
        remote.refresh(2, None);
        assert_eq!(remote.attachment.hat, None);
    }
}
