//! Revisioned server controls; future starts never take effect early.

use super::MAX_DURATION_US;
use crate::runtime::Principal;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The media instance admitted by the initial routing contract.
pub const INITIAL_MEDIA_INSTANCE: u32 = 1;
/// The first route generation, independent of decoder discontinuity generations.
pub const INITIAL_MEDIA_GENERATION: u64 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    Ui {
        widget: String,
    },
    Quad {
        object: u32,
        generation: u64,
    },
    Entity {
        runtime_id: u64,
        generation: u64,
        material_slot: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Prepare { media_id: String },
    Play { position_us: u64 },
    Pause { position_us: u64 },
    Seek { position_us: u64 },
    SetLoop { bounds_us: Option<[u64; 2]> },
    SetVolume { per_mille: u16 },
    Attach { surface: Surface },
    Detach,
    Stop,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub owner: Principal,
    pub instance: u32,
    pub generation: u64,
    pub timeline: String,
    pub world_epoch: u64,
    pub revision: u64,
    pub effective_server_us: u64,
    pub operation: Operation,
}

#[derive(Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub stopped: bool,
    pub media_id: Option<String>,
    /// Unwrapped media time at `anchor_us`; looping only wraps what is shown, so loop
    /// iterations are always `(position - loop start) / loop length`.
    position_us: u64,
    anchor_us: u64,
    pub loop_us: Option<[u64; 2]>,
    pub volume: u16,
    pub surface: Option<Surface>,
    pub decode_generation: u64,
    held: bool,
    /// Loop ends crossed by elapsed playback since the last `advance`.
    crossings: u64,
    /// The last `advance` restarted decoding only because playback looped.
    pub looped: bool,
    revision: u64,
    last_effective_us: Option<u64>,
    pending: VecDeque<Message>,
}

impl Default for Playback {
    /// Full volume until the server sets one; mute and sliders still apply on top.
    fn default() -> Self {
        Self {
            playing: false,
            stopped: false,
            media_id: None,
            position_us: 0,
            anchor_us: 0,
            loop_us: None,
            volume: 1000,
            surface: None,
            decode_generation: 0,
            held: false,
            crossings: 0,
            looped: false,
            revision: 0,
            last_effective_us: None,
            pending: VecDeque::new(),
        }
    }
}

impl Playback {
    /// Rejects stale routes, revisions and unbounded scheduling before mutating state.
    pub fn enqueue(
        &mut self,
        message: Message,
        owner: &Principal,
        epoch: u64,
        timeline: &str,
        now_us: u64,
    ) -> Result<()> {
        ensure!(
            &message.owner == owner
                && message.world_epoch == epoch
                && message.timeline == timeline
                && message.instance == INITIAL_MEDIA_INSTANCE
                && message.generation == INITIAL_MEDIA_GENERATION,
            "stale media route"
        );
        ensure!(
            message.revision > self.revision && self.pending.len() < 32,
            "media revision or queue rejected"
        );
        ensure!(
            message.effective_server_us <= now_us.saturating_add(30_000_000),
            "media schedule too distant"
        );
        ensure!(
            self.last_effective_us
                .is_none_or(|last| last <= message.effective_server_us),
            "media controls reordered"
        );
        match &message.operation {
            Operation::Play { position_us }
            | Operation::Pause { position_us }
            | Operation::Seek { position_us } => {
                ensure!(*position_us <= MAX_DURATION_US, "media position too large")
            }
            Operation::SetLoop {
                bounds_us: Some([start, end]),
            } => ensure!(start < end && *end <= MAX_DURATION_US, "invalid loop range"),
            Operation::SetVolume { per_mille } => ensure!(*per_mille <= 1000, "invalid volume"),
            _ => {}
        }
        self.revision = message.revision;
        self.last_effective_us = Some(message.effective_server_us);
        self.pending.push_back(message);
        Ok(())
    }

