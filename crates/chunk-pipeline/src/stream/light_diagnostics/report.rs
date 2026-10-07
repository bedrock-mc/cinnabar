use std::fmt::Write;

use super::*;

impl WorldStream {
    /// Formats at most seven block samples and three columns; it never runs or changes a solve.
    pub fn lighting_diagnostic(&self, positions: &[[f32; 3]]) -> String {
        let mut out = String::new();
        let dimension = self.current_dimension();
        let range = self.authority.dimension_range(dimension);
        let block_range = range.map(|r| {
            (
                i64::from(r.base_sub_chunk_y) * 16,
                (i64::from(r.base_sub_chunk_y) + r.sub_chunk_count as i64) * 16,
            )
        });
        let advertised = self
            .light_diagnostics
            .heights
            .iter()
            .find(|r| r.dimension == dimension);
        let _ = write!(
            out,
            "effective_height_range={block_range:?}(max-exclusive) advertised_height={advertised:?} server_light=not-in-chunk-protocol heightmap_used_by_solver=false "
        );
        let ids = self.decode_ids(dimension);
        let mut columns = BTreeMap::new();
        for (index, position) in positions.iter().take(MAX_SAMPLES).enumerate() {
            if !position.iter().all(|v| v.is_finite()) {
                let _ = write!(out, "sample[{index}]=non-finite; ");
                continue;
            }
            let position = position.map(floor_to_i32);
            let (key, local) = split_light_position(
                dimension,
                BlockPos::new(position[0], position[1], position[2]),
            );
            let _ = write!(out, "sample[{index}] pos={position:?} section_y={} ", key.y);
            self.describe_block(&mut out, key, local, &ids);
            let stored = self.solved_light_at(position.map(|v| v as f32));
            let current = self.light_is_current(key);
            let direct = self.lighting.direct_sky.get(&key);
            let _ = write!(
                out,
                " stored_block_sky={stored:?} solved_block_sky={:?} solve_current={current} light_generation={:?} block_generation={:?} direct_generation={:?} direct_sky={:?} pending={} in_flight={}; ",
                stored.filter(|_| current),
                self.lighting.store.light(key).map(|l| l.generation()),
                self.lighting.block_generations.get(&key),
                direct.map(|d| d.light_revision),
                direct.map(|d| d.mask.get(local[0], local[1], local[2])),
                self.lighting.jobs.pending.contains_key(&key),
                self.lighting.jobs.in_flight.contains_key(&key)
            );
            if columns.len() < MAX_COLUMNS {
                columns.entry(key.chunk()).or_insert(position);
            }
        }
        let _ = write!(out, "samples_truncated={} ", positions.len() > MAX_SAMPLES);
        for (column, position) in &columns {
            self.describe_column(&mut out, *column);
            self.describe_sky_path(&mut out, *position, &ids);
        }
        let _ = write!(
            out,
            "column_limit={MAX_COLUMNS} column_evidence_truncated={}",
            self.light_diagnostics.column_evidence_truncated
        );
        out
    }

    /// Reports all storage layers because waterlogging and overlays also filter light.
    pub(super) fn describe_block(
        &self,
        out: &mut String,
        key: SubChunkKey,
        local: [u8; 3],
        ids: &DecodeIds,
    ) {
        let sub_chunk = self.authority.terrain().sub_chunk(key);
        let air = self.known_air.contains(&key)
            || sub_chunk.as_ref().is_some_and(|s| s.has_no_storages());
        if air {
            let identity = self.light_diagnostics.identities.describe(ids, ids.air);
            let _ = write!(
                out,
                "presence=known-air layers=[{identity}] effective_light_filter=0 light_dampening=0 emission=0"
            );
        } else if let Some(sub_chunk) = sub_chunk {
            let mut filter = 0;
            let mut emission = 0;
            let _ = write!(out, "presence=resident layers=[");
            for layer in 0..sub_chunk.storages().len() {
                let Some(id) = sub_chunk.runtime_id(layer, local[0], local[1], local[2]) else {
                    continue;
                };
                let properties = ids.assets.resolve(ids.mode, id).light_properties();
                if id != ids.air {
                    filter = filter.max(properties.filter());
                    emission = emission.max(properties.emission());
                }
                if layer < 4 {
                    let identity = self.light_diagnostics.identities.describe(ids, id);
                    let _ = write!(
                        out,
                        "layer{layer}({identity} light_filter={} light_dampening={} emission={}),",
                        properties.filter(),
                        properties.filter(),
                        properties.emission()
                    );
                }
            }
            let _ = write!(
                out,
                "] layers_truncated={} effective_light_filter={filter} light_dampening={filter} emission={emission}",
                sub_chunk.storages().len() > 4
            );
        } else {
            let _ = write!(
                out,
                "presence=absent identity=unknown effective_light_filter=unknown light_dampening=unknown"
            );
        }
    }

