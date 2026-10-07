//! Developer media adapter: routes guest `media.control` to decoder players, binds their
//! frames to the same bundle's scene quads and reports playback transitions to that guest.

use anyhow::{Result, ensure};
use client_presentation::audio::{
    media::{self as media_audio, MediaAudio},
    settings::AudioSettings,
};
use server_experience::{
    bundle::VerifiedBundle,
    manifest::Permission,
    media::{
        service::{Event, EventKind, Player},
        timeline::{INITIAL_MEDIA_GENERATION, INITIAL_MEDIA_INSTANCE, Message, Operation},
    },
    negotiation::Grant,
    runtime::{MediaOperation, Principal, SceneObject},
    wire::Scalar,
};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

/// Host-owned channel on which the owning guest hears `[media path, event, position_ms]`.
pub(super) const EVENT_CHANNEL: &str = "cinnabar.media";
/// Shared download allowance across every restart in one session.
const DATA_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_PLAYERS: usize = 4;
const MAX_PENDING: usize = 32;
const MAX_EVENTS: usize = 64;

struct Control {
    bundle: String,
    id: String,
    operation: MediaOperation,
    position_ms: u64,
}

struct Slot {
    player: Player,
    voice: Voice,
}

type Key = (String, String);

/// Whether a slot still counts toward the concurrent player limit.
trait Activity {
    fn active(&self) -> bool;
}

impl Activity for Slot {
    fn active(&self) -> bool {
        !self.player.playback().stopped && !self.player.ended()
    }
}

/// Makes room for one more player within the concurrent limit.
fn admit<V: Activity>(
    players: &mut BTreeMap<Key, V>,
    frames: &mut BTreeMap<Key, Option<render::MediaFrame>>,
) -> Result<()> {
    if players.len() >= MAX_PLAYERS {
        // Finished players keep their last frame only until a new one needs the slot.
        players.retain(|key, player| {
            let keep = player.active();
            if !keep {
                frames.remove(key);
            }
            keep
        });
    }
    ensure!(players.len() < MAX_PLAYERS, "media player limit");
    Ok(())
}

/// One player's mixer voice; only one exists process-wide, so a finished player hands it back.
#[derive(Default)]
struct Voice {
    audio: Option<Arc<MediaAudio>>,
    generation: u64,
}

impl Voice {
    /// Opens, resets or keeps the voice for an active player; an inactive one releases it.
    fn sync(
        &mut self,
        device: Option<&mut client_presentation::named_audio::AudioDevice>,
        generation: u64,
        active: bool,
    ) -> Option<&Arc<MediaAudio>> {
        if !active {
            *self = Self::default();
            return None;
        }
        if self.audio.is_none()
            && generation != 0
            && let Some(device) = device
        {
            self.audio = media_audio::start(device, generation);
            self.generation = generation;
        }
        let audio = self.audio.as_ref()?;
        if self.generation != generation {
            audio.reset(generation);
            self.generation = generation;
        }
        Some(audio)
    }
}

impl Drop for Voice {
    /// Retires the mixer source, which releases the voice permit.
    fn drop(&mut self) {
        if let Some(audio) = &self.audio {
            audio.cancel();
        }
    }
}

pub(super) struct Media {
    grant: Grant,
    epoch: u64,
    helper: PathBuf,
    bundles: BTreeMap<String, VerifiedBundle>,
    players: BTreeMap<(String, String), Slot>,
    frames: BTreeMap<(String, String), Option<render::MediaFrame>>,
    pending: VecDeque<Control>,
    events: VecDeque<(String, Vec<u8>)>,
    data_budget: Arc<AtomicU64>,
    revision: u64,
    serial: u64,
}

impl Media {
    pub(super) fn new(grant: Grant, epoch: u64, helper: PathBuf) -> Self {
        Self {
            grant,
            epoch,
            helper,
            bundles: BTreeMap::new(),
            players: BTreeMap::new(),
            frames: BTreeMap::new(),
            pending: VecDeque::new(),
            events: VecDeque::new(),
            data_budget: Arc::new(AtomicU64::new(DATA_BUDGET_BYTES)),
            revision: 0,
            serial: 0,
        }
    }

