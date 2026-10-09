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
    state.player_state.set_snapshot(Some(snapshot())).unwrap();
    assert!(state.read_snapshot().unwrap().is_err());
    assert!(state.read_revision().unwrap().is_err());
    state.grants.player_state = true;
    assert_eq!(state.read_snapshot().unwrap().unwrap(), Some(snapshot()));
    assert!(state.read_revision().unwrap().unwrap().is_some());
    state.player_state.begin_frame();
    assert_eq!(state.read_snapshot().unwrap().unwrap(), None);
    assert_eq!(state.read_revision().unwrap().unwrap(), None);
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
    for index in 0..MAX_IMPORT_WRITES {
        if index % 2 == 0 {
            assert!(state.read_snapshot().unwrap().is_ok());
        } else {
            assert!(state.read_revision().unwrap().is_ok());
        }
    }
    assert!(state.read_revision().is_err());
    state.player_state.begin_frame();
    assert!(state.read_snapshot().unwrap().is_ok());
    assert!(state.read_revision().unwrap().is_ok());
}

/// Creates a valid item observation so identifier and scalar changes are exercised together.
fn observed_snapshot() -> PlayerStateSnapshot {
    let mut snapshot = snapshot();
    snapshot.inventory[0] = PlayerStateSlot {
        known: true,
        item: Some(PlayerStateItem {
            identifier: Some("minecraft:arrow".into()),
            network_id: 6,
            metadata: 0,
            count: 8,
            block: false,
            damage: Some(2),
            max_durability: Some(20),
        }),
    };
    snapshot
}

#[test]
fn equal_current_facts_keep_the_revision_across_callbacks() {
    let mut state = State::new(
        ModGrants {
            player_state: true,
            ..Default::default()
        },
        String::new(),
    );
    let facts = observed_snapshot();
    state
        .player_state
        .set_snapshot(Some(facts.clone()))
        .unwrap();
    let revision = state.read_revision().unwrap().unwrap().unwrap();
    state.player_state.begin_frame();
    assert_eq!(state.read_revision().unwrap().unwrap(), None);
    assert_eq!(state.read_snapshot().unwrap().unwrap(), None);
    state
        .player_state
        .set_snapshot(Some(facts.clone()))
        .unwrap();
    assert_eq!(state.read_revision().unwrap().unwrap(), Some(revision));
    assert_eq!(state.read_snapshot().unwrap().unwrap(), Some(facts));
}

#[test]
fn revision_changes_for_exact_identifiers_scalars_ticks_and_session_owners() {
    let mut state = State::new(
        ModGrants {
            player_state: true,
            ..Default::default()
        },
        String::new(),
    );
    let changes: &[fn(&mut PlayerStateSnapshot)] = &[
        |facts| facts.session += 1,
        |facts| facts.dimension += 1,
        |facts| facts.selected_slot = Some(3),
        |facts| facts.inventory[1].known = true,
        |facts| facts.inventory[0].item = None,
        |facts| facts.inventory[0].item.as_mut().unwrap().identifier = None,
        |facts| {
            facts.inventory[0].item.as_mut().unwrap().identifier =
                Some("minecraft:ender_pearl".into())
        },
        |facts| facts.inventory[0].item.as_mut().unwrap().network_id += 1,
        |facts| facts.inventory[0].item.as_mut().unwrap().metadata += 1,
        |facts| facts.inventory[0].item.as_mut().unwrap().count += 1,
        |facts| facts.inventory[0].item.as_mut().unwrap().block = true,
        |facts| facts.inventory[0].item.as_mut().unwrap().damage = None,
        |facts| facts.inventory[0].item.as_mut().unwrap().damage = Some(3),
        |facts| facts.inventory[0].item.as_mut().unwrap().max_durability = Some(21),
        |facts| facts.armor[0].known = true,
        |facts| facts.offhand.known = true,
        |facts| facts.effects[0].effect_id += 1,
        |facts| facts.effects[0].amplifier += 1,
        |facts| facts.effects[0].remaining_ticks = Some(119),
        |facts| facts.effects[0].remaining_ticks = None,
        |facts| facts.effects[0].ambient = true,
        |facts| facts.effects[0].particles = false,
        |facts| facts.effects.clear(),
    ];
    for change in changes {
        state.player_state.begin_frame();
        let mut facts = observed_snapshot();
        state
            .player_state
            .set_snapshot(Some(facts.clone()))
            .unwrap();
        let before = state.read_revision().unwrap().unwrap().unwrap();
        change(&mut facts);
        assert!(validate(Some(&facts)).is_ok());
        state
            .player_state
            .set_snapshot(Some(facts.clone()))
            .unwrap();
        let after = state.read_revision().unwrap().unwrap().unwrap();
        assert_ne!(before, after);
        assert_eq!(state.read_snapshot().unwrap().unwrap(), Some(facts));
    }
}

#[test]
fn unavailable_or_revoked_facts_release_history_and_never_reuse_a_token() {
    let mut state = State::new(
        ModGrants {
            player_state: true,
            ..Default::default()
        },
        String::new(),
    );
    state.player_state.set_snapshot(Some(snapshot())).unwrap();
    let first = state.read_revision().unwrap().unwrap().unwrap();
    state.player_state.begin_frame();
    state.player_state.set_snapshot(None).unwrap();
    assert_eq!(state.read_revision().unwrap().unwrap(), None);
    assert!(state.player_state.previous_snapshot.is_none());
    state.player_state.set_snapshot(Some(snapshot())).unwrap();
    let reappeared = state.read_revision().unwrap().unwrap().unwrap();
    assert_ne!(first, reappeared);
    state.player_state.revoke();
    assert_eq!(state.read_snapshot().unwrap().unwrap(), None);
    assert_eq!(state.read_revision().unwrap().unwrap(), None);
    assert!(state.player_state.previous_snapshot.is_none());
    state.player_state.set_snapshot(Some(snapshot())).unwrap();
    assert_ne!(state.read_revision().unwrap().unwrap().unwrap(), reappeared);
}

#[test]
fn revision_exhaustion_releases_facts_instead_of_reusing_an_old_token() {
    let mut facts = PlayerState::default();
    facts.set_snapshot(Some(snapshot())).unwrap();
    facts.revision = u64::MAX;
    let mut changed = snapshot();
    changed.session += 1;
    assert!(facts.set_snapshot(Some(changed)).is_err());
    assert!(facts.snapshot.is_none() && facts.previous_snapshot.is_none());
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
