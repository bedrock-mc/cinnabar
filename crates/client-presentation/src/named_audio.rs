//! Narrow finite, unit-dynamics, nonspatial named playback from LIVE ingress.
//! Spatial/loop/streaming/category parity is not provided by this lane.

use crate::{
    audio_ingress::SequencedAudioEvent,
    camera::FlyCamera,
    local_player::{CameraPose, LocalPlayerFrameCarrier},
    local_player_camera_receipt::{CameraOwner, CameraPublicationAttempt},
};

use assets::RuntimeAudioPcm;
use bevy::prelude::{MessageReader, NonSendMut, Query, Res, ResMut, Resource, Transform, With};
use semantic_input::PerspectiveMode;
use std::sync::Arc;
mod backend;
pub use backend::{AudioDevice, CAPTURE_CHANNELS, CaptureMixer};
use backend::{CancelablePcm, PermitPool, VOICE_LIMIT, VoiceControl};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioOwner {
    session: u64,
    stream: u64,
    dimension: i32,
    epoch: u64,
}
#[derive(Debug, Default)]
pub struct NamedAudioStats {
    pub accepted: u64,
    pub submitted: u64,
    pub stopped: u64,
    pub capacity: u64,
    pub unsupported: u64,
    pub stale: u64,
    pub missing_camera: u64,
    pub outside_radius: u64,
    pub unavailable: u64,
    pub backend_failed: u64,
}
#[derive(Resource)]
pub struct NamedAudio {
    sample: Option<Arc<RuntimeAudioPcm>>,
    pool: Arc<PermitPool>,
    owner: Option<AudioOwner>,
    last_sequence: u64,
    controls: [Option<VoiceControl>; VOICE_LIMIT],
    pending: [Option<CancelablePcm>; VOICE_LIMIT],
    stats: NamedAudioStats,
}
impl Default for NamedAudio {
    fn default() -> Self {
        Self::new(None)
    }
}
impl NamedAudio {
    #[cfg(any(test, feature = "test-support"))]
    pub fn stats(&self) -> &NamedAudioStats {
        &self.stats
    }
    /// Reports backend-owned slots for composed cancellation tests.
    #[cfg(feature = "test-support")]
    pub fn occupied_permits(&self) -> usize {
        self.pool.occupied()
    }
    /// Reports the retained cancellation controls for composed tests.
    #[cfg(feature = "test-support")]
    pub fn retained_controls(&self) -> (usize, bool) {
        (
            self.controls.iter().flatten().count(),
            self.controls
                .iter()
                .flatten()
                .all(|control| control.cancel.load(std::sync::atomic::Ordering::Acquire)),
        )
    }
    pub fn new(sample: Option<Arc<RuntimeAudioPcm>>) -> Self {
        Self {
            sample,
            pool: Arc::new(PermitPool::default()),
            owner: None,
            last_sequence: 0,
            controls: std::array::from_fn(|_| None),
            pending: std::array::from_fn(|_| None),
            stats: NamedAudioStats::default(),
        }
    }
    fn collect_retired(&mut self) {
        for control in &mut self.controls {
            if control
                .as_ref()
                .is_some_and(|value| self.pool.retired(value.slot, value.token))
            {
                *control = None;
            }
        }
    }
    fn cancel_all(&mut self) {
        for control in self.controls.iter().flatten() {
            control.cancel();
        }
        for source in &mut self.pending {
            *source = None;
        }
        self.collect_retired();
    }
    fn bind(&mut self, owner: Option<AudioOwner>) {
        self.collect_retired();
        if self.owner != owner {
            self.cancel_all();
            if self.owner.map(|value| value.stream) != owner.map(|value| value.stream) {
                self.last_sequence = 0;
            }
            self.owner = owner;
        }
    }
    fn event(&mut self, event: &SequencedAudioEvent, camera: Option<[f32; 3]>, available: bool) {
        if !self.owner.is_some_and(|owner| {
            owner.stream == event.origin_stream_session_id
                && owner.dimension == event.dimension
                && owner.epoch == event.dimension_epoch
        }) || event.sequence <= self.last_sequence
        {
            self.stats.stale = self.stats.stale.saturating_add(1);
            return;
        }
        self.last_sequence = event.sequence;
        match &event.event {
            protocol::AudioEvent::Stop(stop) => {
                if stop.stop_all_sounds
                    || self
                        .sample
                        .as_ref()
                        .is_some_and(|sample| sample.identifier() == stop.name.as_ref())
                {
                    self.cancel_all();
                    self.stats.stopped = self.stats.stopped.saturating_add(1);
                }
                if stop.stop_music_legacy {
                    self.stats.unsupported = self.stats.unsupported.saturating_add(1);
                }
            }
            protocol::AudioEvent::Level(_) | protocol::AudioEvent::LevelEvent(_) => {
                self.stats.unsupported = self.stats.unsupported.saturating_add(1)
            }
            protocol::AudioEvent::Play(play) => {
                let Some(sample) = self.sample.as_ref() else {
                    self.stats.unavailable = self.stats.unavailable.saturating_add(1);
                    return;
                };
                if play.name.as_ref() != sample.identifier()
                    || play.volume != 1.0
                    || play.pitch != 1.0
                    || play.loop_count != -1
                    || play.server_sound_handle.is_some()
                {
                    self.stats.unsupported = self.stats.unsupported.saturating_add(1);
                    return;
                }
                let Some(camera) = camera.filter(|value| value.iter().all(|n| n.is_finite()))
                else {
                    self.stats.missing_camera = self.stats.missing_camera.saturating_add(1);
                    return;
                };
                if !inside_radius(play.position, camera) {
                    self.stats.outside_radius = self.stats.outside_radius.saturating_add(1);
                    return;
                }
                if !available {
                    self.stats.unavailable = self.stats.unavailable.saturating_add(1);
                    return;
                }
                self.collect_retired();
                let Some(slot) = self.controls.iter().position(Option::is_none) else {
                    self.stats.capacity = self.stats.capacity.saturating_add(1);
                    return;
                };
                let Some((source, control)) =
                    CancelablePcm::prepare(self.sample.as_ref().unwrap(), &self.pool)
                else {
                    self.stats.capacity = self.stats.capacity.saturating_add(1);
                    return;
                };
                self.controls[slot] = Some(control);
                self.pending[slot] = Some(source);
                self.stats.accepted = self.stats.accepted.saturating_add(1);
            }
        }
    }
    fn flush(&mut self, mut submit: impl FnMut(CancelablePcm) -> bool) {
        for slot in 0..VOICE_LIMIT {
            if let Some(source) = self.pending[slot].take() {
                if !submit(source) {
                    self.stats.backend_failed = self.stats.backend_failed.saturating_add(1);
                    self.cancel_all();
                    break;
                }
                self.stats.submitted = self.stats.submitted.saturating_add(1);
            }
        }
        self.collect_retired();
    }
}
fn inside_radius(raw: [i32; 3], camera: [f32; 3]) -> bool {
    let delta = std::array::from_fn::<_, 3, _>(|axis| raw[axis] as f32 * 0.125 - camera[axis]);
    let [x, y, z] = delta;
    let squared = z * z + y * y + x * x;
    squared.is_finite() && squared < 256.0
}

