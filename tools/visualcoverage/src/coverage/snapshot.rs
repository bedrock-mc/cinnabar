use super::*;

pub fn baseline_from_snapshot(
    snapshot: &CoverageSnapshot,
    mut invisible_allowlist: Vec<AllowlistEntry>,
) -> Result<Baseline, CoverageError> {
    validate_protocol_snapshot(snapshot)?;
    invisible_allowlist.sort_by(|left, right| left.state.cmp(&right.state));
    let allowlisted = invisible_allowlist
        .iter()
        .map(|entry| entry.state.clone())
        .collect::<BTreeSet<_>>();
    let air = snapshot.air_states.iter().cloned().collect::<BTreeSet<_>>();
    let invisible = snapshot
        .invisible_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for entry in &invisible_allowlist {
        if entry.authority.trim().is_empty()
            || !entry.source.starts_with("https://")
            || entry.source.len() <= "https://".len()
        {
            return Err(CoverageError::InvalidInvisibleCitation {
                state: entry.state.clone(),
            });
        }
        if !invisible.contains(&entry.state) {
            return Err(CoverageError::StaleInvisibleAllowlist {
                state: entry.state.clone(),
            });
        }
    }
    for state in &snapshot.invisible_states {
        if !air.contains(state) && !allowlisted.contains(state) {
            return Err(CoverageError::MissingInvisibleCitation {
                state: state.clone(),
            });
        }
    }
    let baseline = Baseline {
        schema: BASELINE_SCHEMA.to_owned(),
        protocol: snapshot.protocol,
        registry_sha256: snapshot.registry_sha256.clone(),
        counts: snapshot.counts,
        states: snapshot.states.clone(),
        diagnostic_sequential_ids: snapshot
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist,
        expected_vine_diagnostic_masks: snapshot.vine_diagnostic_masks.clone(),
    };
    validate_baseline(&baseline)?;
    Ok(baseline)
}

pub fn analyze_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
) -> Result<CoverageSnapshot, CoverageError> {
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )
}

pub fn analyze_records(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
    registry_sha256: &str,
    assets_sha256: &str,
) -> Result<CoverageSnapshot, CoverageError> {
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|record| record.sequential_id);

    let mut previous = None;
    let mut canonical = BTreeSet::new();
    for (expected, record) in ordered.iter().enumerate() {
        if previous == Some(record.sequential_id) {
            return Err(CoverageError::DuplicateSequentialId(record.sequential_id));
        }
        previous = Some(record.sequential_id);
        let expected = u32::try_from(expected).expect("registry is bounded below u32::MAX");
        if record.sequential_id != expected {
            return Err(CoverageError::NonContiguousSequentialId {
                expected,
                actual: record.sequential_id,
            });
        }
        let key = (record.name.to_string(), record.canonical_state.to_string());
        if !canonical.insert(key.clone()) {
            return Err(CoverageError::DuplicateCanonicalState {
                name: key.0,
                canonical_state: key.1,
            });
        }
    }

    if runtime.visual_count() != records.len() || runtime.hashed_count() != records.len() {
        return Err(CoverageError::RuntimeCardinalityMismatch {
            registry: records.len(),
            visuals: runtime.visual_count(),
            hashes: runtime.hashed_count(),
        });
    }

    let mut diagnostics = Vec::new();
    let mut invisible = Vec::new();
    let mut air_states = Vec::new();
    let mut diagnostics_by_family = BTreeMap::new();
    let mut diagnostics_by_name = BTreeMap::new();
    let mut vine_masks = Vec::new();
    for record in &ordered {
        if runtime.sequential_id_for_hash(record.network_hash) != Some(record.sequential_id) {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        if !sequential.is_known() || !hashed.is_known() || sequential != hashed {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }
        let identity = StateIdentity::from_record(record);
        if identity.is_air {
            air_states.push(identity.clone());
        }
        match sequential.kind() {
            VisualKind::Diagnostic => {
                *diagnostics_by_family
                    .entry(identity.model_family.clone())
                    .or_insert(0) += 1;
                *diagnostics_by_name
                    .entry(identity.name.clone())
                    .or_insert(0) += 1;
                if record.name.as_ref() == "minecraft:vine"
                    && let Some(mask) = record.model_state.get(ModelStateField::Connections)
                    && let Ok(mask) = u8::try_from(mask)
                {
                    vine_masks.push(mask);
                }
                diagnostics.push(identity);
            }
            VisualKind::Invisible => invisible.push(identity),
            _ => {}
        }
    }
    diagnostics.sort();
    invisible.sort();
    air_states.sort();
    vine_masks.sort_unstable();
    vine_masks.dedup();

    let names = records
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<BTreeSet<_>>()
        .len();
    let air = records
        .iter()
        .filter(|record| record.flags.contains(BlockFlags::AIR))
        .count();
    Ok(CoverageSnapshot {
        protocol: PROTOCOL,
        registry_sha256: registry_sha256.to_owned(),
        assets_sha256: assets_sha256.to_owned(),
        counts: Counts {
            names,
            states: records.len(),
            air,
        },
        states: ordered
            .into_iter()
            .map(StateIdentity::from_record)
            .collect(),
        diagnostic_states: diagnostics,
        diagnostics_by_family,
        diagnostics_by_name,
        invisible_states: invisible,
        air_states,
        vine_diagnostic_masks: vine_masks,
    })
}

