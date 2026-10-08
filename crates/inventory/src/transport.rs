use crate::InventorySession;
use protocol::Packet;

/// Bounds one frame's inventory transport work.
const MAX_INVENTORY_PACKETS_PER_FLUSH: usize = 32;

impl InventorySession {
    /// Admits every ready inventory packet in queue order, stopping at the first
    /// transport refusal. Returns whether anything was admitted.
    pub fn flush_inventory_send<E>(
        &mut self,
        now_millis: u64,
        send: impl FnMut(Packet) -> Result<(), E>,
    ) -> Result<bool, E> {
        self.poll_inventory_timeout(now_millis);
        self.flush_pending_inventory_send(now_millis, send)
    }

    /// Sends ready commands after the caller has polled timeouts and projected any screen change.
    pub fn flush_pending_inventory_send<E>(
        &mut self,
        now_millis: u64,
        mut send: impl FnMut(Packet) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut admitted_any = false;
        for _ in 0..MAX_INVENTORY_PACKETS_PER_FLUSH {
            let Some((packet, entries)) = self
                .ledger()
                .pending_batch()
                .expect("the ledger retains only validated protocol requests")
            else {
                break;
            };
            if let Err(error) = send(packet) {
                self.ledger_mut().note_transport_pressure(now_millis);
                return Err(error);
            }
            for _ in 0..entries {
                let admitted = self.ledger_mut().mark_transport_enqueued(now_millis);
                debug_assert!(admitted, "only an awaiting request can be transported");
            }
            admitted_any = true;
        }
        Ok(admitted_any)
    }
}
