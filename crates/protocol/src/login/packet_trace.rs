use valentine::bedrock::version::v1_26_51::McpePacketName;

pub(super) const MAX_PACKET_ID_TRACE_ENTRIES: usize = 256;
pub(super) const PACKET_ID_TRACE_DURATION: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketIdTraceSnapshot {
    pub packet_ids: Box<[u32]>,
    pub overflow: u64,
    pub timed_out: bool,
}

#[derive(Default)]
pub(super) struct PacketIdTraceState {
    pub(super) started_at: Option<web_time::Instant>,
    pub(super) packet_ids: Vec<u32>,
    pub(super) recorded: usize,
    pub(super) overflow: u64,
    pub(super) timed_out: bool,
}

impl PacketIdTraceState {
    pub(super) fn begin(&mut self) {
        self.started_at = Some(web_time::Instant::now());
        self.packet_ids.clear();
        self.recorded = 0;
        self.overflow = 0;
        self.timed_out = false;
    }

    pub(super) fn observe(&mut self, packet: McpePacketName) {
        let Some(started_at) = self.started_at else {
            return;
        };
        if started_at.elapsed() >= PACKET_ID_TRACE_DURATION {
            self.started_at = None;
            self.timed_out = true;
            return;
        }
        if self.recorded < MAX_PACKET_ID_TRACE_ENTRIES {
            self.packet_ids.push(packet as u32);
            self.recorded += 1;
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    pub(super) fn drain(&mut self) -> Option<PacketIdTraceSnapshot> {
        if self.packet_ids.is_empty() && !self.timed_out {
            return None;
        }
        let overflow = if self.timed_out {
            std::mem::take(&mut self.overflow)
        } else {
            0
        };
        Some(PacketIdTraceSnapshot {
            packet_ids: std::mem::take(&mut self.packet_ids).into_boxed_slice(),
            overflow,
            timed_out: std::mem::take(&mut self.timed_out),
        })
    }
}
