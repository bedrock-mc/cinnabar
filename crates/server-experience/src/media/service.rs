//! Native declarative playback controller, independent of a downloaded component.

use super::{
    clock::{Clock, Correction, MAX_PROBE_DELAY_US, correction},
    descriptor::Descriptor,
    frames::{PcmBlock, VideoFrame},
    timeline::{Message, Playback},
    worker::Worker,
};
use crate::{bundle::VerifiedBundle, manifest::Permission, negotiation::Grant, runtime::Principal};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

#[cfg(test)]
mod eof_fuzz;
pub(crate) mod output;

const MAX_EVENTS: usize = 16;

pub struct Player {
    owner: Principal,
    epoch: u64,
    expires_unix: u64,
    descriptor: Descriptor,
    origins: BTreeSet<String>,
    data_budget: Arc<AtomicU64>,
    helper: PathBuf,
    clock: Clock,
    ping: Option<(u64, u64)>,
    ping_id: u64,
    last_ping_us: u64,
    playback: Playback,
    output: output::Queues,
    worker: Option<Worker>,
    decoder_generation: u64,
    decoder_ended: bool,
    presented_us: Option<u64>,
    announce_playing: bool,
    ended: bool,
    events: Vec<Event>,
    pub buffering: bool,
}

/// Playback transitions reported back to the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Playing,
    Paused,
    Stopped,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    pub position_us: u64,
}

impl Player {
    /// Resolves a media ID only from a verified bundle and an unexpired, consented grant.
    pub fn prepare(
        bundle: &VerifiedBundle,
        path: &str,
        grant: &Grant,
        epoch: u64,
        now_unix: u64,
        data_budget: Arc<AtomicU64>,
        helper: PathBuf,
    ) -> Result<Self> {
        ensure!(
            now_unix < grant.expires_unix
                && grant
                    .offer
                    .offer
                    .scope
                    .permissions
                    .contains(&Permission::Media)
                && bundle.manifest.permissions.contains(&Permission::Media),
            "media permission denied"
        );
        ensure!(
            grant
                .offer
                .offer
                .packages
                .iter()
                .any(|p| p.id == bundle.manifest.id
                    && p.publisher_key == bundle.manifest.publisher_key
                    && p.digest == bundle.digest()),
            "foreign bundle"
        );
        let bytes = bundle
            .file(path)
            .ok_or_else(|| anyhow::anyhow!("undeclared media descriptor"))?;
        ensure!(
            bytes.len() <= crate::policy::MAX_MARKER_BYTES,
            "media descriptor too large"
        );
        let descriptor: Descriptor = serde_json::from_slice(bytes)?;
        descriptor.validate(&grant.offer.offer.scope.origins)?;
        ensure!(
            bundle.file(&descriptor.poster).is_some(),
            "fallback poster missing"
        );
        Ok(Self {
            owner: Principal {
                session: grant.session.clone(),
                bundle: bundle.manifest.id.clone(),
                generation: crate::policy::INITIAL_BUNDLE_GENERATION,
            },
            epoch,
            expires_unix: grant.expires_unix,
            descriptor,
            origins: grant.offer.offer.scope.origins.clone(),
            data_budget,
            helper,
            clock: Clock::local(),
            ping: None,
            ping_id: 0,
            last_ping_us: 0,
            playback: Playback::default(),
            output: output::Queues::default(),
            worker: None,
            decoder_generation: 0,
            decoder_ended: false,
            presented_us: None,
            announce_playing: false,
            ended: false,
            events: Vec::new(),
            buffering: true,
        })
    }

    /// Issues coarse clock probes only on an already negotiated extension channel.
    pub fn ping(&mut self, now_us: u64) -> Option<(u64, u64)> {
        if self
            .ping
            .is_some_and(|(_, sent)| now_us.saturating_sub(sent) <= MAX_PROBE_DELAY_US)
            || now_us.saturating_sub(self.last_ping_us) < 1_000_000
        {
            return None;
        }
        self.ping_id = self.ping_id.checked_add(1)?;
        self.last_ping_us = now_us;
        self.ping = Some((self.ping_id, now_us));
        self.ping
    }

