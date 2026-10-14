//! Exhaustive comparisons of retained and compiled placement classifiers.

use crate::placement_state::PlacementInput;

use super::{
    placement_doors, placement_facing, placement_signs, placement_stacking, placement_state,
    placement_support, toggle,
};
use assets::{
    NetworkIdMode, RegistryRecord,
    facts::{BlockFact, BlockFacts},
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Differences {
    counts: BTreeMap<&'static str, u64>,
    samples: BTreeSet<String>,
}

impl Differences {
    /// Counts every differing input while retaining a bounded diagnostic sample.
    fn record(&mut self, kind: &'static str, count: u64, pair: impl FnOnce() -> String) {
        *self.counts.entry(kind).or_default() += count;
        if self
            .samples
            .iter()
            .filter(|sample| sample.starts_with(kind))
            .count()
            < 20
        {
            self.samples.insert(format!("{kind}: {}", pair()));
        }
    }

    /// Reports the complete differential count and fails when any unexplained difference remains.
    fn finish(self) {
        assert!(
            self.counts.is_empty(),
            "differences={:?}\n{}",
            self.counts,
            self.samples.into_iter().collect::<Vec<_>>().join("\n")
        );
    }
}

/// Reads the actual palette used by the facts and the retained classifiers.
fn palette() -> Box<[RegistryRecord]> {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
}

#[derive(Clone)]
struct SupportInput {
    name: Option<String>,
    label: String,
    fact: BlockFact,
    count: u64,
}

/// Groups only identical classifier arguments, preserving the number of pinned state pairs.
fn support_inputs(records: &[RegistryRecord], facts: &BlockFacts) -> Vec<SupportInput> {
    let mut inputs: Vec<SupportInput> = Vec::new();
    for record in records {
        let fact = facts.get(record.sequential_id);
        if let Some(input) = inputs
            .iter_mut()
            .find(|input| input.name.as_deref() == Some(record.name.as_ref()) && input.fact == fact)
        {
            input.count += 1;
        } else {
            inputs.push(SupportInput {
                name: Some(record.name.to_string()),
                label: record.name.to_string(),
                fact,
                count: 1,
            });
        }
    }
    inputs
}

/// Materializes real custom sessions and retains their readable identity in each ID space.
fn custom_support_inputs(records: &[RegistryRecord]) -> Vec<SupportInput> {
    let mut registry = crate::movement::PhysicsCollisionRegistries::from_assets(
        assets::pinned_block_registry_bytes(),
        records,
        include_bytes!("../../../assets/data/block-physics-v2193.bin"),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let empty = [10, 0, 0];
    let mut custom = protocol::CustomBlocks::from_definitions([
        ("test:known_support", empty.as_slice()),
        ("minecraft:known_support", empty.as_slice()),
        ("test:custom_leaves", empty.as_slice()),
    ]);
    custom.vanilla_blocks = assets::server_defined_blocks()
        .iter()
        .map(|block| block.name.into())
        .collect();
    let (range, _) = registry.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(
        registry.begin_session_hashed_custom_blocks(&custom),
        custom.blocks.len()
    );
    let mut inputs = Vec::new();
    for (mode, ids) in [
        (NetworkIdMode::Sequential, range.collect::<Vec<_>>()),
        (
            NetworkIdMode::Hashed,
            custom
                .blocks
                .iter()
                .flat_map(|block| block.hashed_states().into_iter().map(|state| state.hash))
                .collect(),
        ),
    ] {
        for id in ids.into_iter().chain([u32::MAX]) {
            let name = registry.block_identifier(mode, id).map(str::to_owned);
            assert_eq!(registry.block_facts(mode, id).readable, name.is_some());
            assert_eq!(name.is_none(), id == u32::MAX);
            inputs.push(SupportInput {
                label: format!(
                    "{mode:?}:{}",
                    name.as_deref().unwrap_or("unknown runtime ID")
                ),
                name,
                fact: registry.block_facts(mode, id),
                count: 1,
            });
        }
    }
    inputs
}

#[test]
fn exhaustive_support_differential_covers_every_state_pair_and_custom_session() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let held = support_inputs(&records, &facts);
    let mut supports = held.clone();
    let custom = custom_support_inputs(&records);
    let custom_count = custom.len() as u64;
    supports.extend(custom);
    let mut differences = Differences::default();
    for record in &records {
        let states: Map<String, Value> = serde_json::from_str(&record.canonical_state).unwrap();
        let old = placement_support::support_requirement(&record.name, &states);
        let new =
            crate::placement_support::support_requirement(facts.get(record.sequential_id), &states);
        if old != new {
            differences.record("support requirement states", 1, || {
                format!(
                    "{} {}: {old:?} -> {new:?}",
                    record.name, record.canonical_state
                )
            });
        }
    }
    let mut compared = 0_u64;
    for held in &held {
        for support in &supports {
            for full_cube in [false, true] {
                let old = support
                    .name
                    .as_deref()
                    .map(|name| {
                        (
                            placement_support::accepts_support(
                                held.name.as_deref().unwrap(),
                                name,
                                full_cube,
                            ),
                            placement_support::support_acceptance(
                                held.name.as_deref().unwrap(),
                                name,
                                full_cube,
                            ),
                        )
                    })
                    .unwrap_or((false, None));
                let new = (
                    crate::placement_support::accepts_support(held.fact, support.fact, full_cube),
                    crate::placement_support::support_acceptance(
                        held.fact,
                        support.fact,
                        full_cube,
                    ),
                );
                let pairs = held.count * support.count;
                compared += pairs;
                if old != new {
                    differences.record("support pair decisions", pairs, || {
                        format!(
                            "{} / {} full_cube={full_cube}: {old:?} -> {new:?}",
                            held.label, support.label
                        )
                    });
                }
            }
        }
    }
    assert_eq!(
        compared,
        records.len() as u64 * (records.len() as u64 + custom_count) * 2
    );
    println!("Compared {compared} pinned/custom/unknown support pair decisions");
    differences.finish();
}

/// Converts a compiled counterpart back to its retained identifier for differential comparison.
fn named_result(
    result: Option<(u32, Map<String, Value>)>,
    records: &[RegistryRecord],
) -> Option<(String, Map<String, Value>)> {
    result.map(|(id, states)| (records[id as usize].name.to_string(), states))
}

#[test]
fn exhaustive_state_solvers_preserve_pinned_placement_decisions() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let mut differences = Differences::default();
    for record in &records {
        let fact = facts.get(record.sequential_id);
        let states: Map<String, Value> = serde_json::from_str(&record.canonical_state).unwrap();
        for face in 0..=5 {
            for (yaw, hit) in [(0.0, 0.499), (90.0, 0.5), (180.0, 0.501), (270.0, 1.0)] {
                let input = PlacementInput {
                    face,
                    yaw,
                    pitch: 0.0,
                    click_position: [hit; 3],
                };
                let old = placement_state::resolve_placement_state(
                    &record.name,
                    &record.canonical_state,
                    input,
                );
                let new = crate::placement_state::resolve_placement_state(
                    fact,
                    &record.canonical_state,
                    input,
                );
                if old != new {
                    differences.record("placement state inputs", 1, || {
                        format!(
                            "{} {} face={face} yaw={yaw}: {old:?} -> {new:?}",
                            record.name, record.canonical_state
                        )
                    });
                }
                let old =
                    placement_signs::sign_states(&record.name, &record.canonical_state, input);
                let new = named_result(
                    crate::placement_signs::sign_states(fact, &record.canonical_state, input),
                    &records,
                );
                if old != new {
                    differences.record("sign state inputs", 1, || {
                        format!("{} face={face}: {old:?} -> {new:?}", record.name)
                    });
                }
                for supports in [[Some(true); 6], [Some(false); 6], [None; 6]] {
                    let old = placement_support::attachment_states(
                        &record.name,
                        &states,
                        input,
                        supports,
                    );
                    let new =
                        crate::placement_support::attachment_states(fact, &states, input, supports);
                    if old != new {
                        differences.record("attachment state inputs", 1, || {
                            format!(
                                "{} {} face={face} supports={supports:?}: {old:?} -> {new:?}",
                                record.name, record.canonical_state
                            )
                        });
                    }
                }
            }
        }
        for position in [[0, -1, 0], [0, 3, 0], [3, 0, 0]] {
            for yaw in [0.0, 90.0, 180.0, 270.0] {
                let player = ([0.2, 0.0, 0.2], [0.8, 1.8, 0.8]);
                let old = placement_facing::facing_states(
                    &record.name,
                    &record.canonical_state,
                    position,
                    player,
                    yaw,
                );
                let new = crate::placement_facing::facing_states(
                    fact,
                    &record.canonical_state,
                    position,
                    player,
                    yaw,
                );
                if old != new {
                    differences.record("body facing inputs", 1, || record.name.to_string());
                }
            }
        }
        for neighbor in &records {
            // Only a door classifier reads neighbor evidence; all other held states return None.
            if !fact.traits.contains(assets::registry::BlockTraits::DOOR)
                && !placement_doors::is_door(&record.name)
            {
                continue;
            }
            let old = placement_doors::door_states(
                &record.name,
                &record.canonical_state,
                [0; 3],
                0.0,
                |_| Some(neighbor.name.to_string()),
            );
            let new = crate::placement_doors::door_states(
                fact,
                &record.canonical_state,
                [0; 3],
                0.0,
                |_| Some(facts.get(neighbor.sequential_id)),
            );
            if old != new {
                differences.record("door neighbor pairs", 1, || {
                    format!("{} / {}", record.name, neighbor.name)
                });
            }
        }
    }
    differences.finish();
}

