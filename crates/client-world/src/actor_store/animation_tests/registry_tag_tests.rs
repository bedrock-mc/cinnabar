use std::{collections::HashMap, sync::Arc};

use actor_animation::{ActorAnimationStore, ActorTickContext};
use assets::{CompiledMolangExpression, MolangCall, MolangOp, MolangSymbol, MolangSymbolKind};

/// Encodes a short string field or payload in the original network-NBT fixture.
fn name(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(u8::try_from(value.len()).unwrap());
    bytes.extend_from_slice(value.as_bytes());
}

/// Writes the supported list encoding using real tags from the pinned item census.
fn tags(bytes: &mut Vec<u8>, field: &str, values: &[&str]) {
    bytes.push(9);
    name(bytes, field);
    bytes.extend([8, u8::try_from(values.len() * 2).unwrap()]);
    for value in values {
        name(bytes, value);
    }
}

/// Builds legacy, authored, or combined tag declarations without native item-name inference.
fn registry(legacy: bool, authored: bool) -> protocol::ItemRegistryEvent {
    let mut bytes = vec![10, 0, 10];
    name(&mut bytes, "components");
    if legacy {
        tags(&mut bytes, "item_tags", &["minecraft:is_spear"]);
    }
    if authored {
        bytes.push(10);
        name(&mut bytes, "minecraft:tags");
        tags(
            &mut bytes,
            "tags",
            &["minecraft:is_spear", "minecraft:is_sword"],
        );
        bytes.push(0);
    }
    bytes.extend([0, 0]);
    protocol::test_support::items::component_item_registry_payload(
        "minecraft:iron_sword",
        10_001,
        bytes,
    )
}

/// Makes the hand-tag query visible in a rig's red tint channel.
fn query_assets() -> Arc<assets::RuntimeEntityAssets> {
    let mut compiled =
        actor_animation::test_support::counting_random_compiled("minecraft:iron_golem");
    compiled.molang_symbols = vec![
        MolangSymbol {
            kind: MolangSymbolKind::Name,
            identifier: "wield".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.equipped_item_any_tag".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.charge_amount".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::String,
            identifier: "minecraft:is_spear".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::String,
            identifier: "slot.weapon.mainhand".into(),
        },
    ]
    .into_boxed_slice();
    compiled.molang_ops = vec![
        MolangOp::PushString(4),
        MolangOp::PushString(3),
        MolangOp::CallQuery(MolangCall {
            symbol: 1,
            arguments: 2,
        }),
        MolangOp::StoreVariable(2),
        MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::LoadVariable(2),
        MolangOp::Push(assets::EntityGeometryScalar::new(1.0).unwrap()),
        MolangOp::Push(assets::EntityGeometryScalar::new(1.0).unwrap()),
    ]
    .into_boxed_slice();
    compiled.molang_expressions = [(0, 5, 2), (5, 1, 1), (6, 1, 1), (7, 1, 1)]
        .into_iter()
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .collect();
    Arc::new(
        assets::RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    )
}

#[test]
fn component_registry_tag_encodings_reach_the_numeric_hand_query() {
    let mut actor = super::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: "minecraft:iron_golem".into(),
    };
    let actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut animation = ActorAnimationStore::with_assets(query_assets());
    animation.insert(1, 0, &actors[&1]);
    let mut items = crate::item::ItemStateStore::diagnostic();
    for (legacy, authored) in [(true, false), (false, true), (true, true), (false, false)] {
        let registry = registry(legacy, authored);
        assert_eq!(
            registry.entries[0]
                .components
                .as_ref()
                .unwrap()
                .attack
                .as_ref()
                .is_some_and(|attack| attack.is_spear),
            legacy || authored
        );
        assert!(items.apply_registry(registry));
        for in_main_hand in [true, false] {
            let fact = items.animation_fact(10_001);
            animation.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
                held_item_facts: if in_main_hand {
                    [fact, Default::default()]
                } else {
                    [Default::default(), fact]
                },
                ..Default::default()
            });
            assert_eq!(
                animation.get(1).unwrap().render[0].color[0],
                if in_main_hand && (legacy || authored) {
                    1.0
                } else {
                    0.0
                }
            );
        }
    }
}
