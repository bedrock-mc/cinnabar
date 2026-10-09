use super::*;

fn ticks(identifier: &str, conditions: DestroyConditions) -> Option<u32> {
    let block = block_destroy_info(identifier).expect("known block");
    destroy_progress_per_tick(&block, &conditions).map(|rate| (1.0 / rate).ceil() as u32)
}

fn grounded(tool: Option<&str>) -> DestroyConditions {
    DestroyConditions {
        tool: tool.map(|identifier| HeldTool::from_identifier(identifier).expect("tool")),
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
fn only_vanilla_tool_identifiers_classify() {
    assert_eq!(
        HeldTool::from_identifier("minecraft:golden_hoe"),
        Some(HeldTool {
            kind: ToolKind::Hoe,
            tier: Some(ToolTier::Gold)
        })
    );
    for identifier in [
        "minecraft:stick",
        "minecraft:iron_shears",
        "custom:iron_pickaxe",
        "minecraft:iron_ingot",
    ] {
        assert_eq!(HeldTool::from_identifier(identifier), None, "{identifier}");
    }
}

#[test]
fn table_header_pins_manifest_sources() {
    let manifest = include_str!("../../../../assets/block-data-sources.json");
    let pins = DESTROY_TABLE
        .lines()
        .take_while(|line| line.starts_with('#'))
        .filter_map(|line| line.split_once("sha256=").map(|(_, digest)| digest))
        .collect::<Vec<_>>();
    assert_eq!(pins.len(), 2);
    for digest in pins {
        assert!(
            manifest.contains(&format!("\"sha256\": \"{digest}\"")),
            "{digest}"
        );
    }
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

#[test]
fn rows_without_tool_evidence_take_the_slowest_rate_for_every_tool() {
    let unresolved = table()
        .iter()
        .filter(|(_, info)| info.harvest == HarvestRequirement::Unresolved && info.hardness > 0.0)
        .collect::<Vec<_>>();
    for name in [
        "minecraft:crafter",
        "minecraft:glowingobsidian",
        "minecraft:trial_spawner",
    ] {
        assert!(
            unresolved.iter().any(|(identifier, _)| *identifier == name),
            "{name}"
        );
    }
    for (identifier, info) in unresolved {
        let slowest = 1.0 / info.hardness / 100.0;
        for tool in EVERY_TOOL {
            let conditions = DestroyConditions {
                tool: HeldTool::from_identifier(tool),
                on_ground: true,
                ..DestroyConditions::default()
            };
            assert_eq!(
                destroy_progress_per_tick(info, &conditions),
                Some(slowest),
                "{identifier} with {tool}"
            );
        }
    }
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
