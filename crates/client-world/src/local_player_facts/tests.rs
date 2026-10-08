use protocol::{ActorAttribute, GameModeUpdate, PlayerGameMode};

use super::{LocalPlayerFacts, LocalPlayerStat};

/// Builds hunger evidence with a valid default and no modifiers.
fn hunger(current: f32, maximum: f32) -> ActorAttribute {
    ActorAttribute {
        name: "minecraft:player.hunger".into(),
        min: 0.0,
        max: maximum,
        current,
        default: Some(0.0),
        modifiers: [].into(),
    }
}

#[test]
fn world_default_updates_only_players_bound_to_it() {
    let mut facts = LocalPlayerFacts::new(4);
    facts.publish_bootstrap_game_modes(PlayerGameMode::Survival, PlayerGameMode::Survival, true);
    assert!(
        facts.apply_default_game_mode_update(GameModeUpdate::Explicit(PlayerGameMode::Creative))
    );
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Creative));
    assert!(!facts.survival_stats_visible());

    assert!(facts.apply_game_mode_update(GameModeUpdate::Explicit(PlayerGameMode::Adventure)));
    assert!(
        facts.apply_default_game_mode_update(GameModeUpdate::Explicit(PlayerGameMode::Spectator))
    );
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Adventure));
    assert!(facts.survival_stats_visible());
    assert!(facts.apply_game_mode_update(GameModeUpdate::WorldDefault));
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Spectator));
}

#[test]
fn unresolved_and_odd_modes_preserve_existing_authority() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(facts.survival_stats_visible());
    assert_eq!(facts.game_mode_capabilities(), None);
    facts.publish_player_game_mode(PlayerGameMode::Adventure);
    assert!(!facts.apply_game_mode_update(GameModeUpdate::WorldDefault));
    assert!(!facts.apply_game_mode_update(GameModeUpdate::Unknown(87)));
    assert!(!facts.apply_default_game_mode_update(GameModeUpdate::WorldDefault));
    assert!(!facts.apply_default_game_mode_update(GameModeUpdate::Unknown(87)));
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Adventure));
}

#[test]
fn quantized_hunger_preserves_current_value_and_scale() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(facts.apply_hunger_attribute(&hunger(6.0, 20.0)));
    let hunger = facts.hunger().unwrap();
    assert_eq!(
        (hunger.current(), hunger.maximum(), hunger.scale()),
        (600, 2_000, 100)
    );
}

#[test]
fn invalid_hunger_keeps_the_previous_accepted_value() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(facts.apply_hunger_attribute(&hunger(12.5, 20.0)));
    let accepted = facts.hunger();
    for (current, maximum) in [
        (f32::NAN, 20.0),
        (f32::INFINITY, 20.0),
        (1.0, f32::INFINITY),
        (1.0, 0.0),
        (-1.0, 20.0),
        (21.0, 20.0),
        (0.0, 0.001),
        (1.0, f32::from(u16::MAX) + 1.0),
    ] {
        assert!(!facts.apply_hunger_attribute(&hunger(current, maximum)));
        assert_eq!(facts.hunger(), accepted);
    }
}

#[test]
fn large_attributes_keep_the_original_unscaled_fallback() {
    let stat = LocalPlayerStat::from_attribute(&hunger(777.6, 1_000.0)).unwrap();
    assert_eq!(
        (stat.current(), stat.maximum(), stat.scale()),
        (778, 1_000, 1)
    );
}

#[test]
fn session_reset_clears_all_facts_but_repeating_the_session_does_not() {
    let mut facts = LocalPlayerFacts::new(4);
    facts.publish_bootstrap_game_modes(PlayerGameMode::Creative, PlayerGameMode::Creative, true);
    assert!(facts.apply_hunger_attribute(&hunger(6.0, 20.0)));
    facts.set_mount(Some(-9));
    facts.begin_session(4);
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Creative));
    assert!(facts.hunger().is_some());
    assert_eq!(facts.mount_unique_id(), Some(-9));

    facts.begin_session(5);
    assert_eq!(facts.session_id(), 5);
    assert_eq!(facts.player_game_mode(), None);
    assert_eq!(facts.hunger(), None);
    assert_eq!(facts.mount_unique_id(), None);
    assert!(!facts.apply_game_mode_update(GameModeUpdate::WorldDefault));
}