    /// Keeps a media-permitted bundle's verified assets for descriptor lookup.
    pub(super) fn register(&mut self, bundle: VerifiedBundle) {
        if bundle.manifest.permissions.contains(&Permission::Media) {
            self.bundles.insert(bundle.manifest.id.clone(), bundle);
        }
    }

    /// Stages a validated guest control; it is applied on the next service pass.
    pub(super) fn queue(
        &mut self,
        owner: &Principal,
        id: String,
        operation: MediaOperation,
        position_ms: u64,
    ) -> Result<()> {
        ensure!(self.pending.len() < MAX_PENDING, "media control queue full");
        self.pending.push_back(Control {
            bundle: owner.bundle.clone(),
            id,
            operation,
            position_ms,
        });
        Ok(())
    }

    /// Applies staged controls and advances every player; a failing player is dropped and
    /// reported as stopped, never failing the session.
    pub(super) fn service(&mut self, now_unix: u64, local_us: u64, autoplay: bool) {
        while let Some(control) = self.pending.pop_front() {
            let key = (control.bundle.clone(), control.id.clone());
            if let Err(error) = self.apply(control, now_unix, local_us) {
                bevy::log::warn!(%error, media = %key.1, "server media control rejected");
                self.players.remove(&key);
                self.frames.remove(&key);
                self.serial += 1;
                self.event(&key, EventKind::Stopped, 0);
            }
        }
        let mut failed = Vec::new();
        for (key, slot) in &mut self.players {
            match slot.player.tick(now_unix, local_us, autoplay) {
                Ok(()) => {
                    if let Some(frame) = slot.player.video(local_us) {
                        self.serial += 1;
                        self.frames.insert(
                            key.clone(),
                            Some(render::MediaFrame {
                                serial: self.serial,
                                width: frame.width,
                                height: frame.height,
                                rgba: Arc::from(frame.rgba),
                            }),
                        );
                    }
                }
                Err(error) => {
                    bevy::log::warn!(%error, media = %key.1, "server media playback stopped");
                    failed.push((key.clone(), slot.player.position_us(local_us).unwrap_or(0)));
                }
            }
        }
        let transitions: Vec<_> = self
            .players
            .iter_mut()
            .flat_map(|(key, slot)| {
                slot.player
                    .take_events()
                    .into_iter()
                    .map(|event| (key.clone(), event))
            })
            .collect();
        for (key, Event { kind, position_us }) in transitions {
            self.event(&key, kind, position_us);
        }
        for (key, position) in failed {
            self.players.remove(&key);
            self.frames.remove(&key);
            self.serial += 1;
            self.event(&key, EventKind::Stopped, position);
        }
    }

    fn apply(&mut self, control: Control, now_unix: u64, local_us: u64) -> Result<()> {
        let key = (control.bundle.clone(), control.id.clone());
        if !self.players.contains_key(&key) {
            admit(&mut self.players, &mut self.frames)?;
            self.serial += 1;
            let bundle = self
                .bundles
                .get(&control.bundle)
                .ok_or_else(|| anyhow::anyhow!("bundle lacks media permission"))?;
            let player = Player::prepare(
                bundle,
                &control.id,
                &self.grant,
                self.epoch,
                now_unix,
                Arc::clone(&self.data_budget),
                self.helper.clone(),
            )?;
            self.players.insert(
                key.clone(),
                Slot {
                    player,
                    voice: Voice::default(),
                },
            );
            self.frames.insert(key.clone(), None);
            self.serial += 1;
        }
        let slot = self.players.get_mut(&key).expect("slot inserted");
        let position_us = control.position_ms.saturating_mul(1000);
        let operation = match control.operation {
            MediaOperation::Prepare => Operation::Prepare {
                media_id: slot.player.descriptor().id.clone(),
            },
            MediaOperation::Play => Operation::Play { position_us },
            // Pausing freezes where playback is; positioning is what play and seek are for.
            MediaOperation::Pause => Operation::Pause {
                position_us: slot.player.position_us(local_us).unwrap_or(0),
            },
            MediaOperation::Seek => Operation::Seek { position_us },
            MediaOperation::Stop => Operation::Stop,
        };
        self.revision += 1;
        let message = Message {
            owner: Principal {
                session: self.grant.session.clone(),
                bundle: control.bundle,
                generation: server_experience::policy::INITIAL_BUNDLE_GENERATION,
            },
            instance: INITIAL_MEDIA_INSTANCE,
            generation: INITIAL_MEDIA_GENERATION,
            timeline: slot.player.descriptor().timeline.clone(),
            world_epoch: self.epoch,
            revision: self.revision,
            effective_server_us: local_us,
            operation,
        };
        slot.player.control(message, local_us)
    }

