//! Bed screen state: sleeping shows the bed screen and a wake request leaves the bed.

use super::{UiApplyOutcome, UiRuntime};

/// The world's sleep status as the server last sent it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SleepStatus {
    pub sleeping: u32,
    /// Sleepers needed to skip the night (the `overworldPlayerCount` field).
    pub required: u32,
    pub able: bool,
}

impl UiRuntime {
    /// Tracks the local player's sleep; waking closes a chat opened from the bed.
    pub fn set_local_sleeping(&mut self, sleeping: bool) {
        if sleeping == self.local_sleeping {
            return;
        }
        self.local_sleeping = sleeping;
        if !sleeping {
            self.wake_requested = false;
            if self.chat_focused {
                self.close_chat();
            }
        }
    }

    /// Whether the bed screen owns input: the player lies in bed.
    pub const fn local_sleeping(&self) -> bool {
        self.local_sleeping
    }

    /// Decodes the `SleepingPlayers` compound; an undecodable one is skipped.
    pub fn apply_sleep_status(&mut self, event: &protocol::SleepStatusEvent) -> UiApplyOutcome {
        let Some(root) = world::BlockEntityNbt::decode_prefix(&event.nbt)
            .ok()
            .and_then(|(nbt, _)| nbt.parse())
        else {
            return UiApplyOutcome::IgnoredByReceiveStore;
        };
        // Absent or non-int fields read as zero, as the client does.
        let count = |key: &str| {
            root.integer(key)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0)
        };
        self.sleep_status = Some(SleepStatus {
            sleeping: count("sleepingPlayerCount"),
            required: count("overworldPlayerCount"),
            able: count("ableToSleep") != 0,
        });
        UiApplyOutcome::Applied
    }

    pub const fn sleep_status(&self) -> Option<SleepStatus> {
        self.sleep_status
    }

    /// Queues one StopSleeping action; a no-op unless the local player is asleep.
    pub fn request_wake(&mut self) {
        self.wake_requested |= self.local_sleeping;
    }

    /// Sends a wake action when the local actor is known and the transport accepts it.
    pub fn flush_wake_request<E>(
        &mut self,
        runtime_id: Option<u64>,
        send: impl FnOnce(protocol::Packet) -> Result<(), E>,
    ) -> bool {
        if self.wake_requested
            && let Some(runtime_id) = runtime_id
            && send(protocol::stop_sleeping_packet(runtime_id)).is_ok()
        {
            self.wake_requested = false;
            true
        } else {
            false
        }
    }

    /// Consumes the retained request in focused state tests.
    #[cfg(test)]
    fn take_wake_request(&mut self) -> bool {
        std::mem::take(&mut self.wake_requested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_takes_ui_focus_and_wake_request_is_taken_once() {
        let mut player_runtime = player_state::PlayerState::new(1);

        let mut runtime = UiRuntime::new(1);
        runtime.request_wake();
        assert!(!runtime.take_wake_request());

        runtime.set_local_sleeping(true);
        assert!(runtime.ui_focused(&player_runtime) && !runtime.chat_focused());
        runtime.open_chat(&mut player_runtime);
        runtime.request_wake();
        assert!(runtime.take_wake_request());
        assert!(!runtime.take_wake_request());

        runtime.request_wake();
        runtime.set_local_sleeping(false);
        assert!(!runtime.chat_focused());
        assert!(!runtime.take_wake_request());
    }

    #[test]
    fn sleeping_players_compound_decodes_leniently() {
        // A root compound of zigzag-varint ints, as servers send it.
        let mut nbt = vec![10, 0];
        for (name, value) in [
            ("sleepingPlayerCount", 2u8),
            ("overworldPlayerCount", 6),
            ("ableToSleep", 2),
        ] {
            nbt.push(3);
            nbt.push(name.len() as u8);
            nbt.extend_from_slice(name.as_bytes());
            nbt.push(value);
        }
        nbt.push(0);
        let mut runtime = UiRuntime::new(1);
        let event = protocol::SleepStatusEvent { nbt: nbt.into() };
        assert_eq!(runtime.apply_sleep_status(&event), UiApplyOutcome::Applied);
        assert_eq!(
            runtime.sleep_status(),
            Some(SleepStatus {
                sleeping: 1,
                required: 3,
                able: true
            })
        );
        let garbage = protocol::SleepStatusEvent {
            nbt: vec![1, 2].into(),
        };
        assert_eq!(
            runtime.apply_sleep_status(&garbage),
            UiApplyOutcome::IgnoredByReceiveStore
        );
        assert!(
            runtime.sleep_status().is_some(),
            "a bad packet keeps the last status"
        );
    }
    #[test]
    fn wake_survives_transport_pressure_and_missing_actor_identity() {
        for missing_actor in [false, true] {
            let mut runtime = UiRuntime::new(1);
            runtime.set_local_sleeping(true);
            runtime.request_wake();
            assert!(!runtime.flush_wake_request((!missing_actor).then_some(7), |_| Err(())));
            let mut sent = 0;
            assert!(runtime.flush_wake_request(Some(7), |_| {
                sent += 1;
                Ok::<(), ()>(())
            }));
            assert_eq!(sent, 1);
            assert!(!runtime.flush_wake_request(Some(7), |_| Ok::<(), ()>(())));
        }
    }
}