#[test]
fn retiring_abilities_does_not_clear_other_retained_facts() {
    let mut facts = LocalPlayerFacts::new(4);
    facts.publish_player_game_mode(PlayerGameMode::Survival);
    assert!(facts.apply_hunger_attribute(&hunger(6.0, 20.0)));
    facts.set_mount(Some(8));
    facts.clear_local_abilities();
    assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Survival));
    assert!(facts.hunger().is_some());
    assert_eq!(facts.mount_unique_id(), Some(8));
    facts.set_mount(None);
    assert_eq!(facts.mount_unique_id(), None);
}

#[test]
fn block_breaking_negotiation_requires_successful_setup_in_the_same_session() {
    let mut facts = LocalPlayerFacts::new(4);
    assert_eq!(facts.server_authoritative_block_breaking(), None);
    facts.install_block_breaking_mode(3, true, true);
    facts.install_block_breaking_mode(4, true, false);
    assert_eq!(facts.server_authoritative_block_breaking(), None);
    facts.install_block_breaking_mode(4, false, true);
    assert_eq!(facts.server_authoritative_block_breaking(), Some(false));
    facts.clear_block_breaking_mode();
    assert_eq!(facts.server_authoritative_block_breaking(), None);
    facts.install_block_breaking_mode(4, true, true);
    facts.begin_session(5);
    assert_eq!(facts.server_authoritative_block_breaking(), None);
}

fn immobile(value: bool) -> crate::MovementFlagUpdate {
    crate::MovementFlagUpdate {
        immobile: Some(value),
        ..Default::default()
    }
}

#[test]
fn local_immobility_retains_omitted_metadata_until_an_explicit_clear() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(!facts.is_immobile());
    assert!(facts.apply_local_movement_flags(4, immobile(true)));
    assert!(facts.is_immobile());
    assert!(facts.apply_local_movement_flags(4, crate::MovementFlagUpdate::default()));
    facts.publish_player_game_mode(PlayerGameMode::Creative);
    facts.set_mount(Some(9));
    facts.clear_local_abilities();
    assert!(
        facts.is_immobile(),
        "other player facts do not release travel"
    );
    assert!(facts.apply_local_movement_flags(4, immobile(false)));
    assert!(!facts.is_immobile());
}

#[test]
fn local_immobility_preserves_same_session_and_retires_on_session_replacement() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(!facts.apply_local_movement_flags(3, immobile(true)));
    assert!(!facts.is_immobile());
    assert!(facts.apply_local_movement_flags(4, immobile(true)));
    assert!(!facts.apply_local_movement_flags(3, immobile(false)));
    facts.begin_session(4);
    assert!(facts.is_immobile());
    facts.begin_session(5);
    assert!(!facts.is_immobile());
    assert!(!facts.apply_local_movement_flags(4, immobile(true)));
    assert!(!facts.is_immobile());
    assert!(facts.apply_local_movement_flags(5, immobile(true)));
    assert!(facts.is_immobile());
}

#[test]
fn gravity_defaults_on_and_follows_only_explicit_flag_words() {
    let mut facts = LocalPlayerFacts::new(4);
    assert!(facts.has_gravity() && !facts.uses_uniform_air_drag());
    assert_eq!(facts.air_drag_modifier(), None);
    assert!(facts.apply_local_movement_flags(4, immobile(false)));
    assert!(
        facts.has_gravity(),
        "an update without the gravity bit retains it"
    );
    let cleared = crate::MovementFlagUpdate {
        has_gravity: Some(false),
        uniform_air_drag: Some(true),
        ..Default::default()
    };
    assert!(facts.apply_local_movement_flags(4, cleared));
    assert!(!facts.has_gravity() && facts.uses_uniform_air_drag());
    assert!(!facts.apply_air_drag_modifier(3, 2.0));
    assert!(facts.apply_air_drag_modifier(4, 2.0));
    assert_eq!(facts.air_drag_modifier(), Some(2.0));
    facts.begin_session(5);
    assert!(facts.has_gravity() && !facts.uses_uniform_air_drag());
    assert_eq!(facts.air_drag_modifier(), None);
}