    /// Accepts only a response to this player's outstanding probe.
    pub fn clock_reply(&mut self, id: u64, c0: u64, s1: u64, s2: u64, c3: u64) -> Result<()> {
        ensure!(self.ping == Some((id, c0)), "unsolicited clock reply");
        self.ping = None;
        self.clock.observe(c0, s1, s2, c3)
    }

    /// Queues a validated server control without starting network or decode work.
    pub fn control(&mut self, message: Message, local_us: u64) -> Result<()> {
        let (server_us, _) = self
            .clock
            .server_now(local_us)
            .ok_or_else(|| anyhow::anyhow!("media clock unavailable"))?;
        if let super::timeline::Operation::Prepare { media_id } = &message.operation {
            ensure!(media_id == &self.descriptor.id, "unknown media ID");
        }
        self.playback.enqueue(
            message,
            &self.owner,
            self.epoch,
            &self.descriptor.timeline,
            server_us,
        )
    }

    /// Applies due controls, keeps one helper decoding the current generation and holds the
    /// timeline while it rebuffers; never waits.
    pub fn tick(&mut self, now_unix: u64, local_us: u64, autoplay: bool) -> Result<()> {
        if now_unix >= self.expires_unix {
            self.stop_decoder();
            anyhow::bail!("media grant expired");
        }
        let Some((server_us, _)) = self.clock.server_now(local_us) else {
            self.stop_decoder();
            return Ok(());
        };
        let duration = self.descriptor.duration_us;
        let (was_playing, was_stopped, was_generation) = (
            self.playback.playing,
            self.playback.stopped,
            self.playback.decode_generation,
        );
        self.playback.advance(server_us, duration)?;
        let position = self.playback.position(server_us, duration);
        if self.playback.stopped && !was_stopped {
            self.ended = false;
            self.event(EventKind::Stopped, position);
        } else if was_playing && !self.playback.playing {
            self.event(EventKind::Paused, position);
        }
        if self.playback.playing
            && (!was_playing
                || self.playback.decode_generation != was_generation && !self.playback.looped)
        {
            self.ended = false;
            self.announce_playing = true;
        }
        if !autoplay || self.playback.stopped || self.playback.decode_generation == 0 {
            self.stop_decoder();
            return Ok(());
        }
        if self.decoder_generation != self.playback.decode_generation {
            self.stop_decoder();
            // A replaced helper's IPC thread releases the decoder slot shortly after it dies.
            if !super::worker::DecoderLease::free() {
                if self.playback.playing {
                    self.playback.hold(server_us, duration);
                }
                return Ok(());
            }
            self.worker = Some(Worker::start(
                &self.helper,
                self.descriptor.clone(),
                self.origins.clone(),
                self.playback.decode_generation,
                Arc::clone(&self.data_budget),
                position,
            )?);
            self.decoder_generation = self.playback.decode_generation;
        }
        let worker = &self.worker;
        self.decoder_ended |= self.output.pump(self.decoder_generation, || {
            worker.as_ref().and_then(Worker::poll)
        })?;
        if !self.playback.playing || self.ended {
            return Ok(());
        }
        if !self.output.frames.is_empty() {
            self.playback.release();
        } else {
            let interval = 1_000_000 / u64::from(self.descriptor.fps);
            if self.decoder_ended && self.output.pcm.is_empty() {
                // The last frame stays up for its interval and queued audio plays out first.
                let end = self
                    .presented_us
                    .map_or(0, |shown| shown.saturating_add(interval))
                    .max(self.output.audio_end_us)
                    .min(duration);
                if position >= end {
                    self.playback.finish(server_us, duration);
                    self.ended = true;
                    self.event(EventKind::Ended, position);
                } else {
                    // Nothing more will arrive; the last frame and audio need time to play out.
                    self.playback.release();
                }
            } else if !self.decoder_ended
                && self
                    .presented_us
                    .is_none_or(|shown| position > shown.saturating_add(2 * interval))
            {
                self.playback.hold(server_us, duration);
                self.buffering = true;
            } else {
                self.playback.release();
            }
        }
        Ok(())
    }

