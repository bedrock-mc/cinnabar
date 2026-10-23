use protocol::{AbilitiesUpdate, AbilityLayerEvidence, AbilityLayersEvidence};

use super::{
    CREATIVE_ATTACK_REACH, FlightAuthority, GameModeCapabilities, HeldEditItem,
    SURVIVAL_ATTACK_REACH, ability_bit,
};
use protocol::PlayerGameMode::{Adventure, Creative, Spectator, Survival, Unknown};

/// Builds one received base ability layer.
fn layer(abilities: u32, values: u32) -> AbilityLayerEvidence {
    AbilityLayerEvidence {
        layer_type: 1,
        abilities,
        values,
        fly_speed_bits: 0,
        vertical_fly_speed_bits: 0,
        walk_speed_bits: 0,
    }
}

/// Wraps received layers in local-player ability evidence.
fn update(layers: Vec<AbilityLayerEvidence>) -> AbilitiesUpdate {
    AbilitiesUpdate {
        actor_unique_id: 0,
        player_permission: 1,
        command_permission: 0,
        layers: AbilityLayersEvidence::Received(layers.into()),
    }
}

/// Per-mode defaults match dragonfly's `world.GameMode` plus creative instant break.
#[test]
fn mode_defaults_follow_the_gamemode_table() {
    let survival = GameModeCapabilities::for_mode(Survival);
    assert!(survival.can_build && survival.can_mine && survival.can_attack);
    assert!(survival.can_use_switches && survival.can_open_containers);
    assert!(!survival.can_fly && !survival.creative_inventory && !survival.instant_break);
    assert!(!survival.invulnerable && survival.visible && survival.has_collision);
    assert!(matches!(survival.flight, FlightAuthority::Mutable(_)));
    assert_eq!(survival.attack_reach, SURVIVAL_ATTACK_REACH);
    assert!(!survival.creative_reach);

    let creative = GameModeCapabilities::for_mode(Creative);
    assert!(creative.can_build && creative.can_mine && creative.can_attack);
    assert!(creative.can_fly && creative.creative_inventory && creative.instant_break);
    assert!(creative.invulnerable && creative.visible && creative.has_collision);
    assert!(matches!(creative.flight, FlightAuthority::Mutable(_)));
    assert_eq!(creative.attack_reach, CREATIVE_ATTACK_REACH);
    assert!(creative.creative_reach);

    let adventure = GameModeCapabilities::for_mode(Adventure);
    assert!(
        !adventure.can_build && !adventure.can_mine,
        "adventure cannot edit without a grant"
    );
    assert!(adventure.can_use_switches && adventure.can_open_containers && adventure.can_attack);
    assert!(!adventure.can_fly && !adventure.instant_break && !adventure.invulnerable);
    assert!(adventure.visible && adventure.has_collision);
    assert!(matches!(adventure.flight, FlightAuthority::Mutable(_)));
    assert_eq!(adventure.attack_reach, SURVIVAL_ATTACK_REACH);

    let spectator = GameModeCapabilities::for_mode(Spectator);
    assert!(!spectator.can_use_blocks() && !spectator.can_mine && !spectator.can_attack);
    assert!(!spectator.can_use_items, "spectators cannot use held items");
    assert!(survival.can_use_items && creative.can_use_items && adventure.can_use_items);
    assert!(!spectator.can_fly && spectator.flight.is_flying() && spectator.invulnerable);
    assert!(!spectator.visible && !spectator.has_collision);
    assert_eq!(spectator.flight, FlightAuthority::Fixed(true));
    assert_eq!(spectator.attack_reach, 0.0);

    // An unrecognized id interacts like vanilla's base game mode instead of failing closed.
    let unknown = GameModeCapabilities::for_mode(Unknown);
    assert!(unknown.can_use_blocks() && unknown.can_mine && unknown.can_attack);
    assert!(unknown.can_use_items && !unknown.can_fly);
    assert!(matches!(unknown.flight, FlightAuthority::Mutable(_)));
    assert_eq!(unknown.attack_reach, survival.attack_reach);
}

