//! Current-frame interaction output from the explicitly selected personal component.

use bevy::prelude::Resource;

/// The host validates outputs before publishing them; absence keeps vanilla reach.
#[derive(Resource, Debug, Default)]
pub struct ModInteraction {
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
}

/// Uses the validated personal override for both actor picking and attack admission.
pub fn effective_reach(default: f64, interaction: Option<&ModInteraction>) -> f64 {
    interaction
        .and_then(|interaction| interaction.attack_reach)
        .filter(|reach| reach.is_finite() && *reach >= 0.0)
        .map_or(default, f64::from)
}

/// Actor reach may extend wall observation, but cannot shorten ordinary block picking.
pub fn block_pick_reach(default: f64, actor_reach: f64) -> f64 {
    default.max(actor_reach)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use client_world::{
        ActorSnapshot, WorldAuthority, game_mode_capabilities::SURVIVAL_ATTACK_REACH,
    };
    use gameplay::melee::{Crosshair, classify, pick_actor};

    use super::*;

    fn player(distance: f32) -> ActorSnapshot {
        let position = [0.0, 0.0, -distance];
        let mut authority = WorldAuthority::new(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 0,
                local_player_unique_id: 0,
                player_position: position,
                world_spawn_position: [0; 3],
                air_network_id: protocol::air_network_id(false),
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            position,
            None,
        );
        let spawn = protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 9,
            runtime_id: 9,
            kind: protocol::ActorKind::Player {
                uuid: [0; 16],
                username: "remote".into(),
            },
            position,
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        };
        authority
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
                Some(1),
            )
            .unwrap();
        authority.actor(9).expect("remote player admitted").clone()
    }

    fn crosshair(
        actor: &ActorSnapshot,
        interaction: Option<&ModInteraction>,
        block_distance: Option<f64>,
    ) -> Crosshair {
        let actor = pick_actor(
            std::iter::once(actor),
            None,
            [0.0, 1.62, 0.0],
            [0.0, 0.0, -1.0],
            effective_reach(
                gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse),
                interaction,
            ),
        );
        classify(
            actor,
            block_distance,
            effective_reach(SURVIVAL_ATTACK_REACH, interaction),
        )
    }

    #[test]
    fn absent_or_disabled_override_keeps_vanilla_attack_admission() {
        let player = player(4.0);
        assert_eq!(crosshair(&player, None, None), Crosshair::Miss);
        assert_eq!(
            crosshair(&player, Some(&ModInteraction::default()), None),
            Crosshair::Miss
        );
    }

    #[test]
    fn personal_reach_admits_distant_actor_but_keeps_wall_occlusion() {
        let interaction = ModInteraction {
            attack_reach: Some(5.0),
            ..Default::default()
        };
        let target = player(4.0);
        assert!(matches!(
            crosshair(&target, Some(&interaction), None),
            Crosshair::Actor(hit) if hit.runtime_id == target.runtime_id
        ));
        assert_eq!(
            crosshair(&target, Some(&interaction), Some(2.0)),
            Crosshair::Block
        );
        assert_eq!(
            crosshair(&player(5.8), Some(&interaction), None),
            Crosshair::Miss
        );
    }

    #[test]
    fn personal_reach_extends_actor_pick_without_skipping_distant_wall() {
        let interaction = ModInteraction {
            attack_reach: Some(6.0),
            ..Default::default()
        };
        let target = player(6.25);
        assert_eq!(crosshair(&target, None, None), Crosshair::Miss);
        assert!(matches!(
            crosshair(&target, Some(&interaction), None),
            Crosshair::Actor(_)
        ));
        assert_eq!(
            crosshair(&target, Some(&interaction), Some(5.75)),
            Crosshair::Block
        );
    }

    #[test]
    fn shorter_actor_reach_keeps_distant_blocks_available_for_mining() {
        let interaction = ModInteraction {
            attack_reach: Some(SURVIVAL_ATTACK_REACH as f32),
            ..Default::default()
        };
        let vanilla_pick = gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse);
        let actor_pick = effective_reach(vanilla_pick, Some(&interaction));
        let block_distance = 4.0;
        let observed = (block_distance <= block_pick_reach(vanilla_pick, actor_pick))
            .then_some(block_distance);
        let crosshair = classify(None, observed, SURVIVAL_ATTACK_REACH);
        assert_eq!(crosshair, Crosshair::Block);
        let mut runtime = gameplay::melee::MeleeRuntime::default();
        runtime.observe_crosshair(crosshair);
        assert!(
            !runtime.actor_in_front(),
            "no actor may veto mining this block"
        );
    }

    #[test]
    fn invalid_adapter_value_cannot_widen_reach() {
        for reach in [f32::NAN, f32::INFINITY, -1.0] {
            let interaction = ModInteraction {
                attack_reach: Some(reach),
                ..Default::default()
            };
            assert_eq!(
                effective_reach(SURVIVAL_ATTACK_REACH, Some(&interaction)),
                SURVIVAL_ATTACK_REACH
            );
        }
    }

    #[test]
    fn zero_reach_remains_an_explicit_override() {
        let interaction = ModInteraction {
            attack_reach: Some(0.0),
            ..Default::default()
        };
        assert_eq!(
            effective_reach(SURVIVAL_ATTACK_REACH, Some(&interaction)),
            0.0
        );
        assert_eq!(
            crosshair(&player(2.0), Some(&interaction), None),
            Crosshair::Miss
        );
    }
}
