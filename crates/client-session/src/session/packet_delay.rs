//! The current native play connection's finite opt-in policy, scoped to the original session.
use super::*;

#[derive(Clone, Default)]
pub(super) struct Control(Arc<Mutex<Option<protocol::NativePacketDelay>>>);

pub(super) struct Guard(Control);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

impl Control {
    /// Installs only the original actual PlaySession's already-spawned capability.
    pub(super) fn install(&self, delay: Option<protocol::NativePacketDelay>) -> Guard {
        self.clear();
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = delay;
        Guard(self.clone())
    }

    /// Withdraws local control and immediately disables its accepted backlog before teardown.
    pub(super) fn clear(&self) {
        let delay = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(delay) = delay {
            let _ = delay.configure(0, false);
        }
    }
}

impl<P> NetworkHandle<P> {
    /// Refreshes this exact spawned native session's finite application-packet delay lease.
    /// No transport task, socket I/O or Core fallback is started by this call.
    pub fn configure_packet_delay(
        &self,
        session: u64,
        delay_ms: u32,
        show: bool,
    ) -> std::io::Result<()> {
        if session == 0 || session != self.session_generation || *self.shutdown.borrow() {
            return Err(unavailable());
        }
        let delay = self
            .packet_delay
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        delay
            .as_ref()
            .ok_or_else(unavailable)?
            .configure(delay_ms, show)
    }

    /// Copies only this original session's last current successful-write network anchor.
    pub fn packet_delay_position(&self, session: u64) -> Option<[f32; 3]> {
        if session == 0 || session != self.session_generation || *self.shutdown.borrow() {
            return None;
        }
        self.packet_delay
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()?
            .position()
    }
}

/// An absent or retired actual native session never delegates control to another transport.
fn unavailable() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "native play session is unavailable",
    )
}