/// Build and Mine grants alone do not let an adventure player edit with an empty hand:
/// vanilla also needs PrivilegedBuilder there.
#[test]
fn adventure_edits_need_privileged_builder_with_an_empty_hand() {
    let grant = update(vec![layer(
        ability_bit::BUILD | ability_bit::MINE,
        ability_bit::BUILD | ability_bit::MINE,
    )]);
    let caps = GameModeCapabilities::resolve(Adventure, Some(&grant));
    assert!(
        caps.can_build && caps.can_mine,
        "the grants themselves resolve"
    );
    assert!(
        !caps.permits_edit(HeldEditItem::EmptyHand),
        "an adventure Build/Mine grant is not PrivilegedBuilder"
    );
    // A grant does not fabricate the other creative privileges.
    assert!(!caps.can_fly && !caps.instant_break && !caps.creative_inventory);

    let privileged = update(vec![layer(
        ability_bit::BUILD | ability_bit::MINE | ability_bit::PRIVILEGED_BUILDER,
        ability_bit::BUILD | ability_bit::MINE | ability_bit::PRIVILEGED_BUILDER,
    )]);
    let caps = GameModeCapabilities::resolve(Adventure, Some(&privileged));
    assert!(caps.permits_edit(HeldEditItem::EmptyHand));
    assert!(
        !caps.permits_edit(HeldEditItem::Item { lists_block: false }),
        "a held item's list replaces PrivilegedBuilder"
    );
}

/// In Adventure a held item edits exactly the blocks its list names.
#[test]
fn adventure_held_item_lists_decide_each_block() {
    let caps = GameModeCapabilities::resolve(Adventure, None);
    assert!(caps.permits_edit(HeldEditItem::Item { lists_block: true }));
    assert!(!caps.permits_edit(HeldEditItem::Item { lists_block: false }));
    assert!(!caps.permits_edit(HeldEditItem::EmptyHand));
}

/// Survival and Creative edits follow the Build and Mine grants alone.
#[test]
fn editing_modes_ignore_item_lists_and_privileged_builder() {
    for mode in [Survival, Creative, Unknown] {
        let caps = GameModeCapabilities::resolve(mode, None);
        for held in [
            HeldEditItem::EmptyHand,
            HeldEditItem::Item { lists_block: false },
        ] {
            assert!(caps.permits_edit(held), "{mode:?} {held:?}");
        }
    }
}

#[test]
fn spectator_mode_suppresses_stale_build_and_flight_toggle_permissions() {
    let base = update(vec![layer(u32::MAX, u32::MAX)]);
    let caps = GameModeCapabilities::resolve(Spectator, Some(&base));
    assert!(!caps.can_use_blocks() && !caps.can_mine && !caps.can_attack);
    assert!(caps.flight.is_flying() && !caps.has_collision);
    assert!(!caps.can_fly, "spectator flight cannot be toggled off");
    assert_eq!(caps.flight, FlightAuthority::Fixed(true));
}

/// Build and Mine are separate grants; neither implies the other.
#[test]
fn build_and_mine_grants_stay_separate() {
    let mine = update(vec![layer(ability_bit::MINE, ability_bit::MINE)]);
    let caps = GameModeCapabilities::resolve(Adventure, Some(&mine));
    assert!(caps.can_mine && !caps.can_build);
    let build = update(vec![layer(ability_bit::BUILD, ability_bit::BUILD)]);
    let caps = GameModeCapabilities::resolve(Adventure, Some(&build));
    assert!(caps.can_build && !caps.can_mine);
    let no_doors = update(vec![layer(ability_bit::DOORS_AND_SWITCHES, 0)]);
    let caps = GameModeCapabilities::resolve(Survival, Some(&no_doors));
    assert!(!caps.can_use_switches && caps.can_open_containers);
}