    /// Feeds decoded PCM to one mixer voice per player and steers it onto the media clock.
    pub(super) fn pump_audio(
        &mut self,
        mut device: Option<&mut client_presentation::named_audio::AudioDevice>,
        settings: &AudioSettings,
        muted: bool,
        local_us: u64,
    ) {
        for slot in self.players.values_mut() {
            let active = !slot.player.playback().stopped && !slot.player.ended();
            let generation = slot.player.decoder_generation();
            let Some(audio) = slot.voice.sync(device.as_deref_mut(), generation, active) else {
                while slot.player.take_pcm().is_some() {}
                continue;
            };
            while let Some(block) = slot.player.peek_pcm() {
                if !audio.has_room(block.samples.len() / usize::from(block.channels)) {
                    break;
                }
                let block = slot.player.take_pcm().expect("peeked block");
                let _ = audio.push(&block);
            }
            let playback = slot.player.playback();
            // Audio keeps draining through a video rebuffer, so the decoder never stalls behind
            // a full PCM queue; drift correction realigns it afterwards.
            audio.pause(!playback.playing);
            let running = playback.playing && !slot.player.buffering;
            audio.update(settings, playback.volume, muted, None, None);
            if running
                && let Some(audible) = audio.audible_position_us()
                && let Some(correction) = slot.player.drift(local_us, audible)
            {
                audio.correct(correction, audible);
            }
        }
    }

    /// The latest frame bound to the bundle's quad textured by `texture`, if one plays.
    /// The latest frame for the bundle's quad textured by `texture`; a linear scan over at
    /// most MAX_PLAYERS entries, so lookups never allocate.
    pub(super) fn frame(&self, bundle: &str, texture: &str) -> Option<Option<render::MediaFrame>> {
        self.frames
            .iter()
            .find(|((owner, path), _)| owner == bundle && path == texture)
            .map(|(_, frame)| frame.clone())
    }

    /// Changes whenever a frame is published or a player's frame entry comes or goes.
    pub(super) fn frames_revision(&self) -> u64 {
        self.serial
    }

    #[cfg(test)]
    pub(super) fn set_frame(
        &mut self,
        bundle: &str,
        texture: &str,
        frame: Option<render::MediaFrame>,
    ) {
        self.frames
            .insert((bundle.to_owned(), texture.to_owned()), frame);
        self.serial += 1;
    }

    /// Transitions for guest dispatch, oldest first.
    pub(super) fn next_event(&mut self) -> Option<(String, Vec<u8>)> {
        self.events.pop_front()
    }

    /// Restores an event the guest could not take yet.
    pub(super) fn defer_event(&mut self, event: (String, Vec<u8>)) {
        self.events.push_front(event);
    }

    fn event(&mut self, (bundle, id): &(String, String), kind: EventKind, position_us: u64) {
        bevy::log::info!(media = %id, ?kind, position_us, "server media event");
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        let choice = match kind {
            EventKind::Playing => 0,
            EventKind::Paused => 1,
            EventKind::Stopped => 2,
            EventKind::Ended => 3,
        };
        let record = vec![
            Scalar::Text(id.clone()),
            Scalar::Choice(choice),
            Scalar::Integer(i64::try_from(position_us / 1000).unwrap_or(i64::MAX)),
        ];
        if let Ok(bytes) = serde_json::to_vec(&record) {
            self.events.push_back((bundle.clone(), bytes));
        }
    }
}

