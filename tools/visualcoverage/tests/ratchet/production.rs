use super::*;

#[test]
#[ignore = "requires CINNABAR_REAL_PACK pointing at the ignored pinned MCBEAS07 blob"]
fn production_ratchet_separates_zero_diagnostics_from_provisional_fallbacks() {
    let assets_path = std::env::var_os("CINNABAR_REAL_PACK")
        .map(std::path::PathBuf::from)
        .expect("set CINNABAR_REAL_PACK to the ignored pinned vanilla-v1001.mcbea");
    assert!(assets_path.is_file(), "missing real pack: {assets_path:?}");
    let registry_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/assets/data/block-registry-v1001.bin");
    let registry_bytes = std::fs::read(&registry_path).unwrap();
    let assets_bytes = std::fs::read(&assets_path).unwrap();
    let baseline = parse_baseline(include_bytes!(
        "../../../../crates/assets/data/visual-coverage-v1001.json"
    ))
    .expect("parse committed production baseline");
    let current = analyze_bytes(&registry_bytes, &assets_bytes).unwrap();

    assert_eq!(current.states.len(), 16_913);
    assert_eq!(baseline.diagnostic_sequential_ids.len(), 2_415);
    assert_eq!(current.diagnostic_states.len(), 383);
    assert_eq!(current.fallback_states.len(), 2_031);
    assert_eq!(current.fallbacks_by_name.len(), 335);
    assert!(current.fallback_states.iter().all(|state| !state.is_air));

    let expected_fallback_ids = baseline
        .diagnostic_sequential_ids
        .iter()
        .copied()
        .filter(|&id| {
            !baseline.states[id as usize].is_air
                && baseline.states[id as usize].name != "cinnabar:reserved"
        })
        .collect::<Vec<_>>();
    let actual_fallback_ids = current
        .fallback_states
        .iter()
        .map(|state| state.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(actual_fallback_ids, expected_fallback_ids);

    let report = ratchet_protocol_1001(current, &baseline).expect("run production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 2_032);
    assert_eq!(report.fallback_states.len(), 2_031);
    assert_eq!(report.fallbacks_by_name.len(), 335);
}
