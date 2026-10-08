use super::*;
use crate::{ModGrants, PlayerStateEffect, PlayerStateSlot};
use cinnabar::extension::player_state::Host as _;

fn snapshot() -> PlayerStateSnapshot {
    PlayerStateSnapshot {
        session: 7,
        dimension: 0,
        selected_slot: Some(2),
        inventory: vec![
            PlayerStateSlot {
                known: false,
                item: None,
            };
            PLAYER_STATE_INVENTORY_SLOTS
        ],
        armor: vec![
            PlayerStateSlot {
                known: false,
                item: None
            };
            PLAYER_STATE_ARMOR_SLOTS
        ],
        offhand: PlayerStateSlot {
            known: false,
            item: None,
        },
        effects: vec![PlayerStateEffect {
            effect_id: 1,
            amplifier: 1,
            remaining_ticks: Some(120),
            ambient: false,
            particles: true,
        }],
    }
}

#[test]
fn local_facts_require_their_own_grant_and_expire_after_the_callback() {
    let mut state = State::new(ModGrants::default(), String::new());
    state.player_state.snapshot = Some(snapshot());
    assert!(state.read_snapshot().unwrap().is_err());
    state.grants.player_state = true;
    assert_eq!(state.read_snapshot().unwrap().unwrap(), Some(snapshot()));
    state.player_state.begin_frame();
    assert_eq!(state.read_snapshot().unwrap().unwrap(), None);
    assert!(
        state.snapshot.is_none(),
        "no camera/gameplay input is granted"
    );
}

#[test]
fn read_budget_is_bounded_and_renews_with_the_callback() {
    let mut state = State::new(
        ModGrants {
            player_state: true,
            ..Default::default()
        },
        String::new(),
    );
    for _ in 0..MAX_IMPORT_WRITES {
        assert!(state.read_snapshot().unwrap().is_ok());
    }
    assert!(state.read_snapshot().is_err());
    state.player_state.begin_frame();
    assert!(state.read_snapshot().unwrap().is_ok());
}

#[test]
fn malformed_snapshots_cannot_expose_items_or_expired_effects() {
    assert!(validate(Some(&snapshot())).is_ok());
    let mut bad = snapshot();
    bad.inventory.pop();
    assert!(validate(Some(&bad)).is_err());
    bad = snapshot();
    bad.selected_slot = Some(9);
    assert!(validate(Some(&bad)).is_err());
    bad = snapshot();
    bad.effects[0].remaining_ticks = Some(0);
    assert!(validate(Some(&bad)).is_err());
    bad = snapshot();
    bad.effects.push(bad.effects[0]);
    assert!(validate(Some(&bad)).is_err());
    bad = snapshot();
    bad.inventory[0].item = Some(PlayerStateItem {
        identifier: Some("minecraft:arrow".into()),
        network_id: 6,
        metadata: 0,
        count: 8,
        block: false,
        damage: None,
        max_durability: None,
    });
    assert!(
        validate(Some(&bad)).is_err(),
        "unknown cells cannot hold a presented item"
    );
    bad.inventory[0].known = true;
    assert!(validate(Some(&bad)).is_ok());
    bad.inventory[0].item.as_mut().unwrap().count = 0;
    assert!(validate(Some(&bad)).is_err());
}
