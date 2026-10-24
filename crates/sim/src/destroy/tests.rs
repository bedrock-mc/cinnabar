/// Reads the generated source only to build simulation fixtures.
fn block_destroy_info(identifier: &str) -> Option<BlockDestroyInfo> {
    BlockDestroyInfo::from_facts(assets::compiled::block_destroy::builtin_destroy_facts(
        identifier,
    ))
}

/// Builds ordered simulation fixtures from the one generated source.
fn table() -> Vec<(&'static str, BlockDestroyInfo)> {
    let mut rows = DESTROY_TABLE
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let name = line.split('\t').next()?;
            Some((name, block_destroy_info(name)?))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(name, _)| *name);
    rows
}

const DESTROY_TABLE: &str = assets::compiled::block_destroy::DESTROY_SOURCE;

use super::*;

/// Resolves a server component through the same facts path as the client.
fn custom(seconds: f32) -> BlockDestroyInfo {
    BlockDestroyInfo::from_facts(BlockDestroyFacts::Custom {
        seconds_to_destroy_bits: seconds.to_bits(),
        effective_tools: 0,
    })
    .unwrap()
}

#[test]
fn custom_mining_uses_component_value_and_preserves_instant_and_disabled() {
    assert_eq!(
        destroy_progress_per_tick(&custom(2.0), &grounded(None)),
        Some(1.0 / 60.0)
    );
    assert_eq!(
        destroy_progress_per_tick(&custom(0.5), &grounded(None)),
        Some(1.0 / 15.0)
    );
    assert_eq!(
        destroy_progress_per_tick(&custom(0.0), &grounded(None)),
        Some(1.0)
    );
    assert_eq!(
        destroy_progress_per_tick(&custom(-1.0), &grounded(None)),
        None
    );
}

#[test]
fn custom_mining_ordinary_tools_do_not_imply_an_effective_tool() {
    let mut conditions = grounded(Some("minecraft:diamond_pickaxe"));
    conditions.efficiency_level = 5;
    assert_eq!(
        destroy_progress_per_tick(&custom(2.0), &conditions),
        Some(1.0 / 60.0)
    );
    conditions.haste_amplifier = Some(0);
    let rate = destroy_progress_per_tick(&custom(2.0), &conditions).unwrap();
    assert!((rate - (1.2_f32 / 60.0 * 1.2)).abs() < f32::EPSILON);
    conditions.haste_amplifier = None;
    conditions.eyes_in_water = true;
    assert_eq!(
        destroy_progress_per_tick(&custom(2.0), &conditions),
        Some(0.2 / 60.0)
    );
}

#[test]
fn custom_mining_declared_tool_tags_apply_tier_and_enchantment_speed() {
    let facts = BlockDestroyFacts::Custom {
        seconds_to_destroy_bits: 2.0_f32.to_bits(),
        effective_tools: ToolKind::Pickaxe.bit(),
    };
    let block = BlockDestroyInfo::from_facts(facts).unwrap();
    let mut conditions = grounded(Some("minecraft:diamond_pickaxe"));
    assert_eq!(
        destroy_progress_per_tick(&block, &conditions),
        Some(8.0 / 60.0)
    );
    conditions.efficiency_level = 2;
    assert_eq!(
        destroy_progress_per_tick(&block, &conditions),
        Some(13.0 / 60.0)
    );
    conditions.tool = None;
    assert_eq!(
        destroy_progress_per_tick(&block, &conditions),
        Some(1.0 / 60.0)
    );
}

fn ticks(identifier: &str, conditions: DestroyConditions) -> Option<u32> {
    let block = block_destroy_info(identifier).expect("known block");
    destroy_progress_per_tick(&block, &conditions).map(|rate| (1.0 / rate).ceil() as u32)
}

fn grounded(tool: Option<&str>) -> DestroyConditions {
    DestroyConditions {
        tool: tool.map(|identifier| held_tool(identifier).expect("tool")),
        on_ground: true,
        ..DestroyConditions::default()
    }
}

#[test]
fn generated_table_carries_bedrock_hardness() {
    let stone = block_destroy_info("minecraft:stone").unwrap();
    assert_eq!(stone.hardness, 1.5);
    // Bedrock obsidian differs from other editions.
    assert_eq!(
        block_destroy_info("minecraft:obsidian").unwrap().hardness,
        35.0
    );
    assert!(block_destroy_info("minecraft:bedrock").unwrap().hardness < 0.0);
    assert_eq!(block_destroy_info("minecraft:not_a_block"), None);
    assert!(table().windows(2).all(|pair| pair[0].0 < pair[1].0));
}

#[test]
fn hand_and_tier_rates_match_documented_break_times() {
    assert_eq!(ticks("minecraft:dirt", grounded(None)), Some(15));
    assert_eq!(ticks("minecraft:stone", grounded(None)), Some(150));
    assert_eq!(
        ticks(
            "minecraft:stone",
            grounded(Some("minecraft:wooden_pickaxe"))
        ),
        Some(23)
    );
    assert_eq!(
        ticks("minecraft:oak_log", grounded(Some("minecraft:stone_axe"))),
        Some(15)
    );
    // A pickaxe is not an axe: log speed stays at the hand rate.
    assert_eq!(
        ticks(
            "minecraft:oak_log",
            grounded(Some("minecraft:diamond_pickaxe"))
        ),
        Some(60)
    );
}

