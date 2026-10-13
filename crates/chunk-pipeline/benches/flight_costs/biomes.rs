//! Exact biome payload and reuse counters for the flight preflight window.

use std::collections::{BTreeMap, HashSet};

use chunk_pipeline::benchmark_support::biomes::{CpuCounters, cpu_counters};
use meshing::{ChunkMesh, PackedBiomeRecord, chunk_publication_byte_len};
use protocol::{BiomeDefinitionEvent, BiomeDefinitionsEvent, WorldEvent};
use world::SubChunkKey;

/// Alternating broad biome regions give the flight both uniform records and seams.
pub(super) fn column_biome(x: i32, z: i32) -> i32 {
    if (x.div_euclid(4) + z.div_euclid(4)).rem_euclid(2) == 0 {
        7
    } else {
        9
    }
}

/// Registers two synthetic biome IDs with distinct climates and water tints.
pub(super) fn definitions() -> WorldEvent {
    WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
        definitions: [7, 9]
            .map(|id| BiomeDefinitionEvent {
                biome_id: Some(id),
                name: format!("flight_biome_{id}").into(),
                temperature: if id == 7 { 0.8 } else { 0.4 },
                downfall: 0.4,
                snow_foliage: 0.0,
                max_snow_accumulation: None,
                map_water_color: if id == 7 { 0xff22_55aa } else { 0xff44_aa88 },
            })
            .into(),
    })
}

#[derive(Default)]
pub(super) struct PayloadProbe {
    active: bool,
    tracking: bool,
    previous: BTreeMap<SubChunkKey, PackedBiomeRecord>,
    distinct: HashSet<Vec<u32>>,
    publications: u64,
    remeshes: u64,
    unchanged: u64,
    record_bytes: u64,
    uploaded_bytes: u64,
    total_uploaded_bytes: u64,
}

impl PayloadProbe {
    /// Retains warm-up records so the first flight remesh can be compared exactly.
    pub(super) fn warming_up() -> Self {
        Self {
            tracking: true,
            ..Self::default()
        }
    }

    /// Begins the flight window after retaining the warmed-up section records.
    pub(super) fn start(&mut self) {
        self.active = true;
        self.distinct.clear();
    }

    /// Mirrors render-side unchanged-range reuse without performing GPU work.
    pub(super) fn observe(
        &mut self,
        key: SubChunkKey,
        mesh: &ChunkMesh,
        record: &PackedBiomeRecord,
    ) {
        if !self.tracking {
            return;
        }
        if mesh.is_empty() {
            self.previous.remove(&key);
            return;
        }
        let previous = self.previous.insert(key, record.clone());
        if !self.active {
            return;
        }
        self.publications += 1;
        let bytes = if record.is_fallback() {
            0
        } else {
            record.byte_len()
        };
        self.record_bytes += bytes;
        let unchanged = previous.as_ref() == Some(record);
        self.remeshes += u64::from(previous.is_some());
        self.unchanged += u64::from(unchanged);
        self.uploaded_bytes += if unchanged { 0 } else { bytes };
        self.total_uploaded_bytes +=
            chunk_publication_byte_len(mesh, record) - if unchanged { bytes } else { 0 };
        self.distinct.insert(record.words().to_vec());
    }

    /// Drops retired section records from the resident-memory count.
    pub(super) fn remove(&mut self, key: SubChunkKey) {
        if self.tracking {
            self.previous.remove(&key);
        }
    }

    /// Prints payload counters and worker CPU shares for the two-second window.
    pub(super) fn report(&mut self, radius: i32, speed: f32, before: CpuCounters) {
        let after = cpu_counters();
        let biome_ns = after.biome_ns - before.biome_ns;
        let mesh_ns = after.mesh_ns - before.mesh_ns;
        let resident_bytes: u64 = self
            .previous
            .values()
            .filter(|record| !record.is_fallback())
            .map(PackedBiomeRecord::byte_len)
            .sum();
        eprintln!(
            "BIOME_FLIGHT radius={radius} speed={speed} window_seconds=2 publications={} distinct_records={} remeshes={} unchanged_remeshes={} published_record_bytes={} biome_uploaded_bytes={} total_uploaded_bytes={} resident_sections={} resident_record_bytes={} record_requests={} cache_hits={} biome_cpu_us={} mesh_cpu_us={} biome_cpu_percent={:.2}",
            self.publications,
            self.distinct.len(),
            self.remeshes,
            self.unchanged,
            self.record_bytes,
            self.uploaded_bytes,
            self.total_uploaded_bytes,
            self.previous.len(),
            resident_bytes,
            after.requests - before.requests,
            after.cache_hits - before.cache_hits,
            biome_ns / 1000,
            mesh_ns / 1000,
            100.0 * biome_ns as f64 / mesh_ns.max(1) as f64
        );
        self.active = false;
        self.tracking = false;
        self.previous.clear();
        self.distinct.clear();
    }
}