/// No abilities, empty layers, and unavailable evidence all keep mode defaults.
#[test]
fn absent_or_undefined_abilities_keep_mode_defaults() {
    assert!(!GameModeCapabilities::resolve(Adventure, None).can_mine);
    let empty = update(vec![]);
    assert!(!GameModeCapabilities::resolve(Adventure, Some(&empty)).can_mine);
    assert!(GameModeCapabilities::resolve(Survival, Some(&empty)).can_mine);
    // A layer that defines unrelated bits leaves Build/Mine untouched.
    let unrelated = update(vec![layer(ability_bit::FLYING, ability_bit::FLYING)]);
    assert!(!GameModeCapabilities::resolve(Adventure, Some(&unrelated)).can_mine);
    let unavailable = AbilitiesUpdate {
        layers: AbilityLayersEvidence::Unavailable {
            declared_layers: 99,
        },
        ..update(vec![])
    };
    assert!(GameModeCapabilities::resolve(Survival, Some(&unavailable)).can_mine);
}

/// An explicit deny is honored even against an editing mode's default.
#[test]
fn explicit_deny_overrides_the_mode_default() {
    let deny = update(vec![layer(ability_bit::BUILD | ability_bit::MINE, 0)]);
    assert!(!GameModeCapabilities::resolve(Survival, Some(&deny)).can_mine);
}

/// Flight, invulnerability and collision remain independent ability grants.
#[test]
fn ability_bits_refine_flight_and_collision_without_creative_mining() {
    let bits = ability_bit::MAY_FLY
        | ability_bit::FLYING
        | ability_bit::INSTANT_BUILD
        | ability_bit::INVULNERABLE
        | ability_bit::NO_CLIP;
    let all = update(vec![layer(bits, bits)]);
    let caps = GameModeCapabilities::resolve(Survival, Some(&all));
    assert!(caps.can_fly && caps.flight.is_flying() && caps.invulnerable);
    assert!(
        !caps.instant_break,
        "Instabuild does not select creative destruction"
    );
    assert!(!caps.has_collision, "no-clip removes collision");
    assert!(
        matches!(caps.flight, FlightAuthority::Mutable(_)),
        "base-layer Flying remains locally mutable"
    );
}

/// Freecam/noclip shape: base grants MayFly|Flying|NoClip, spectator grants Flying.
/// Both noclip and forced spectator flight apply outside spectator mode.
#[test]
fn freecam_packet_shape_keeps_collision_and_fixed_flight() {
    use super::SPECTATOR_LAYER_TYPE;
    let base_bits = ability_bit::MAY_FLY | ability_bit::FLYING | ability_bit::NO_CLIP;
    let mut base = layer(base_bits, base_bits);
    base.layer_type = 1;
    let spectator = AbilityLayerEvidence {
        layer_type: SPECTATOR_LAYER_TYPE,
        abilities: ability_bit::FLYING,
        values: ability_bit::FLYING,
        fly_speed_bits: 0,
        vertical_fly_speed_bits: 0,
        walk_speed_bits: 0,
    };
    let abilities = AbilitiesUpdate {
        actor_unique_id: 0,
        player_permission: 1,
        command_permission: 0,
        layers: AbilityLayersEvidence::Received([base, spectator].into()),
    };
    let caps = GameModeCapabilities::resolve(Survival, Some(&abilities));
    assert!(caps.can_fly && caps.flight.is_flying());
    assert!(!caps.has_collision, "NoClip ability removes collision");
    assert!(
        caps.flight == FlightAuthority::Fixed(true),
        "a higher Flying grant remains fixed"
    );
}

