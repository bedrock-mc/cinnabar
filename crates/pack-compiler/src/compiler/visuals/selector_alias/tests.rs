use super::*;

fn current_records() -> Vec<RegistryRecord> {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .into_iter()
    .filter(|record| is_selector_alias_cube_name(&record.name))
    .collect()
}

#[test]
fn selector_alias_cubes_admit_the_active_registry_state_product() {
    let records = current_records();
    for record in &records {
        assert!(
            is_selector_alias_cube_record(record),
            "{} state={} id={} flags={:?} role={:?} collision={:?}",
            record.name,
            record.canonical_state,
            record.sequential_id,
            record.flags,
            record.contributor_role,
            record.collision_seed,
        );
    }
    assert!(selector_alias_cube_inventory_is_exact(&records));
}

#[test]
fn selector_alias_cube_inventory_rejects_duplicate_or_missing_states() {
    let records = current_records();
    assert!(selector_alias_cube_inventory_is_exact(&records));
    let mut duplicate = records.clone();
    duplicate[1].canonical_state = duplicate[0].canonical_state.clone();
    duplicate[1].model_state = duplicate[0].model_state;
    duplicate[1].name = duplicate[0].name.clone();
    assert!(!selector_alias_cube_inventory_is_exact(&duplicate));
    assert!(!selector_alias_cube_inventory_is_exact(&records[1..]));
}
