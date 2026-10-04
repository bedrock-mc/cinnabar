//! Read-only lighting evidence with bounded output and column-lifetime provenance.

use super::*;
use client_world::ingestion::{DimensionHeightDiagnostic, SubChunkDiagnostic};

pub(super) mod identity;
mod report;
mod sky_path;
#[cfg(test)]
mod tests;

const MAX_COLUMNS: usize = 3;
const MAX_RETAINED_COLUMNS: usize = 2048;
const MAX_SECTIONS: usize = 64;
const MAX_SAMPLES: usize = 7;

#[derive(Default)]
pub(super) struct LightingDiagnostics {
    identities: identity::BlockIdentities,
    start_dimension: i32,
    column_evidence_truncated: bool,
    modes: [Option<LevelChunkMode>; 3],
    pub(super) columns: HashMap<ChunkKey, ColumnArrival>,
    pub(super) heights: Vec<DimensionHeightDiagnostic>,
}

#[derive(Default)]
pub(super) struct ColumnArrival {
    mode: Option<LevelChunkMode>,
    sections: BTreeMap<i32, SectionArrival>,
    truncated: bool,
}

#[derive(Default)]
struct SectionArrival {
    source: Option<&'static str>,
    reply: Option<String>,
    metadata: Option<SubChunkDiagnostic>,
}

impl LightingDiagnostics {
    /// Retains the original StartGame dimension even after a dimension transition.
    pub(super) fn new(dimension: i32) -> Self {
        Self {
            start_dimension: dimension,
            ..Self::default()
        }
    }

    /// Forgets only column evidence when terrain is evicted; session facts survive.
    pub(super) fn remove_column(&mut self, key: ChunkKey) {
        self.columns.remove(&key);
    }

    /// Records each mode family once and the most recent declaration for this column.
    fn mode(&mut self, key: ChunkKey, mode: LevelChunkMode) {
        let index = match mode {
            LevelChunkMode::Inline { .. } => 0,
            LevelChunkMode::LimitedRequests { .. } => 1,
            LevelChunkMode::LimitlessRequests => 2,
        };
        self.modes[index].get_or_insert(mode);
        if let Some(column) = self.column(key) {
            column.mode = Some(mode);
        }
    }

    /// Caps retained metadata independently of unusual server traffic or pending requests.
    fn column(&mut self, key: ChunkKey) -> Option<&mut ColumnArrival> {
        if self.columns.len() >= MAX_RETAINED_COLUMNS && !self.columns.contains_key(&key) {
            self.column_evidence_truncated = true;
            return None;
        }
        Some(self.columns.entry(key).or_default())
    }

    /// Bounds evidence for unusual section coordinates without changing terrain admission.
    fn section(&mut self, key: SubChunkKey) -> Option<&mut SectionArrival> {
        let column = self.column(key.chunk())?;
        if column.sections.len() >= MAX_SECTIONS && !column.sections.contains_key(&key.y) {
            column.truncated = true;
            return None;
        }
        Some(column.sections.entry(key.y).or_default())
    }
}

impl WorldStream {
    /// Records the sections established by a successfully committed inline column.
    pub(super) fn diagnose_inline_column(
        &mut self,
        event: &LevelChunkEvent,
        stored: &BTreeSet<SubChunkKey>,
    ) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        self.light_diagnostics.columns.remove(&key);
        self.light_diagnostics.mode(key, event.mode);
        let Some(range) = vanilla_dimension_range(event.dimension) else {
            return;
        };
        let LevelChunkMode::Inline { count } = event.mode else {
            return;
        };
        let sections = (0..range.sub_chunk_count)
            .map(|offset| SubChunkKey::from_chunk(key, range.base_sub_chunk_y + offset as i32))
            .chain(stored.iter().copied())
            .collect::<BTreeSet<_>>();
        for section in sections {
            if let Some(record) = self.light_diagnostics.section(section) {
                let offset = i64::from(section.y) - i64::from(range.base_sub_chunk_y);
                record.source = Some(if stored.contains(&section) {
                    "inline-chunk"
                } else if offset >= 0 && (offset as usize) < count {
                    "inline-empty-slot"
                } else {
                    "inline-omitted-air"
                });
            }
        }
    }

    /// Records a request header before its independently committed upper-air sections.
    pub(super) fn diagnose_request_column(&mut self, event: &LevelChunkEvent) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        self.light_diagnostics.mode(key, event.mode);
    }

    /// Records upper air only after the request header successfully established that section.
    pub(super) fn diagnose_request_air_commit(&mut self, key: SubChunkKey) {
        if let Some(record) = self.light_diagnostics.section(key) {
            record.source = Some("limited-request-upper-air");
            record.reply = None;
            record.metadata = None;
        }
    }

    /// Retains the last admitted response, including errors, independently of resident data.
    pub(super) fn diagnose_sub_chunk_reply(
        &mut self,
        key: SubChunkKey,
        result: &PreparedSubChunkResult,
        metadata: Option<SubChunkDiagnostic>,
    ) -> Option<&'static str> {
        let record = self.light_diagnostics.section(key)?;
        let (reply, source) = match result {
            PreparedSubChunkResult::Decoded(decoded) => (
                "Success".to_owned(),
                if decoded.sub_chunk().has_no_storages() {
                    "request-success-empty"
                } else {
                    "request-success"
                },
            ),
            PreparedSubChunkResult::AllAir => ("SuccessAllAir".to_owned(), "request-all-air"),
            PreparedSubChunkResult::Unavailable(reason) => {
                (format!("{reason:?}"), "request-out-of-bounds-air")
            }
        };
        record.reply = Some(reply);
        record.metadata = metadata;
        Some(source)
    }

    /// Updates resident provenance only when a response actually supplied its current contents.
    pub(super) fn diagnose_sub_chunk_commit(
        &mut self,
        key: SubChunkKey,
        source: Option<&'static str>,
    ) {
        let Some(source) = source else { return };
        if let Some(record) = self.light_diagnostics.section(key) {
            record.source = Some(source);
        }
    }

    /// Gives the app immutable StartGame facts without claiming inferred bounds came from wire.
    pub fn lighting_session_facts(&self) -> String {
        format!(
            "start_game_dimension={} start_game_height_range=not-supplied effective_start_range={:?} range_source=vanilla-dimension-table network_ids={:?}",
            self.light_diagnostics.start_dimension,
            vanilla_dimension_range(self.light_diagnostics.start_dimension),
            self.network_id_mode()
        )
    }

    /// Returns the first observed header of each mode family for once-per-session logging.
    pub fn lighting_request_modes(&self) -> [Option<String>; 3] {
        self.light_diagnostics
            .modes
            .map(|mode| mode.map(|mode| format!("{mode:?}")))
    }
}
