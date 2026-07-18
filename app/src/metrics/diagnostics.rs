use std::{collections::BTreeMap, fmt::Write as _, sync::OnceLock};

use meshing::DiagnosticGeometrySummary;
use serde::Serialize;
use world::SubChunkKey;

pub const DIAGNOSTIC_TOP_LIMIT: usize = 8;
const MAX_TRACKED_DIAGNOSTIC_IDENTITIES: usize = 16_913 + 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticAttributionEntry {
    pub sequential_id: Option<u32>,
    pub network_id: u32,
    pub name: String,
    pub quad_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DiagnosticAttributionSnapshot {
    pub total_quad_count: u64,
    pub top: Vec<DiagnosticAttributionEntry>,
    pub omitted_identity_count: u64,
    pub omitted_quad_count: u64,
}

impl DiagnosticAttributionSnapshot {
    #[must_use]
    pub fn marker_fields(&self) -> String {
        let mut top = String::new();
        for (index, entry) in self.top.iter().enumerate() {
            if index != 0 {
                top.push(',');
            }
            let sequential = entry
                .sequential_id
                .map_or_else(|| "unknown".to_owned(), |id| id.to_string());
            let _ = write!(
                top,
                "{sequential}|0x{:08x}|{}|{}",
                entry.network_id, entry.name, entry.quad_count
            );
        }
        if top.is_empty() {
            top.push_str("none");
        }
        format!(
            "diagnostic_attribution_total={} diagnostic_attribution_top={} diagnostic_attribution_omitted_identities={} diagnostic_attribution_omitted_quads={}",
            self.total_quad_count, top, self.omitted_identity_count, self.omitted_quad_count
        )
    }
}

#[derive(Debug)]
struct DiagnosticCatalogEntry {
    name: Box<str>,
}

fn protocol_1001_catalog() -> &'static [DiagnosticCatalogEntry] {
    static CATALOG: OnceLock<Box<[DiagnosticCatalogEntry]>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let records = assets::read_registry(include_bytes!(
            "../../crates/assets/data/block-registry-v1001.bin"
        ))
        .expect("checked-in protocol-1001 registry must remain valid");
        records
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(index, record)| {
                assert_eq!(record.sequential_id as usize, index);
                DiagnosticCatalogEntry { name: record.name }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    })
}

#[derive(Debug)]
struct ResidentDiagnosticContribution {
    summary: DiagnosticGeometrySummary,
    tracked_entries: Box<[bool]>,
}

#[derive(Debug)]
pub struct DiagnosticQuadTracker {
    by_sub_chunk: BTreeMap<SubChunkKey, ResidentDiagnosticContribution>,
    totals: BTreeMap<(Option<u32>, u32), u64>,
    total: u64,
    explicit_omitted_identity_count: u64,
    explicit_omitted_quad_count: u64,
    revision: u64,
    identity_capacity: usize,
}

impl Default for DiagnosticQuadTracker {
    fn default() -> Self {
        Self {
            by_sub_chunk: BTreeMap::new(),
            totals: BTreeMap::new(),
            total: 0,
            explicit_omitted_identity_count: 0,
            explicit_omitted_quad_count: 0,
            revision: 0,
            identity_capacity: MAX_TRACKED_DIAGNOSTIC_IDENTITIES,
        }
    }
}

impl DiagnosticQuadTracker {
    #[cfg(test)]
    pub(super) fn with_identity_capacity(identity_capacity: usize) -> Self {
        Self {
            identity_capacity,
            ..Self::default()
        }
    }

