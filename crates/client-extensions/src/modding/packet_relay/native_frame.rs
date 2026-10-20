//! Latest-value native callbacks: neither frame input nor results can form a backlog.

use super::Request;
use mod_api::native_frame::Frame;
use mod_host::CameraDelta;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

/// Results older than two client frames are never applied to a new camera pose.
const MAX_RESULT_AGE: u64 = 2;

/// A session-owned sender, weakly reachable by the app without retaining the session.
pub(super) struct FrameLink {
    requests: Sender<Request>,
    frames: Arc<NativeFrames>,
}

impl FrameLink {
    /// Whether the session negotiated native frames and still holds its source grants.
    pub(super) fn available(&self) -> bool {
        self.frames.available()
    }

    /// Binds one mailbox to the already serialized packet worker.
    pub(super) fn new(requests: Sender<Request>, frames: Arc<NativeFrames>) -> Self {
        Self { requests, frames }
    }

    /// Publishes without blocking; a busy worker sees only the newest pending input.
    pub(super) fn exchange(&self, frame: Option<Frame>) -> Option<CameraDelta> {
        let (output, wake) = self.frames.exchange(frame)?;
        if wake && self.requests.send(Request::NativeFrame).is_err() {
            self.frames.deactivate();
            return None;
        }
        output
    }

    /// Revokes native work synchronously when the session requests stop.
    pub(super) fn deactivate(&self) {
        self.frames.deactivate();
    }
}

#[derive(Default)]
struct State {
    epoch: u64,
    sequence: u64,
    scope: Option<(u64, i32)>,
    attack_held: Option<bool>,
    pending: Option<Input>,
    output: Option<Output>,
    wake_pending: bool,
}

/// One immutable coalesced observation and its host-only input ownership fence.
pub(super) struct Input {
    pub(super) epoch: u64,
    sequence: u64,
    pub(super) frame: Option<Frame>,
}

impl Input {
    /// Preserves an input-loss reset even when newer valid input replaced the unavailable frame.
    pub(super) fn dispatch<R, E>(
        &self,
        previous_epoch: &mut Option<u64>,
        mut callback: impl FnMut(Option<&Frame>) -> Result<R, E>,
    ) -> Result<R, E> {
        if *previous_epoch != Some(self.epoch) && self.frame.is_some() {
            callback(None)?;
        }
        *previous_epoch = Some(self.epoch);
        callback(self.frame.as_ref())
    }
}

struct Output {
    epoch: u64,
    sequence: u64,
    delta: CameraDelta,
}

/// Shared data holds no sender and cannot keep the packet worker's channel alive.
pub(super) struct NativeFrames {
    state: Mutex<State>,
    ready: AtomicBool,
    stopped: AtomicBool,
    invalidate: AtomicBool,
    authority: Arc<AtomicBool>,
}

impl NativeFrames {
    /// A stopped or revoked session cannot regain authority through an in-flight activation.
    fn available(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.stopped.load(Ordering::Acquire)
            && self.authority.load(Ordering::Acquire)
    }