#[test]
fn exhaustive_toggle_decisions_preserve_pinned_states() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    for record in &records {
        let old = toggle::toggled_states(&record.name, &record.canonical_state);
        let new = crate::block_use::toggled_states(
            facts.get(record.sequential_id),
            &record.canonical_state,
        );
        if record.name.as_ref() == "minecraft:trapdoor" {
            // GP-18: the canonical trapdoor name lacks the old switch suffix.
            assert!(old.is_none());
            assert_eq!(
                new,
                toggle::toggled_states("minecraft:birch_trapdoor", &record.canonical_state)
            );
        } else {
            assert_eq!(old, new, "{} {}", record.name, record.canonical_state);
        }
    }
}

#[test]
fn exhaustive_merge_decisions_preserve_pinned_state_pairs() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let groups = support_inputs(&records, &facts);
    let grouped_states: Vec<Vec<_>> = groups
        .iter()
        .map(|group| {
            records
                .iter()
                .filter(|record| {
                    record.name.as_ref() == group.name.as_deref().unwrap()
                        && facts.get(record.sequential_id) == group.fact
                })
                .collect()
        })
        .collect();
    let parsed: Vec<Map<String, Value>> = records
        .iter()
        .map(|record| serde_json::from_str(&record.canonical_state).unwrap())
        .collect();
    let mut differences = Differences::default();
    for (held_index, held) in groups.iter().enumerate() {
        let held_name = held.name.as_deref().unwrap();
        let held_states = &grouped_states[held_index];
        for (support_index, support) in groups.iter().enumerate() {
            let support_name = support.name.as_deref().unwrap();
            let support_states = &grouped_states[support_index];
            // Distinct identities return before reading either canonical state. Check that relation
            // for every identity pair, then compare every state pair within matching identities.
            assert!(
                held_name == support_name
                    || held.fact.identity.is_none()
                    || held.fact.identity != support.fact.identity
            );
            let held_count = if held_name == support_name {
                held_states.len()
            } else {
                1
            };
            let support_count = if held_name == support_name {
                support_states.len()
            } else {
                1
            };
            for held_state in &held_states[..held_count] {
                for support_state in &support_states[..support_count] {
                    let state = &parsed[support_state.sequential_id as usize];
                    for face in 0..=5 {
                        let old = placement_stacking::stacked_placement_state(
                            held_name,
                            support_name,
                            state,
                            face,
                        );
                        let new = crate::placement_stacking::stacked_placement_state(
                            held.fact,
                            support.fact,
                            state,
                            face,
                        );
                        if old != new {
                            differences.record("stack state pairs", 1, || {
                                format!(
                                    "{held_name} / {support_name} {} face={face}",
                                    support_state.canonical_state
                                )
                            });
                        }
                    }
                    for face in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
                        let old = placement_state::merge_slab_state(
                            held_name,
                            &held_state.canonical_state,
                            support_name,
                            &support_state.canonical_state,
                            face,
                        );
                        let new = named_result(
                            crate::placement_state::merge_slab_state(
                                held.fact,
                                &held_state.canonical_state,
                                support.fact,
                                &support_state.canonical_state,
                                face,
                            ),
                            &records,
                        );
                        if old != new {
                            differences.record("slab state pairs", 1, || {
                                format!(
                                    "{held_name} / {support_name} {} face={face:?}",
                                    support_state.canonical_state
                                )
                            });
                        }
                    }
                }
            }
        }
    }
    differences.finish();
}