#[allow(clippy::too_many_arguments)]
pub fn drain_live_named_audio(
    mut messages: MessageReader<SequencedAudioEvent>,
    world: crate::observations::WorldObservation<'_>,
    clock: crate::observations::SessionObservation,
    receipt: Res<CameraPublicationAttempt>,
    frame: Res<LocalPlayerFrameCarrier>,
    camera_pose: Res<CameraPose>,
    physics: &dyn crate::observations::PhysicsObservation,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut state: ResMut<NamedAudio>,
    mut device: Option<NonSendMut<AudioDevice>>,
) {
    let owner = world.stream.as_ref().map(|stream| AudioOwner {
        session: clock.session_generation(),
        stream: stream.authority().actor_session_id(),
        dimension: stream.current_dimension(),
        epoch: stream.form_dimension_epoch(),
    });
    state.bind(owner);
    let camera = world.stream.as_ref().and_then(|stream| {
        if !stream.audio_default_camera_eligible() {
            return None;
        }
        let proof = receipt.published()?;
        let frame = frame.snapshot()?;
        let state = physics.state()?;
        let physics_world = physics.last_world_identity()?;
        if proof.owner != CameraOwner::current(stream, clock.session_generation())
            || proof.frame_generation != frame.pose_generation()
            || proof.tick != frame.physics_tick()
            || state.tick != proof.tick
            || physics_world != frame.world_collision_identity()
            || proof.owner.session != frame.session_generation()
            || proof.transform != *frame.pose()
            || proof.transform != *camera_pose.transform()
            || proof.perspective != PerspectiveMode::FirstPerson
            || frame.perspective() != proof.perspective
            || cameras.iter().count() != 1
            || cameras.get(proof.entity).ok() != Some(&proof.transform)
        {
            return None;
        }
        Some(proof.transform.translation.to_array())
    });
    if world
        .stream
        .as_ref()
        .is_none_or(|stream| !stream.audio_default_camera_eligible())
    {
        state.cancel_all();
    }
    let available = device.as_ref().is_some_and(|value| value.available());
    for event in messages.read() {
        state.event(event, camera, available);
    }
    if let Some(device) = device.as_mut() {
        state.flush(|source| device.submit(source));
    } else {
        state.cancel_all();
    }
}
#[cfg(test)]
mod tests;

/// Synthetic audio inputs for composed publication tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use assets::RuntimeAudioPcm;
    use sha2::{Digest, Sha256};
    /// Decodes a generated PCM carrier for ownership tests.
    pub fn sample() -> RuntimeAudioPcm {
        let samples: Vec<i16> = (0..64).map(|value| value * 100).collect();
        let pcm: Vec<_> = samples
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let definition = assets::AudioDefinition {
            identifier: "ambient.underwater.loop".into(),
            category: None,
            subtitle: None,
            min_distance: None,
            max_distance: None,
            volume: None,
            pitch: None,
            use_legacy_max_distance: None,
            alternatives: vec![assets::AudioAlternative {
                object_form: true,
                name: "sounds/test/finite".into(),
                weight: 1,
                volume: None,
                pitch: None,
                is_3d: Some(false),
                stream: Some(true),
                load_on_low_memory: None,
            }]
            .into_boxed_slice(),
        };
        let catalog_bytes = assets::encode_audio_catalog([1; 32], [2; 32], &[definition]).unwrap();
        let expected = assets::AudioPcmExpectedIdentity::new(
            "ambient.underwater.loop",
            "sounds/test/finite.fsb",
            Sha256::digest(&catalog_bytes).into(),
            [1; 32],
            [2; 32],
            [3; 32],
            Sha256::digest(pcm).into(),
            208,
            2,
            48000,
            32,
        )
        .unwrap();
        let bytes = assets::encode_audio_pcm(&expected, &samples).unwrap();
        RuntimeAudioPcm::decode(
            &bytes,
            &assets::RuntimeAudioCatalog::decode(&catalog_bytes).unwrap(),
            expected.catalog_sha256(),
            &expected,
        )
        .unwrap()
    }
}