    pub fn upsert(&mut self, key: SubChunkKey, summary: DiagnosticGeometrySummary) {
        if self
            .by_sub_chunk
            .get(&key)
            .is_some_and(|resident| resident.summary == summary)
        {
            return;
        }
        if summary.total_quad_count() == 0 {
            self.remove(key);
            return;
        }
        if let Some(previous) = self.by_sub_chunk.remove(&key) {
            self.subtract_contribution(&previous);
        }
        let contribution = self.add_summary(summary);
        self.by_sub_chunk.insert(key, contribution);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn remove(&mut self, key: SubChunkKey) {
        if let Some(previous) = self.by_sub_chunk.remove(&key) {
            self.subtract_contribution(&previous);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn snapshot(&self) -> DiagnosticAttributionSnapshot {
        let catalog = protocol_1001_catalog();
        let mut counts = self
            .totals
            .iter()
            .map(|(&(sequential_id, network_id), &quad_count)| {
                (sequential_id, network_id, quad_count)
            })
            .collect::<Vec<_>>();
        counts.sort_unstable_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| left.0.unwrap_or(u32::MAX).cmp(&right.0.unwrap_or(u32::MAX)))
                .then_with(|| left.1.cmp(&right.1))
        });
        let omitted = counts.split_off(counts.len().min(DIAGNOSTIC_TOP_LIMIT));
        let top = counts
            .into_iter()
            .map(|(sequential_id, network_id, quad_count)| {
                let name = sequential_id
                    .and_then(|id| catalog.get(id as usize))
                    .map_or_else(|| "unknown".to_owned(), |entry| entry.name.to_string());
                DiagnosticAttributionEntry {
                    sequential_id,
                    network_id,
                    name,
                    quad_count,
                }
            })
            .collect();
        DiagnosticAttributionSnapshot {
            total_quad_count: self.total,
            top,
            omitted_identity_count: self
                .explicit_omitted_identity_count
                .saturating_add(omitted.len() as u64),
            omitted_quad_count: self.explicit_omitted_quad_count.saturating_add(
                omitted
                    .into_iter()
                    .map(|(_, _, quad_count)| quad_count)
                    .sum::<u64>(),
            ),
        }
    }

    fn add_summary(
        &mut self,
        summary: DiagnosticGeometrySummary,
    ) -> ResidentDiagnosticContribution {
        self.total = self.total.saturating_add(summary.total_quad_count());
        self.explicit_omitted_identity_count = self
            .explicit_omitted_identity_count
            .saturating_add(u64::from(summary.omitted_identity_count()));
        self.explicit_omitted_quad_count = self
            .explicit_omitted_quad_count
            .saturating_add(summary.omitted_quad_count());
        let mut tracked_entries = Vec::with_capacity(summary.entries().len());
        for count in summary.entries() {
            let key = (count.sequential_id(), count.network_id());
            if let Some(total) = self.totals.get_mut(&key) {
                *total = total.saturating_add(u64::from(count.quad_count()));
                tracked_entries.push(true);
            } else if self.totals.len() < self.identity_capacity {
                self.totals.insert(key, u64::from(count.quad_count()));
                tracked_entries.push(true);
            } else {
                self.explicit_omitted_identity_count =
                    self.explicit_omitted_identity_count.saturating_add(1);
                self.explicit_omitted_quad_count = self
                    .explicit_omitted_quad_count
                    .saturating_add(u64::from(count.quad_count()));
                tracked_entries.push(false);
            }
        }
        ResidentDiagnosticContribution {
            summary,
            tracked_entries: tracked_entries.into_boxed_slice(),
        }
    }

    fn subtract_contribution(&mut self, contribution: &ResidentDiagnosticContribution) {
        let summary = &contribution.summary;
        self.total = self.total.saturating_sub(summary.total_quad_count());
        self.explicit_omitted_identity_count = self
            .explicit_omitted_identity_count
            .saturating_sub(u64::from(summary.omitted_identity_count()));
        self.explicit_omitted_quad_count = self
            .explicit_omitted_quad_count
            .saturating_sub(summary.omitted_quad_count());
        debug_assert_eq!(summary.entries().len(), contribution.tracked_entries.len());
        for (count, tracked) in summary
            .entries()
            .iter()
            .zip(contribution.tracked_entries.iter().copied())
        {
            let key = (count.sequential_id(), count.network_id());
            if tracked {
                let remove = self.totals.get_mut(&key).is_some_and(|total| {
                    *total = total.saturating_sub(u64::from(count.quad_count()));
                    *total == 0
                });
                if remove {
                    self.totals.remove(&key);
                }
            } else {
                self.explicit_omitted_identity_count =
                    self.explicit_omitted_identity_count.saturating_sub(1);
                self.explicit_omitted_quad_count = self
                    .explicit_omitted_quad_count
                    .saturating_sub(u64::from(count.quad_count()));
            }
        }
    }
}