    /// Summarizes the actual sky ceiling, section availability and last admitted wire replies.
    fn describe_column(&self, out: &mut String, column: ChunkKey) {
        let key = SubChunkKey::from_chunk(column, 0);
        let arrival = self.light_diagnostics.columns.get(&column);
        let top = self.light_column_top_sub_chunk_y(key);
        let top_key = top.map(|y| SubChunkKey::from_chunk(column, y));
        let sky_seed_y = (column.dimension == 0)
            .then_some(top)
            .flatten()
            .and_then(|y| y.checked_mul(16)?.checked_add(15));
        let mut sections = BTreeSet::new();
        if let Some(range) = self.authority.dimension_range(column.dimension) {
            sections.extend((0..range.sub_chunk_count).map(|o| range.base_sub_chunk_y + o as i32));
        }
        sections.extend(self.light_column_sources(key).map(|k| k.y));
        if let Some(arrival) = arrival {
            sections.extend(arrival.sections.keys().copied());
        }
        let _ = write!(
            out,
            "column=({},{}) mode={:?} sky_seed_y={sky_seed_y:?} sky_seed_top_known={} sky_seed_top_current={} sections=[",
            column.x,
            column.z,
            arrival.and_then(|a| a.mode),
            top_key.is_some_and(|k| self.light_source_is_known(k)),
            top_key.is_some_and(|k| self.light_is_current(k))
        );
        for y in sections.iter().take(MAX_SECTIONS) {
            let key = SubChunkKey::from_chunk(column, *y);
            let source = arrival.and_then(|a| a.sections.get(y));
            let presence = if self.known_air.contains(&key) {
                "known-air"
            } else if self.authority.terrain().sub_chunk(key).is_some() {
                "resident"
            } else {
                "absent"
            };
            let _ = write!(
                out,
                "{y}:{presence}/source={}/reply={:?}/current={}/expected={}",
                source.and_then(|s| s.source).unwrap_or("unrecorded"),
                source.and_then(|s| s.reply.as_deref()),
                self.light_is_current(key),
                self.requests.is_expected(key)
            );
            if let Some(metadata) = source.and_then(|s| s.metadata) {
                let _ = write!(out, "/decoded_payload_present={}", metadata.payload_present);
                describe_heightmap(out, "heightmap", metadata.heightmap);
                describe_heightmap(out, "render_heightmap", metadata.render_heightmap);
            } else {
                out.push_str("/heightmap=not-received");
            }
            out.push(';');
        }
        let _ = write!(
            out,
            "] sections_truncated={}; ",
            sections.len() > MAX_SECTIONS || arrival.is_some_and(|a| a.truncated)
        );
    }
}

/// Formats raw heightmap summaries without expanding any of their 256 samples.
fn describe_heightmap(
    out: &mut String,
    name: &str,
    map: client_world::ingestion::HeightmapDiagnostic,
) {
    let kind = match map.kind {
        0 => "no-data",
        1 => "data",
        2 => "all-too-high",
        3 => "all-too-low",
        4 if name == "render_heightmap" => "copied",
        _ => "unknown",
    };
    let _ = write!(
        out,
        "/{name}={kind}({})/present={}/samples={}/raw_min_max={:?}",
        map.kind,
        map.payload_present,
        map.sample_count,
        map.min.zip(map.max)
    );
}