/// Places a quad from a scene object the way the renderer draws a media screen.
pub(super) fn screen(id: u64, object: &SceneObject) -> Option<(&str, render::MediaScreen)> {
    let SceneObject::Quad {
        texture,
        transform,
        size,
    } = object
    else {
        return None;
    };
    let (center, half_right, half_up) = render::media_screen_axes(
        [transform[0], transform[1], transform[2]],
        [transform[3], transform[4], transform[5], transform[6]],
        [transform[7], transform[8], transform[9]],
        *size,
    );
    Some((
        texture.as_str(),
        render::MediaScreen {
            id,
            center,
            half_right,
            half_up,
            frame: None,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_quad_is_placed_from_its_scene_transform() {
        let yaw = std::f32::consts::FRAC_PI_2;
        let object = SceneObject::Quad {
            texture: "media/clip.json".into(),
            transform: [
                10.0,
                70.0,
                -4.0,
                0.0,
                (-yaw / 2.0).sin(),
                0.0,
                (-yaw / 2.0).cos(),
                1.0,
                1.0,
                1.0,
            ],
            size: [24.0, 13.5],
        };
        let (texture, quad) = screen(7, &object).unwrap();
        assert_eq!(texture, "media/clip.json");
        assert_eq!(quad.center, [10.0, 70.0, -4.0]);
        assert!((quad.half_up[1] - 6.75).abs() < 1e-4);
        let right = quad.half_right;
        assert!((right[0].hypot(right[2]) - 12.0).abs() < 1e-4 && right[1].abs() < 1e-4);
        assert!(
            right[0].abs() < 1e-4,
            "a quarter turn puts the width along Z: {right:?}"
        );
        let mesh = SceneObject::Particles {
            effect: "fx".into(),
            transform: [0.0; 10],
            count: 1,
        };
        assert!(screen(1, &mesh).is_none());
    }

    #[test]
    fn a_finished_player_hands_the_single_mixer_voice_to_the_next() {
        let (mut device, mut mixer) = client_presentation::named_audio::AudioDevice::memory_mixer();
        let mut first = Voice::default();
        assert!(first.sync(Some(&mut device), 1, true).is_some());
        assert!(first.sync(Some(&mut device), 1, false).is_none());
        // The mixer retires the cancelled source on its next pull, releasing the permit.
        mixer.by_ref().take(64).for_each(drop);
        let mut second = Voice::default();
        assert!(second.sync(Some(&mut device), 1, true).is_some());
    }

    struct Finished(bool);

    impl Activity for Finished {
        fn active(&self) -> bool {
            !self.0
        }
    }

    #[test]
    fn finished_players_free_their_slots_so_the_limit_is_concurrent() {
        let key = |i: usize| ("cinema".to_owned(), format!("media/{i}.json"));
        let mut players: BTreeMap<Key, Finished> =
            (0..MAX_PLAYERS).map(|i| (key(i), Finished(true))).collect();
        let mut frames = players.keys().map(|key| (key.clone(), None)).collect();
        admit(&mut players, &mut frames).unwrap();
        assert!(players.len() < MAX_PLAYERS && frames.len() == players.len());
        let mut busy: BTreeMap<Key, Finished> = (0..MAX_PLAYERS)
            .map(|i| (key(i), Finished(false)))
            .collect();
        assert!(admit(&mut busy, &mut BTreeMap::new()).is_err());
    }

    #[test]
    fn transitions_reach_the_guest_as_typed_records() {
        let grant = Grant {
            offer: server_experience::negotiation::VerifiedOffer {
                offer: server_experience::manifest::Offer {
                    version: server_experience::policy::WIRE_VERSION,
                    audience: String::new(),
                    server_key: String::new(),
                    revision: 1,
                    expires_unix: u64::MAX,
                    scope: server_experience::manifest::Scope {
                        permissions: Default::default(),
                        origins: Default::default(),
                        memory_bytes: 0,
                        gpu_bytes: 0,
                    },
                    packages: Vec::new(),
                    fallback: String::new(),
                    carrier: protocol::EXPERIENCE_CHANNEL.into(),
                },
                digest: String::new(),
            },
            session: "session".into(),
            connection: "connection".into(),
            subclient: 0,
            expires_unix: u64::MAX,
        };
        let mut media = Media::new(grant, 1, PathBuf::new());
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: 1,
        };
        media
            .queue(&owner, "media/clip.json".into(), MediaOperation::Play, 0)
            .unwrap();
        media.service(0, 0, true);
        let (bundle, record) = media.next_event().unwrap();
        assert_eq!(bundle, "cinema");
        let record: Vec<Scalar> = serde_json::from_slice(&record).unwrap();
        assert!(matches!(
            record.as_slice(),
            [Scalar::Text(id), Scalar::Choice(2), Scalar::Integer(0)] if id == "media/clip.json"
        ));
        assert!(media.frame("cinema", "media/clip.json").is_none());
    }
}