    /// Starts unavailable until the guest successfully starts with negotiated native support.
    pub(super) fn new(authority: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            invalidate: AtomicBool::new(false),
            authority,
        }
    }

    /// Makes only a successfully started, capable guest available to client frames.
    pub(super) fn activate(&self, ready: bool) {
        self.ready.store(
            ready && !self.stopped.load(Ordering::Acquire),
            Ordering::Release,
        );
    }

    /// Clears all pending effects before another session can acquire camera ownership.
    pub(super) fn deactivate(&self) {
        self.ready.store(false, Ordering::Release);
        self.stopped.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.epoch = state.epoch.wrapping_add(1);
        state.pending = None;
        state.output = None;
        state.wake_pending = false;
    }

    /// Packet and timed callbacks may change guest policy; older observations cannot outlive them.
    pub(super) fn fence_guest_state(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending = None;
        state.output = None;
    }

    /// Returns a recent effect and whether the worker needs its one queued wake.
    fn exchange(&self, frame: Option<Frame>) -> Option<(Option<CameraDelta>, bool)> {
        if !self.available() {
            return None;
        }
        let frame = frame.filter(Frame::is_valid);
        if frame.is_none() {
            self.invalidate.store(true, Ordering::Release);
        }
        let mut state = self.state.try_lock().ok()?;
        let scope = frame.as_ref().map(|frame| (frame.session, frame.dimension));
        let attack_held = frame.as_ref().map(|frame| frame.attack_held);
        if state.attack_held != attack_held {
            state.attack_held = attack_held;
            state.output = None;
        }
        let invalidated = self.invalidate.swap(false, Ordering::AcqRel);
        if scope.is_none() && state.scope.is_none() && state.sequence != 0 {
            state.output = None;
            return Some((None, false));
        }
        if state.scope != scope || invalidated {
            state.epoch = state.epoch.wrapping_add(1);
            state.scope = scope;
            state.output = None;
        }
        state.sequence = state.sequence.wrapping_add(1);
        let output = state
            .output
            .take()
            .filter(|output| {
                scope.is_some()
                    && output.epoch == state.epoch
                    && state.sequence.wrapping_sub(output.sequence) <= MAX_RESULT_AGE
            })
            .map(|output| output.delta);
        state.pending = Some(Input {
            epoch: state.epoch,
            sequence: state.sequence,
            frame,
        });
        let wake = !state.wake_pending;
        state.wake_pending = true;
        Some((output, wake))
    }

    /// Removes the newest pending input while allowing the app to queue one later wake.
    pub(super) fn take_input(&self) -> Option<Input> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.wake_pending = false;
        let input = state.pending.take();
        self.available().then_some(input).flatten()
    }

    /// Publishes only a still-current observation; stale worker results are discarded here too.
    pub(super) fn complete(&self, input: Input, delta: Option<CameraDelta>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.available()
            && input.epoch == state.epoch
            && input.frame.as_ref().map(|frame| frame.attack_held) == state.attack_held
            && state.sequence.wrapping_sub(input.sequence) <= MAX_RESULT_AGE
        {
            state.output = delta.map(|delta| Output {
                epoch: input.epoch,
                sequence: input.sequence,
                delta,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the wire fixture's exact 64-bit identities in a deterministic input.
    fn frame() -> Frame {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../../mod-api/native-frame-v1.json")).unwrap();
        serde_json::from_value(fixture["frame"].clone()).unwrap()
    }

    #[test]
    fn frames_coalesce_and_results_are_consumed_once() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        assert_eq!(mailbox.exchange(Some(frame())), Some((None, true)));
        assert_eq!(mailbox.exchange(Some(frame())), Some((None, false)));
        let input = mailbox.take_input().unwrap();
        assert_eq!(input.sequence, 2);
        let delta = CameraDelta {
            yaw: 0.1,
            pitch: 0.0,
        };
        mailbox.complete(input, Some(delta));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, Some(delta));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn loss_of_input_and_revocation_discard_in_flight_results() {
        let authority = Arc::new(AtomicBool::new(true));
        let mailbox = NativeFrames::new(Arc::clone(&authority));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        mailbox.exchange(None);
        mailbox.complete(
            input,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        authority.store(false, Ordering::Release);
        assert_eq!(mailbox.exchange(Some(frame())), None);
    }

    #[test]
    fn stale_results_do_not_move_a_later_frame() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        for _ in 0..3 {
            mailbox.exchange(Some(frame()));
        }
        mailbox.complete(
            input,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn input_loss_is_recorded_even_when_the_mailbox_is_busy() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        {
            let _locked = mailbox.state.lock().unwrap();
            assert_eq!(mailbox.exchange(None), None);
        }
        mailbox.complete(
            input,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn coalesced_input_loss_resets_the_guest_before_the_next_valid_frame() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let initial = mailbox.take_input().unwrap();
        let mut previous_epoch = Some(initial.epoch);
        mailbox.exchange(None);
        mailbox.exchange(Some(frame()));
        let resumed = mailbox.take_input().unwrap();
        let mut calls = Vec::new();
        resumed
            .dispatch(&mut previous_epoch, |frame| {
                calls.push(frame.is_some());
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(calls, [false, true]);
    }

    #[test]
    fn packet_and_tick_state_work_discards_older_effects_without_resetting_target_scope() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        let epoch = input.epoch;
        mailbox.complete(
            input,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        mailbox.fence_guest_state();
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        mailbox.fence_guest_state();
        assert!(mailbox.take_input().is_none());
        mailbox.exchange(Some(frame()));
        let next = mailbox.take_input().unwrap();
        assert_eq!(next.epoch, epoch);
        let mut previous_epoch = Some(epoch);
        let mut calls = Vec::new();
        next.dispatch(&mut previous_epoch, |frame| {
            calls.push(frame.is_some());
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(calls, [true]);
    }

    #[test]
    fn attack_release_discards_cached_and_in_flight_held_input_effects() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        let mut released = frame();
        released.attack_held = false;
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        mailbox.complete(
            input,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        assert_eq!(mailbox.exchange(Some(released.clone())).unwrap().0, None);
        mailbox.exchange(Some(frame()));
        let in_flight = mailbox.take_input().unwrap();
        mailbox.exchange(Some(released.clone()));
        mailbox.complete(
            in_flight,
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
        );
        assert_eq!(mailbox.exchange(Some(released)).unwrap().0, None);
    }

    #[test]
    fn stop_cannot_be_undone_by_an_in_flight_start() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.deactivate();
        mailbox.activate(true);
        assert_eq!(mailbox.exchange(Some(frame())), None);
    }
}