#[test]
fn harvest_tier_selects_the_slow_divisor() {
    let iron = ticks(
        "minecraft:obsidian",
        grounded(Some("minecraft:iron_pickaxe")),
    )
    .unwrap();
    let diamond = ticks(
        "minecraft:obsidian",
        grounded(Some("minecraft:diamond_pickaxe")),
    )
    .unwrap();
    assert_eq!(diamond, 132);
    assert!(
        iron > diamond * 3,
        "iron {iron} must use the unharvestable divisor"
    );
    assert_eq!(
        ticks("minecraft:web", grounded(Some("minecraft:shears"))),
        Some(8)
    );
    assert_eq!(ticks("minecraft:web", grounded(None)), Some(400));
}

#[test]
fn environment_and_effects_scale_speed() {
    let base = grounded(Some("minecraft:wooden_pickaxe"));
    let airborne = DestroyConditions {
        on_ground: false,
        ..base
    };
    assert_eq!(ticks("minecraft:stone", airborne), Some(113));
    let submerged = DestroyConditions {
        eyes_in_water: true,
        ..base
    };
    assert_eq!(ticks("minecraft:stone", submerged), Some(113));
    let aqua = DestroyConditions {
        aqua_affinity: true,
        ..submerged
    };
    assert_eq!(ticks("minecraft:stone", aqua), Some(23));
    let haste = DestroyConditions {
        haste_amplifier: Some(1),
        ..base
    };
    assert_eq!(ticks("minecraft:stone", haste), Some(12));
    let fatigue = DestroyConditions {
        mining_fatigue_amplifier: Some(0),
        ..base
    };
    assert_eq!(ticks("minecraft:stone", fatigue), Some(108));
    let riding = DestroyConditions {
        riding: true,
        ..base
    };
    assert_eq!(ticks("minecraft:stone", riding), Some(113));
    let odd = DestroyConditions {
        mining_fatigue_amplifier: Some(-3),
        ..base
    };
    assert!(ticks("minecraft:stone", odd).unwrap() > 2_000);
}

/// Each effect rounds the resulting rate before the next multiplier is applied.
#[test]
fn native_destroy_effects_round_each_multiplier() {
    let block = block_destroy_info("minecraft:stone").unwrap();
    for (haste, fatigue, expected) in [
        (3, 0, 0x3dfba885),
        (3, 2, 0x3bb191f1),
        (4, 3, 0x3ac95d4e),
        (7, 0, 0x3ec3b084),
        (2, 1, 0x3c9a2404),
        (5, 4, 0x39e1873a),
    ] {
        let conditions = DestroyConditions {
            haste_amplifier: Some(haste - 1),
            mining_fatigue_amplifier: (fatigue > 0).then_some(fatigue - 1),
            ..grounded(Some("minecraft:wooden_pickaxe"))
        };
        assert_eq!(
            destroy_progress_per_tick(&block, &conditions)
                .unwrap()
                .to_bits(),
            expected
        );
    }
}

#[test]
fn zero_hardness_is_instant_and_negative_is_indestructible() {
    assert_eq!(ticks("minecraft:torch", grounded(None)), Some(1));
    assert_eq!(
        ticks(
            "minecraft:bedrock",
            grounded(Some("minecraft:netherite_pickaxe"))
        ),
        None
    );
}

#[test]
fn pinned_tool_traits_and_tiers_classify() {
    assert_eq!(
        held_tool("minecraft:golden_hoe"),
        Some(HeldTool {
            kind: ToolKind::Hoe,
            tier: Some(assets::facts::ItemTier::Gold)
        })
    );
    for identifier in [
        "minecraft:stick",
        "minecraft:iron_ingot",
        "minecraft:mace",
        "minecraft:iron_spear",
    ] {
        assert_eq!(held_tool(identifier), None, "{identifier}");
    }
}

