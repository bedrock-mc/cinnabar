//! Actual camera-writer proof, invalidated before every scheduled attempt.
use bevy::prelude::{Entity, ResMut, Resource, Transform};
use chunk_pipeline::WorldStream;
use semantic_input::PerspectiveMode;
use sim::WorldCollisionIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraOwner {
    pub session: u64,
    pub stream: u64,
    pub runtime: u64,
    pub dimension: i32,
    pub epoch: u64,
    pub sequence: u64,
}
impl CameraOwner {
    pub fn current(stream: &WorldStream, session: u64) -> Self {
        Self {
            session,
            stream: stream.authority().actor_session_id(),
            runtime: stream.local_player_runtime_id(),
            dimension: stream.current_dimension(),
            epoch: stream.form_dimension_epoch(),
            sequence: stream.committed_sequence(),
        }
    }
}
#[derive(Debug)]
struct Prepared {
    owner: CameraOwner,
    entity: Entity,
    transform: Transform,
    perspective: PerspectiveMode,
    tick: u64,
    world: WorldCollisionIdentity,
}
#[derive(Debug, Clone, Copy)]
pub struct PublishedCamera {
    pub owner: CameraOwner,
    pub entity: Entity,
    pub transform: Transform,
    pub perspective: PerspectiveMode,
    pub tick: u64,
    pub frame_generation: u64,
}
#[derive(Debug, Default, Resource)]
pub struct CameraPublicationAttempt {
    attempt: u64,
    exhausted: bool,
    prepared: Option<Prepared>,
    published: Option<(u64, PublishedCamera)>,
}
impl CameraPublicationAttempt {
    fn begin(&mut self) {
        self.prepared = None;
        self.published = None;
        match self.attempt.checked_add(1).filter(|_| !self.exhausted) {
            Some(next) => self.attempt = next,
            None => self.exhausted = true,
        }
    }
    pub fn prepare(
        &mut self,
        owner: CameraOwner,
        entity: Entity,
        transform: Transform,
        perspective: PerspectiveMode,
        tick: u64,
        world: WorldCollisionIdentity,
    ) {
        self.prepared = None;
        self.published = None;
        if self.attempt == 0
            || self.exhausted
            || !transform.translation.is_finite()
            || !transform.rotation.is_finite()
            || !transform.scale.is_finite()
        {
            return;
        }
        self.prepared = Some(Prepared {
            owner,
            entity,
            transform,
            perspective,
            tick,
            world,
        });
    }
    pub fn take_prepared(
        &mut self,
        owner: CameraOwner,
        transform: Transform,
        perspective: PerspectiveMode,
        tick: u64,
        world: &WorldCollisionIdentity,
    ) -> Option<(PublishedCamera, WorldCollisionIdentity)> {
        let value = self.prepared.take()?;
        if self.exhausted
            || value.owner != owner
            || value.transform != transform
            || value.perspective != perspective
            || value.tick != tick
            || &value.world != world
        {
            return None;
        }
        Some((
            PublishedCamera {
                owner,
                entity: value.entity,
                transform,
                perspective,
                tick,
                frame_generation: 0,
            },
            value.world,
        ))
    }
    pub fn finish(&mut self, mut value: PublishedCamera, generation: u64) {
        if self.exhausted || self.attempt == 0 || generation == 0 {
            return;
        }
        value.frame_generation = generation;
        self.published = Some((self.attempt, value));
    }
    pub fn published(&self) -> Option<PublishedCamera> {
        self.published
            .filter(|(attempt, _)| *attempt == self.attempt && !self.exhausted)
            .map(|(_, value)| value)
    }
}
pub fn begin_camera_publication_attempt(mut receipt: ResMut<CameraPublicationAttempt>) {
    receipt.begin();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhaustion_withholds_proof_without_reusing_attempt() {
        let mut value = CameraPublicationAttempt {
            attempt: u64::MAX,
            ..Default::default()
        };
        value.begin();
        assert!(value.exhausted);
        assert!(value.published().is_none());
        value.begin();
        assert_eq!(value.attempt, u64::MAX);
    }
}
