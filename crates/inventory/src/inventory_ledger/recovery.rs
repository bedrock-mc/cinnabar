//! Bounds how long gestures wait for an authoritative restatement that a server may never send.

use super::PlayerInventoryLedger;

/// How long an unverified surface refuses gestures before its retained values are trusted
/// again. Vanilla's request manager keeps no response deadline and refuses nothing meanwhile.
pub const INVENTORY_RECOVERY_WINDOW_MILLIS: u64 = 1_500;

impl PlayerInventoryLedger {
    /// Whether polling starts, ends or forgets a recovery window.
    pub(super) fn recovery_work_due(&self, now_millis: u64) -> bool {
        match self.recovery_since_millis {
            None => self.resync_required(),
            Some(since) => {
                !self.resync_required()
                    || now_millis >= since.saturating_add(INVENTORY_RECOVERY_WINDOW_MILLIS)
            }
        }
    }

    /// Lifts every gate a missing restatement left once the window passes. Retained values,
    /// timed-out predictions and late answers keep their meaning; only the refusal ends.
    pub(super) fn poll_recovery(&mut self, now_millis: u64) {
        if !self.resync_required() {
            self.recovery_since_millis = None;
            return;
        }
        let since = *self.recovery_since_millis.get_or_insert(now_millis);
        if now_millis < since.saturating_add(INVENTORY_RECOVERY_WINDOW_MILLIS) {
            return;
        }
        tracing::debug!(target: "cinnabar::inventory_requests",
            "no authoritative restatement arrived; trusting retained inventory");
        self.recovery_since_millis = None;
        self.player_resync_required = false;
        self.cursor_resync_required = false;
        self.armor_resync_required = false;
        self.offhand_resync_required = false;
        self.crafting_resync_required = false;
        if let Some(storage) = self.storage.as_mut() {
            storage.resync_required = false;
        }
        self.abandoned.clear();
        for request in &mut self.queue {
            request.refresh_gates = false;
        }
    }
}
