//! Resource limits for one packet-mod store, recording peak linear memory for benchmarks.

use super::PACKET_MOD_MEMORY_BYTES;
use anyhow::{Result, bail};
use wasmtime::ResourceLimiter;

/// Table elements one table may hold; language runtimes keep every function in one table.
const MAX_TABLE_ELEMENTS: usize = 1 << 20;
/// Core instances, memories and tables of one component, including the WASI adapter's.
const MAX_INSTANCES: usize = 64;
const MAX_MEMORIES: usize = 8;
const MAX_TABLES: usize = 16;

/// Refuses growth past the trusted limits with a trap and remembers the largest memory request.
#[derive(Default)]
pub(super) struct PacketLimits {
    peak: usize,
}

impl PacketLimits {
    /// The largest linear memory size any memory of this store requested within the limit.
    pub(super) fn peak(&self) -> usize {
        self.peak
    }
}

impl ResourceLimiter for PacketLimits {
    fn memory_growing(&mut self, _: usize, desired: usize, _: Option<usize>) -> Result<bool> {
        if desired > PACKET_MOD_MEMORY_BYTES {
            bail!("packet mod memory exceeds {PACKET_MOD_MEMORY_BYTES} bytes");
        }
        self.peak = self.peak.max(desired);
        Ok(true)
    }

    fn table_growing(&mut self, _: usize, desired: usize, _: Option<usize>) -> Result<bool> {
        if desired > MAX_TABLE_ELEMENTS {
            bail!("packet mod table exceeds {MAX_TABLE_ELEMENTS} elements");
        }
        Ok(true)
    }

    fn instances(&self) -> usize {
        MAX_INSTANCES
    }

    fn memories(&self) -> usize {
        MAX_MEMORIES
    }

    fn tables(&self) -> usize {
        MAX_TABLES
    }
}