    /// Applies due controls at their authored time, preserving the common timeline, and
    /// restarts decoding once when elapsed playback crossed a loop end and no control did.
    pub fn advance(&mut self, server_us: u64, duration_us: u64) -> Result<()> {
        let generation = self.decode_generation;
        while self
            .pending
            .front()
            .is_some_and(|message| message.effective_server_us <= server_us)
        {
            let message = self.pending.pop_front().expect("front checked");
            let authored = message.effective_server_us;
            self.run_to(authored, duration_us);
            self.held = false;
            match message.operation {
                Operation::Prepare { media_id } => {
                    self.media_id = Some(media_id);
                    self.playing = false;
                    self.stopped = false;
                    self.reset_decode()?;
                }
                Operation::Play { position_us } => {
                    self.place(position_us.min(duration_us), authored);
                    self.playing = true;
                    self.stopped = false;
                    self.reset_decode()?;
                }
                Operation::Pause { position_us } => {
                    self.place(position_us.min(duration_us), authored);
                    self.playing = false;
                }
                Operation::Seek { position_us } => {
                    self.place(position_us.min(duration_us), authored);
                    self.reset_decode()?;
                }
                Operation::SetLoop { bounds_us } => {
                    ensure!(
                        bounds_us.is_none_or(|[_, end]| end <= duration_us),
                        "loop exceeds duration"
                    );
                    // New bounds continue from what is shown under the old ones.
                    self.position_us = self.position(message.effective_server_us, duration_us);
                    self.loop_us = bounds_us;
                }
                Operation::SetVolume { per_mille } => self.volume = per_mille,
                Operation::Attach { surface } => self.surface = Some(surface),
                Operation::Detach => self.surface = None,
                Operation::Stop => {
                    self.playing = false;
                    self.stopped = true;
                    self.surface = None;
                    self.reset_decode()?;
                }
            }
        }
        self.run_to(server_us, duration_us);
        self.looped =
            std::mem::take(&mut self.crossings) > 0 && self.decode_generation == generation;
        if self.looped {
            self.reset_decode()?;
        }
        Ok(())
    }

    /// Shown position: the unwrapped timeline wrapped into the loop, or clamped to the end.
    pub fn position(&self, server_us: u64, duration_us: u64) -> u64 {
        let position = self.unwrapped(server_us);
        match self.loop_us {
            Some([start, end]) if position >= end => start + (position - start) % (end - start),
            _ => position.min(duration_us),
        }
    }

    /// Keeps the timeline from advancing while the decoder rebuffers; call every starved tick.
    pub fn hold(&mut self, server_us: u64, duration_us: u64) {
        self.run_to(server_us, duration_us);
        self.held = true;
    }

    /// Lets a held timeline advance again from the time it was last accounted.
    pub fn release(&mut self) {
        self.held = false;
    }

    /// Stops advancing at the current position once the stream has ended.
    pub fn finish(&mut self, server_us: u64, duration_us: u64) {
        self.hold(server_us, duration_us);
        self.playing = false;
    }

    /// Puts the playhead at `position` as of the control's authored time, so a control that
    /// arrives late lands where it would have on time.
    fn place(&mut self, position: u64, authored_us: u64) {
        self.position_us = position;
        self.anchor_us = authored_us;
    }

    fn unwrapped(&self, server_us: u64) -> u64 {
        if self.playing && !self.held {
            self.position_us
                .saturating_add(server_us.saturating_sub(self.anchor_us))
        } else {
            self.position_us
        }
    }

    fn iteration(&self, position: u64) -> u64 {
        self.loop_us.map_or(0, |[start, end]| {
            position.saturating_sub(start) / (end - start)
        })
    }

    /// Accounts elapsed playback up to `server_us`, counting the loop ends it crossed.
    fn run_to(&mut self, server_us: u64, duration_us: u64) {
        let mut next = self.unwrapped(server_us);
        if self.loop_us.is_none() {
            next = next.min(duration_us.max(self.position_us));
        }
        self.crossings += self
            .iteration(next)
            .saturating_sub(self.iteration(self.position_us));
        self.position_us = next;
        self.anchor_us = self.anchor_us.max(server_us);
    }

    /// Invalidates queued PCM, frames and range reads on discontinuity.
    fn reset_decode(&mut self) -> Result<()> {
        self.decode_generation = self
            .decode_generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("media generation exhausted"))?;
        Ok(())
    }
}

