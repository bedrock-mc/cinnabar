//! Bounded block palette evidence shared by this session's decode workers.

use std::{
    fmt::Write as _,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Formats the registry identity embedded in the exact session world carrier.
pub(crate) fn block_registry_sha256(assets: &assets::RuntimeAssets) -> String {
    let bytes = assets.provenance().block_registry_sha256;
    let mut hash = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// Keep enough identities to distinguish an in-range palette mismatch without log flooding.
const MAX_PALETTE_SAMPLES: usize = 16;
const MAX_UNRESOLVED_SAMPLES: usize = 8;

#[derive(Debug, Default)]
struct Samples {
    ids: Mutex<Vec<u32>>,
    retained: AtomicUsize,
}

impl Samples {
    /// Retains each identity once, stopping all work when the sample limit is reached.
    fn retain(&self, id: u32, limit: usize) -> bool {
        if self.retained.load(Ordering::Relaxed) >= limit {
            return false;
        }
        let mut ids = self.ids.lock().unwrap_or_else(|error| error.into_inner());
        if ids.len() >= limit || ids.contains(&id) {
            return false;
        }
        ids.push(id);
        self.retained.store(ids.len(), Ordering::Relaxed);
        true
    }
}

#[derive(Debug, Default)]
pub(crate) struct DecodeDiagnostics {
    palette: Samples,
    unresolved: Samples,
}

impl DecodeDiagnostics {
    /// Logs the first distinct wire identities and unresolved identities for one session.
    pub(crate) fn observe(
        &self,
        wire: u32,
        internal: u32,
        ids: &super::ids::DecodeIds,
        known: bool,
    ) {
        let mode = ids.mode;
        let air = ids.air;
        let session = ids.session_id;
        let assets = &ids.assets;
        if self.palette.retain(wire, MAX_PALETTE_SAMPLES) {
            let resolved = (!assets.is_diagnostic() && assets.is_known(mode, internal))
                .then(|| assets.resolve(mode, internal));
            eprintln!(
                "BLOCK_PALETTE_SAMPLE session={session} mode={mode:?} wire={wire:#010x} internal={internal:#010x} known={known} air={air:#010x} visual={:?} support={:?} face_materials={:?} light_filter={:?} light_emission={:?}",
                resolved.map(|block| block.kind()),
                resolved.map(|block| block.support()),
                resolved
                    .map(|block| assets::BlockFace::ALL.map(|face| block.face(face).material_id())),
                resolved.map(|block| block.light_properties().filter()),
                resolved.map(|block| block.light_properties().emission()),
            );
        }
        if !known && self.unresolved.retain(wire, MAX_UNRESOLVED_SAMPLES) {
            eprintln!(
                "UNRESOLVED_BLOCK_ID session={session} mode={mode:?} wire={wire:#010x} internal={internal:#010x} fallback_air={air:#010x} visual_count={}",
                assets.visual_count(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_deduplicate_and_stop_at_their_limit() {
        let samples = Samples::default();
        assert!(samples.retain(15844, 2));
        assert!(!samples.retain(15844, 2));
        assert!(samples.retain(11838, 2));
        assert!(!samples.retain(42, 2));
        assert_eq!(*samples.ids.lock().unwrap(), [15844, 11838]);
    }

    #[test]
    fn concurrent_workers_share_one_sample_budget() {
        let samples = Samples::default();
        std::thread::scope(|scope| {
            for id in 0..64 {
                let samples = &samples;
                scope.spawn(move || samples.retain(id, MAX_PALETTE_SAMPLES));
            }
        });
        assert_eq!(samples.ids.lock().unwrap().len(), MAX_PALETTE_SAMPLES);
    }
}
