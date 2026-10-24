//! Read-only transport measurements tied to one actual spawned local actor.
use super::*;

#[derive(Clone, Default)]
pub(super) struct Observation(Arc<Mutex<Option<Connection>>>);

struct Connection {
    runtime_id: u64,
    rtt: protocol::RoundTripTime,
}

pub(super) struct Guard(Observation);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

impl Observation {
    /// Retains only a read-only measurement after this session's real Login/bootstrap.
    pub(super) fn install(&self, runtime_id: u64, rtt: Option<protocol::RoundTripTime>) -> Guard {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            rtt.map(|rtt| Connection { runtime_id, rtt });
        Guard(self.clone())
    }

    /// Withdraws the original observation without waiting for transport teardown.
    pub(super) fn clear(&self) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Copies one current positive full RTT; the original publisher owns terminal withdrawal.
    fn round_trip_ms(&self, runtime_id: u64) -> Option<u32> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let connection = connection
            .as_ref()
            .filter(|connection| connection.runtime_id == runtime_id)?;
        let sample = connection.rtt.get();
        let sample = sample.filter(|sample| !sample.is_zero())?;
        let millis = sample
            .as_nanos()
            .div_ceil(Duration::from_millis(1).as_nanos());
        Some(u32::try_from(millis).unwrap_or(u32::MAX))
    }
}

impl<P> NetworkHandle<P> {
    /// Returns this exact session/actor's native full RTT in rounded-up milliseconds.
    /// Unknown, retired and Core observations remain absent; this never performs I/O.
    pub fn round_trip_ms(&self, session: u64, local_runtime_id: u64) -> Option<u32> {
        if session == 0
            || local_runtime_id == 0
            || session != self.session_generation
            || *self.shutdown.borrow()
        {
            return None;
        }
        self.connection_observation.round_trip_ms(local_runtime_id)
    }
}