#[cfg(test)]
mod loop_fuzz;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::INITIAL_BUNDLE_GENERATION;

    /// Builds one controlled future command without a network or decoder.
    fn message(owner: &Principal, revision: u64, at: u64, operation: Operation) -> Message {
        Message {
            owner: owner.clone(),
            instance: INITIAL_MEDIA_INSTANCE,
            generation: INITIAL_MEDIA_GENERATION,
            timeline: "cinema".into(),
            world_epoch: 1,
            revision,
            effective_server_us: at,
            operation,
        }
    }

    #[test]
    fn initial_route_rejects_other_media_instances_and_generations() {
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: INITIAL_BUNDLE_GENERATION,
        };
        let mut playback = Playback::default();
        let initial = message(&owner, 1, 0, Operation::Stop);
        let mut other_instance = initial.clone();
        other_instance.instance = INITIAL_MEDIA_INSTANCE + 1;
        assert!(
            playback
                .enqueue(other_instance, &owner, 1, "cinema", 0)
                .is_err()
        );
        let mut other_generation = initial.clone();
        other_generation.generation = INITIAL_MEDIA_GENERATION + 1;
        assert!(
            playback
                .enqueue(other_generation, &owner, 1, "cinema", 0)
                .is_err()
        );
        playback.enqueue(initial, &owner, 1, "cinema", 0).unwrap();
        assert_eq!(playback.pending.len(), 1);
    }

    #[test]
    fn applied_controls_keep_the_last_accepted_timestamp() {
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: INITIAL_BUNDLE_GENERATION,
        };
        let mut playback = Playback::default();
        playback
            .enqueue(
                message(&owner, 1, 100, Operation::Play { position_us: 10 }),
                &owner,
                1,
                "cinema",
                100,
            )
            .unwrap();
        playback.advance(200, 1000).unwrap();
        assert!(playback.pending.is_empty());
        assert!(
            playback
                .enqueue(
                    message(&owner, 2, 99, Operation::SetVolume { per_mille: 500 }),
                    &owner,
                    1,
                    "cinema",
                    200
                )
                .is_err()
        );
        assert_eq!(playback.position(200, 1000), 110);
        playback
            .enqueue(
                message(&owner, 2, 100, Operation::SetVolume { per_mille: 500 }),
                &owner,
                1,
                "cinema",
                200,
            )
            .unwrap();
        playback.advance(200, 1000).unwrap();
        assert_eq!(playback.position(200, 1000), 110);
        assert_eq!(playback.volume, 500);
    }

    #[test]
    fn a_rebuffering_hold_freezes_the_timeline_until_released() {
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: INITIAL_BUNDLE_GENERATION,
        };
        let mut playback = Playback::default();
        playback
            .enqueue(
                message(&owner, 1, 0, Operation::Play { position_us: 0 }),
                &owner,
                1,
                "cinema",
                0,
            )
            .unwrap();
        playback.advance(0, 10_000_000).unwrap();
        for now in [400_000, 900_000, 1_400_000] {
            playback.hold(now, 10_000_000);
        }
        assert_eq!(playback.position(1_400_000, 10_000_000), 400_000);
        playback.release();
        assert_eq!(playback.position(1_500_000, 10_000_000), 500_000);
        playback.finish(1_500_000, 10_000_000);
        assert!(!playback.playing);
        assert_eq!(playback.position(9_000_000, 10_000_000), 500_000);
    }

    #[test]
    fn scheduled_play_waits_and_seek_revokes_old_decoder_output() {
        let owner = Principal {
            session: "session".into(),
            bundle: "cinema".into(),
            generation: INITIAL_BUNDLE_GENERATION,
        };
        let mut playback = Playback::default();
        playback
            .enqueue(
                message(&owner, 1, 1_000_000, Operation::Play { position_us: 0 }),
                &owner,
                1,
                "cinema",
                0,
            )
            .unwrap();
        playback.advance(999_999, 10_000_000).unwrap();
        assert!(!playback.playing);
        playback.advance(1_250_000, 10_000_000).unwrap();
        assert_eq!(playback.position(1_250_000, 10_000_000), 250_000);
        let previous = playback.decode_generation;
        playback
            .enqueue(
                message(
                    &owner,
                    2,
                    1_250_000,
                    Operation::Seek {
                        position_us: 5_000_000,
                    },
                ),
                &owner,
                1,
                "cinema",
                1_250_000,
            )
            .unwrap();
        playback.advance(1_250_000, 10_000_000).unwrap();
        assert!(playback.decode_generation > previous);
        assert_eq!(playback.position(1_250_000, 10_000_000), 5_000_000);
        assert!(
            playback
                .enqueue(
                    message(&owner, 2, 1_250_000, Operation::Stop),
                    &owner,
                    1,
                    "cinema",
                    1_250_000
                )
                .is_err()
        );
        assert!(
            playback
                .enqueue(
                    message(&owner, 3, 1_250_000, Operation::Stop),
                    &owner,
                    2,
                    "cinema",
                    1_250_000
                )
                .is_err()
        );
    }
}