    /// Presents the newest due frame on the media clock, which audio is corrected toward.
    pub fn video(&mut self, local_us: u64) -> Option<VideoFrame> {
        let (server_us, _) = self.clock.server_now(local_us)?;
        let desired = self
            .playback
            .position(server_us, self.descriptor.duration_us);
        let frame = self
            .output
            .frames
            .present(desired, self.decoder_generation)?;
        self.presented_us = Some(frame.pts_us);
        self.buffering = false;
        if std::mem::take(&mut self.announce_playing) {
            self.event(EventKind::Playing, desired);
        }
        Some(frame)
    }

    /// Current media-clock position, the master for both outputs.
    pub fn position_us(&self, local_us: u64) -> Option<u64> {
        let (server_us, _) = self.clock.server_now(local_us)?;
        Some(
            self.playback
                .position(server_us, self.descriptor.duration_us),
        )
    }

    /// True once playback reached the presentation end.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Decoder discontinuity counter; audio queued under another value is stale.
    pub fn decoder_generation(&self) -> u64 {
        self.decoder_generation
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    /// Transitions since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn event(&mut self, kind: EventKind, position_us: u64) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(Event { kind, position_us });
        }
    }

    /// Reports a drift decision; an output adapter must perform rate or seek correction.
    pub fn drift(&self, local_us: u64, audible_us: u64) -> Option<Correction> {
        let (server_us, _) = self.clock.server_now(local_us)?;
        Some(correction(
            audible_us,
            self.playback
                .position(server_us, self.descriptor.duration_us),
        ))
    }

    /// Hands one bounded block to the host mixer, never to guest code.
    pub fn take_pcm(&mut self) -> Option<PcmBlock> {
        self.output.pcm.pop_front()
    }

    /// The next block, so the mixer can check its ring has room before taking it.
    pub fn peek_pcm(&self) -> Option<&PcmBlock> {
        self.output.pcm.front()
    }

    /// Exposes authoritative pause, gain and surface state to trusted adapters.
    pub fn playback(&self) -> &Playback {
        &self.playback
    }

    /// Drops all generation-owned decode output immediately.
    fn stop_decoder(&mut self) {
        self.worker = None;
        self.output = output::Queues::default();
        self.buffering = true;
        self.decoder_generation = 0;
        self.decoder_ended = false;
        self.presented_us = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{
            ranges::tests::descriptor_for,
            timeline::{INITIAL_MEDIA_GENERATION, INITIAL_MEDIA_INSTANCE, Operation},
        },
        *,
    };

    /// A player for the 500 ms, 10 fps fixture descriptor, without a bundle or helper.
    pub(super) fn player() -> Player {
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: crate::policy::INITIAL_BUNDLE_GENERATION,
        };
        Player {
            owner,
            epoch: 1,
            expires_unix: u64::MAX,
            descriptor: descriptor_for(&[0; 16]),
            origins: BTreeSet::new(),
            data_budget: Arc::new(AtomicU64::new(0)),
            helper: PathBuf::from("/nonexistent/cinnabar-media-helper"),
            clock: Clock::local(),
            ping: None,
            ping_id: 0,
            last_ping_us: 0,
            playback: Playback::default(),
            output: output::Queues::default(),
            worker: None,
            decoder_generation: 0,
            decoder_ended: false,
            presented_us: None,
            announce_playing: false,
            ended: false,
            events: Vec::new(),
            buffering: true,
        }
    }

    fn play(player: &mut Player, revision: u64, at_us: u64) {
        control(player, revision, at_us, Operation::Play { position_us: 0 });
    }

    pub(super) fn control(player: &mut Player, revision: u64, at_us: u64, operation: Operation) {
        let message = Message {
            owner: player.owner.clone(),
            instance: INITIAL_MEDIA_INSTANCE,
            generation: INITIAL_MEDIA_GENERATION,
            timeline: player.descriptor.timeline.clone(),
            world_epoch: 1,
            revision,
            effective_server_us: at_us,
            operation,
        };
        player.control(message, at_us).unwrap();
    }

    #[test]
    fn waiting_for_the_decoder_slot_holds_the_timeline() {
        let _slot = super::super::worker::tests::DECODER_SLOT.lock();
        let lease = super::super::worker::DecoderLease::acquire().unwrap();
        let mut player = player();
        play(&mut player, 1, 0);
        player.tick(0, 0, true).unwrap();
        player.tick(0, 400_000, true).unwrap();
        assert_eq!(
            player.position_us(400_000),
            Some(0),
            "the clip ran without output"
        );
        drop(lease);
    }

    #[test]
    fn a_rebuffering_hold_with_uneven_ticks_is_not_a_loop_wrap() {
        let _slot = super::super::worker::tests::DECODER_SLOT.lock();
        let lease = super::super::worker::DecoderLease::acquire().unwrap();
        let mut player = player();
        control(
            &mut player,
            1,
            0,
            Operation::SetLoop {
                bounds_us: Some([0, 500_000]),
            },
        );
        play(&mut player, 2, 0);
        player.descriptor.duration_us = 10_000_000;
        player.playback.advance(0, 10_000_000).unwrap();
        player.playback.loop_us = Some([0, 10_000_000]);
        player.decoder_generation = player.playback.decode_generation;
        player.presented_us = Some(1_000_000);
        let generation = player.playback.decode_generation;
        // Starved: each tick holds, so the next one sees only its own interval of progress.
        for now in [3_000_000, 3_100_000, 3_150_000, 3_300_000, 3_310_000] {
            player.tick(0, now, true).unwrap();
        }
        assert_eq!(
            player.playback.decode_generation, generation,
            "hold restarted decoding"
        );
        drop(lease);
    }

    #[test]
    fn a_short_loop_restarts_decoding_when_the_timeline_wraps() {
        let _slot = super::super::worker::tests::DECODER_SLOT.lock();
        let lease = super::super::worker::DecoderLease::acquire().unwrap();
        let mut player = player();
        control(
            &mut player,
            1,
            0,
            Operation::SetLoop {
                bounds_us: Some([0, 500_000]),
            },
        );
        play(&mut player, 2, 0);
        player.playback.advance(0, 500_000).unwrap();
        player.decoder_generation = player.playback.decode_generation;
        player.presented_us = Some(400_000);
        player
            .output
            .frames
            .push(
                VideoFrame {
                    generation: player.decoder_generation,
                    pts_us: 450_000,
                    width: 2,
                    height: 2,
                    rgba: vec![0; 16],
                },
                player.decoder_generation,
            )
            .unwrap();
        player.tick(0, 450_000, true).unwrap();
        let before = player.playback.decode_generation;
        player.tick(0, 520_000, true).unwrap();
        assert!(
            player.playback.decode_generation > before,
            "wrap went unnoticed"
        );
        drop(lease);
    }

    #[test]
    fn a_restart_waits_for_the_previous_decoder_to_release_its_lease() {
        let _slot = super::super::worker::tests::DECODER_SLOT.lock();
        let lease = super::super::worker::DecoderLease::acquire().unwrap();
        let mut player = player();
        play(&mut player, 1, 0);
        player.tick(0, 0, true).unwrap();
        assert!(player.worker.is_none() && player.buffering);
        assert_eq!(player.decoder_generation, 0, "restart stays pending");
        drop(lease);
    }

    #[test]
    fn the_end_waits_for_the_last_frame_and_queued_audio_to_play_out() {
        let mut player = player();
        play(&mut player, 1, 0);
        player.playback.advance(0, 500_000).unwrap();
        player.decoder_generation = player.playback.decode_generation;
        player.decoder_ended = true;
        player.presented_us = Some(400_000);
        player.tick(0, 417_000, true).unwrap();
        assert!(
            player.take_events().is_empty(),
            "ended before the last frame's interval"
        );
        player.tick(0, 500_000, true).unwrap();
        assert_eq!(
            player.take_events(),
            [Event {
                kind: EventKind::Ended,
                position_us: 500_000
            }]
        );
    }
}