pub fn ratchet(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
) -> Result<RatchetReport, CoverageError> {
    validate_baseline(baseline)?;
    if snapshot.registry_sha256 != baseline.registry_sha256 {
        return Err(CoverageError::RegistryHashMismatch {
            expected: baseline.registry_sha256.clone(),
            actual: snapshot.registry_sha256,
        });
    }
    if snapshot.counts != baseline.counts {
        return Err(CoverageError::CountMismatch {
            expected: baseline.counts,
            actual: snapshot.counts,
        });
    }
    if snapshot.states != baseline.states {
        return Err(CoverageError::StateInventoryMismatch);
    }

    let old = baseline
        .diagnostic_sequential_ids
        .iter()
        .map(|&sequential_id| baseline.states[sequential_id as usize].clone())
        .collect::<BTreeSet<_>>();
    let current = snapshot
        .diagnostic_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let added = current.difference(&old).cloned().collect::<Vec<_>>();
    if !added.is_empty() {
        return Err(CoverageError::DiagnosticRegression { states: added });
    }
    let removed = old.difference(&current).cloned().collect::<Vec<_>>();

    if snapshot.vine_diagnostic_masks != baseline.expected_vine_diagnostic_masks {
        return Err(CoverageError::VineDiagnosticsMismatch {
            expected: baseline.expected_vine_diagnostic_masks.clone(),
            actual: snapshot.vine_diagnostic_masks,
        });
    }

    let invisible_set = snapshot
        .invisible_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut allowlist = BTreeMap::new();
    for entry in &baseline.invisible_allowlist {
        if entry.authority.trim().is_empty()
            || !entry.source.starts_with("https://")
            || entry.source.len() <= "https://".len()
        {
            return Err(CoverageError::InvalidInvisibleCitation {
                state: entry.state.clone(),
            });
        }
        if allowlist.insert(entry.state.clone(), entry).is_some() {
            return Err(CoverageError::NonCanonicalBaseline);
        }
        if !invisible_set.contains(&entry.state) {
            return Err(CoverageError::StaleInvisibleAllowlist {
                state: entry.state.clone(),
            });
        }
    }

    let air_states = snapshot.air_states.iter().cloned().collect::<BTreeSet<_>>();
    let mut invisible_decisions = Vec::with_capacity(snapshot.invisible_states.len());
    for state in &snapshot.invisible_states {
        if air_states.contains(state) {
            invisible_decisions.push(InvisibleDecision {
                state: state.clone(),
                allowed: true,
                authority: "BREG1003 AIR flag".into(),
                source: "registry://BREG1003/air".into(),
            });
        } else if let Some(entry) = allowlist.get(state) {
            invisible_decisions.push(InvisibleDecision {
                state: state.clone(),
                allowed: true,
                authority: entry.authority.clone(),
                source: entry.source.clone(),
            });
        } else {
            return Err(CoverageError::UnreviewedInvisible {
                state: state.clone(),
            });
        }
    }

    Ok(RatchetReport {
        schema: REPORT_SCHEMA,
        protocol: snapshot.protocol,
        registry_sha256: snapshot.registry_sha256,
        assets_sha256: snapshot.assets_sha256,
        counts: snapshot.counts,
        states: snapshot.states,
        diagnostic_states: snapshot.diagnostic_states,
        diagnostics_by_family: snapshot.diagnostics_by_family,
        diagnostics_by_name: snapshot.diagnostics_by_name,
        added_diagnostics: Vec::new(),
        removed_diagnostics: removed,
        invisible_decisions,
        vine_diagnostic_masks: snapshot.vine_diagnostic_masks,
    })
}

/// Runs the production protocol-1001 gate. Synthetic tests may use `ratchet`
/// directly, but the CLI must never accept a caller-replaced smaller corpus.
pub fn ratchet_protocol_1001(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
) -> Result<RatchetReport, CoverageError> {
    validate_protocol_snapshot(&snapshot)?;
    validate_protocol_baseline(baseline)?;
    ratchet(snapshot, baseline)
}