#[test]
fn table_header_identifies_shared_catalog() {
    let catalog = include_str!("../../../../assets/block-projection-v2193.json");
    let headers = DESTROY_TABLE
        .lines()
        .take_while(|line| line.starts_with('#'))
        .collect::<Vec<_>>();
    let shared = headers
        .iter()
        .find(|line| line.starts_with("# hardness and tools: protocolgen/generated/data "))
        .expect("shared data header");
    let (_, source_lock) = shared
        .split_once("source_lock_sha256=")
        .expect("source lock");
    assert!(catalog.contains(&format!("\"source_lock_sha256\": \"{source_lock}\"")));
    let mining = headers
        .iter()
        .find(|line| line.starts_with("# special-tool speeds (provisional): PrismarineJS Bedrock "))
        .expect("mining evidence header");
    let (_, digest) = mining.split_once("sha256=").expect("mining digest");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

const EVERY_TOOL: [&str; 9] = [
    "minecraft:netherite_pickaxe",
    "minecraft:golden_pickaxe",
    "minecraft:netherite_axe",
    "minecraft:golden_shovel",
    "minecraft:netherite_hoe",
    "minecraft:golden_sword",
    "minecraft:shears",
    "minecraft:diamond_sword",
    "minecraft:stick",
];

/// Untagged native blocks have known hand harvestability instead of the slow fallback.
#[test]
fn native_no_tool_blocks_use_the_hand_harvest_rate() {
    assert_eq!(ticks("minecraft:bed", grounded(None)), Some(6));
    for name in ["minecraft:bed", "minecraft:glass"] {
        let block = block_destroy_info(name).unwrap();
        let expected = 1.0 / block.hardness / 30.0;
        assert_eq!(
            destroy_progress_per_tick(&block, &grounded(None)),
            Some(expected),
            "{name}"
        );
        for tool in EVERY_TOOL {
            let conditions = DestroyConditions {
                tool: held_tool(tool),
                ..grounded(None)
            };
            assert_eq!(
                destroy_progress_per_tick(&block, &conditions),
                Some(expected),
                "{name} with {tool}"
            );
        }
    }
    assert!(
        table()
            .iter()
            .all(|(_, info)| info.hardness < 0.0 || info.harvest != HarvestRequirement::Unresolved)
    );
}

#[test]
fn flying_is_exempt_from_the_airborne_penalty_but_riding_is_not() {
    let base = grounded(Some("minecraft:wooden_pickaxe"));
    let flying = DestroyConditions {
        on_ground: false,
        flying: true,
        ..base
    };
    assert_eq!(ticks("minecraft:stone", flying), Some(23));
    let riding_flight = DestroyConditions {
        riding: true,
        ..flying
    };
    assert_eq!(ticks("minecraft:stone", riding_flight), Some(113));
}

/// Bedrock swords clear bamboo in one tick, not at the generic sword speed.
#[test]
fn swords_clear_bamboo_at_the_bedrock_special_speed() {
    let bamboo = block_destroy_info("minecraft:bamboo").unwrap();
    let rate = destroy_progress_per_tick(&bamboo, &grounded(Some("minecraft:iron_sword")));
    assert_eq!(rate, Some(1.0));
    let sapling = block_destroy_info("minecraft:bamboo_sapling").unwrap();
    assert_eq!(
        destroy_progress_per_tick(&sapling, &grounded(Some("minecraft:wooden_sword"))),
        Some(1.0)
    );
    // The special case is sword-only.
    assert!(destroy_progress_per_tick(&bamboo, &grounded(None)).unwrap() < 0.1);
}

#[test]
fn review_extreme_effect_amplifiers_never_create_nonfinite_destroy_progress() {
    let block = block_destroy_info("minecraft:stone").unwrap();
    for amplifier in [500, 1000, i32::MAX] {
        for fatigue in [None, Some(amplifier)] {
            let conditions = DestroyConditions {
                on_ground: true,
                haste_amplifier: Some(amplifier),
                mining_fatigue_amplifier: fatigue,
                ..Default::default()
            };
            let progress = destroy_progress_per_tick(&block, &conditions).unwrap();
            assert!(
                progress.is_finite() && progress == 0.0,
                "unsupported effects predicted {progress}"
            );
        }
    }
}

#[test]
fn unresolved_destroy_facts_never_predict_completion() {
    assert_eq!(
        BlockDestroyInfo::from_facts(BlockDestroyFacts::Unresolved),
        None
    );
    let indestructible =
        BlockDestroyInfo::from_facts(BlockDestroyFacts::ProvisionalIndestructible).unwrap();
    assert_eq!(
        destroy_progress_per_tick(&indestructible, &DestroyConditions::default()),
        None
    );
}

/// Resolves fixture tools from the real pinned item carrier.
fn held_tool(identifier: &str) -> Option<HeldTool> {
    HeldTool::from_facts(
        assets::facts::ItemFacts::builtin()
            .unwrap()
            .by_identifier(identifier)?,
    )
}

#[test]
fn component_digger_speed_and_match_replace_vanilla_tool_rules() {
    let hand = ticks("minecraft:red_concrete", grounded(None));
    let matched = DestroyConditions {
        component_digger: Some(ComponentDigger {
            speed: 30.0,
            matched: true,
        }),
        ..grounded(None)
    };
    // A matched entry harvests the pickaxe-only block, so its rate uses the harvest divisor.
    assert_eq!(hand, Some(180));
    assert_eq!(ticks("minecraft:red_concrete", matched), Some(2));
    let unmatched = DestroyConditions {
        component_digger: Some(ComponentDigger {
            speed: 1.0,
            matched: false,
        }),
        efficiency_level: 5,
        ..grounded(Some("minecraft:diamond_pickaxe"))
    };
    assert_eq!(ticks("minecraft:red_concrete", unmatched), hand);
}