/// Defined higher-layer values remain fixed, including explicit false values.
#[test]
fn higher_flight_authority_retains_value_and_definition() {
    for kind in super::SPECTATOR_LAYER_TYPE..6 {
        for flying in [false, true] {
            let mut higher = layer(
                ability_bit::FLYING,
                if flying { ability_bit::FLYING } else { 0 },
            );
            higher.layer_type = kind;
            assert_eq!(
                GameModeCapabilities::resolve(Survival, Some(&update(vec![higher]))).flight,
                FlightAuthority::Fixed(flying)
            );
            higher.abilities = 0;
            assert_eq!(
                GameModeCapabilities::resolve(Survival, Some(&update(vec![higher]))).flight,
                FlightAuthority::Mutable(false)
            );
        }
    }
    let mut grant = layer(ability_bit::FLYING, ability_bit::FLYING);
    grant.layer_type = super::SPECTATOR_LAYER_TYPE;
    let deny = protocol::AbilityLayerEvidence { values: 0, ..grant };
    assert_eq!(
        GameModeCapabilities::resolve(Survival, Some(&update(vec![grant, deny]))).flight,
        FlightAuthority::Fixed(false)
    );
}

/// Replacing a spectator layer can remove its Flying definition completely.
#[test]
fn spectator_layer_replacement_can_remove_flying_definition() {
    let mut grant = layer(ability_bit::FLYING, ability_bit::FLYING);
    grant.layer_type = super::SPECTATOR_LAYER_TYPE;
    let replacement = AbilityLayerEvidence {
        abilities: 0,
        ..grant
    };
    let abilities = update(vec![grant, replacement]);
    let caps = GameModeCapabilities::resolve(Survival, Some(&abilities));
    assert!(
        !caps.flight.is_flying(),
        "the effective layer no longer defines Flying"
    );
    assert!(
        matches!(caps.flight, FlightAuthority::Mutable(_)),
        "a replaced grant must release its flight authority"
    );
}

/// The native creative route is selected by game type, not Instabuild.
#[test]
fn instant_destruction_follows_mode_even_when_instabuild_disagrees() {
    for values in [0, ability_bit::INSTANT_BUILD] {
        let abilities = update(vec![layer(ability_bit::INSTANT_BUILD, values)]);
        for mode in [Survival, Adventure, Spectator, Unknown, Creative] {
            assert_eq!(
                GameModeCapabilities::resolve(mode, Some(&abilities)).instant_break,
                mode == Creative,
                "mode={mode:?}, Instabuild={values}"
            );
        }
    }
}

/// Layers apply in received order; the last one that defines a bit wins.
#[test]
fn later_layers_override_earlier_ones() {
    let layers = update(vec![
        layer(ability_bit::MAY_FLY, ability_bit::MAY_FLY),
        layer(ability_bit::MAY_FLY, 0),
    ]);
    assert!(!GameModeCapabilities::resolve(Survival, Some(&layers)).can_fly);
}

#[test]
fn review_independent_block_uses_survive_revoked_target_abilities() {
    let caps = GameModeCapabilities {
        can_build: false,
        can_use_switches: false,
        can_open_containers: false,
        ..GameModeCapabilities::for_mode(Survival)
    };
    assert!(caps.can_use_blocks());
    assert!(!GameModeCapabilities::for_mode(Spectator).can_use_blocks());
}

#[test]
fn ability_axes_higher_flying_denial_wins() {
    for kind in 3..=5 {
        for reverse in [false, true] {
            let mut spectator = layer(ability_bit::FLYING, ability_bit::FLYING);
            spectator.layer_type = super::SPECTATOR_LAYER_TYPE;
            let mut deny = layer(ability_bit::FLYING, 0);
            deny.layer_type = kind;
            let mut layers = vec![
                layer(
                    ability_bit::MAY_FLY | ability_bit::NO_CLIP,
                    ability_bit::MAY_FLY,
                ),
                spectator,
                deny,
            ];
            if reverse {
                layers.reverse();
            }
            let caps = GameModeCapabilities::resolve(Survival, Some(&update(layers)));
            assert!(!caps.flight.is_flying());
            assert!(caps.has_collision);
            assert!(
                caps.flight == FlightAuthority::Fixed(false),
                "higher Flying denial must remain authoritative: layer {kind}"
            );
        }
    }
}
