//! Equipped ground effects remain independent of the visibility of the owner's body.
use super::{ActorFrameInput, RemoteHat};
use bevy::prelude::Resource;
use cosmetics::Item;
use render_model::hats;

/// One admitted item bound to an actor lifetime, without account data or draw visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroundEffectWearer {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub item: Item,
}

/// Ground items selected for the latest actor frame, including first-person wearers.
#[derive(Resource, Default)]
pub struct GroundEffectWearers(pub Vec<GroundEffectWearer>);

impl GroundEffectWearers {
    /// Resolves current equipped selections against live actors before body culling.
    pub(super) fn capture(
        &mut self,
        input: &ActorFrameInput,
        stream: Option<&chunk_pipeline::WorldStream>,
    ) {
        self.0.clear();
        let Some(stream) = stream else {
            return;
        };
        let local = stream.local_player_runtime_id();
        if let Some(actor) = stream.authority().actor(local) {
            for &item in &input.cosmetics {
                self.push(local, actor.spawn_revision, item);
            }
        }
        for &RemoteHat {
            runtime_id,
            spawn_revision,
            hat,
        } in &input.remote_hats
        {
            if runtime_id != local
                && stream
                    .authority()
                    .actor(runtime_id)
                    .is_some_and(|a| a.spawn_revision == spawn_revision && !a.is_invisible())
            {
                self.push(runtime_id, spawn_revision, hat);
            }
        }
    }

    /// Keeps each ground-only item once even if a repeated selection reaches the frame input.
    fn push(&mut self, runtime_id: u64, spawn_revision: u64, item: Item) {
        let wearer = GroundEffectWearer {
            runtime_id,
            spawn_revision,
            item,
        };
        if hats::rig(item).is_ground_effect() && !self.0.contains(&wearer) {
            self.0.push(wearer);
        }
    }
}
