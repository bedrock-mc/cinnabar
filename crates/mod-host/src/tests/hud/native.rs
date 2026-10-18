use super::*;
use crate::module_observations::ModuleObservations;

/// Loads an extension that publishes native inventory, compass and bounds policies.
fn native_host() -> (tempfile::TempDir, ModHost) {
    let content = serde_json::json!({
        "cards": [
            {"id":"inventory", "rows":[], "source": {
                "kind":{"kind":"inventory", "hotbar":true},
                "limit":mod_api::PLAYER_STATE_INVENTORY_SLOTS
            }},
            {"id":"compass", "rows":[], "width":ui::mod_hud::COMPASS_DEFAULT_WIDTH,
                "compass":{"session":1}}
        ],
        "actor_bounds":{"players":true, "mobs":false, "range":32, "color":[1,1,1,1]}
    })
    .to_string();
    super::super::load_with_view_closed(
        source(&content, "", "", "call $cards", false)
            .replace("(func (export \"init\") )", "(func (export \"init\"))"),
        ModGrants {
            hud: true,
            player_state: true,
            render: true,
            players: true,
            ..Default::default()
        },
        "",
    )
}

/// Supplies current player facts while keeping the observed actor in its original session.
fn native_frame(host: &mut ModHost, session: u64, focused: bool, slot_known: bool, yaw: f32) {
    let world = WorldObservation {
        session,
        dimension: 0,
        world: "fixture".into(),
        position: (0., 0., 0.),
        alive: true,
    };
    host.observe_presentation(Some(world), None, None, Vec::new())
        .unwrap();
    host.observe_modules(ModuleObservations {
        motion: Some(MotionObservation {
            session,
            dimension: 0,
            yaw,
            pitch: 0.,
            velocity: (0., 0., 0.),
            on_ground: true,
        }),
        actors: vec![crate::ObservedActor {
            actor: ActorObservation {
                session: 1,
                dimension: 0,
                runtime_id: 2,
                identifier: String::new(),
                name: "Actor".into(),
                position: (1., 0., 0.),
                yaw: 0.,
                pitch: 0.,
                player: true,
                invisible: false,
                sneaking: false,
                hurt: false,
                pickable: true,
                hittable: None,
                look_origin: None,
                look_direction: None,
                health: None,
                boxes: vec![ActorObservationBounds {
                    min: (0., 0., 0.),
                    max: (1., 2., 1.),
                }],
            },
            max_health: None,
        }],
        ..Default::default()
    })
    .unwrap();
    let unknown = PlayerStateSlot {
        known: false,
        item: None,
    };
    let mut inventory = vec![unknown.clone(); mod_api::PLAYER_STATE_INVENTORY_SLOTS];
    inventory[0].known = slot_known;
    host.frame_with_player_state(
        false,
        Some(GameplaySnapshot {
            session,
            dimension: 0,
            eye: GameplayVector3 {
                x: 0.,
                y: 1.,
                z: 0.,
            },
            yaw: 0.,
            pitch: 0.,
            attack_held: false,
            frame_seconds: 0.016,
            players: Vec::new(),
        }),
        Vec::new(),
        Some(PlayerStateSnapshot {
            session,
            dimension: 0,
            selected_slot: None,
            inventory,
            armor: vec![unknown.clone(); mod_api::PLAYER_STATE_ARMOR_SLOTS],
            offhand: unknown,
            effects: Vec::new(),
        }),
        ControlFrame {
            focused,
            gameplay: true,
            ..empty_controls()
        },
    )
    .unwrap();
}

/// Both event-lane and frame-lane deferral refresh facts and revoke unfocused outlines.
#[test]
fn deferred_native_presentation_tracks_current_observations() {
    for lifecycle in [false, true] {
        let (_directory, mut host) = native_host();
        native_frame(&mut host, 1, true, false, 180.);
        assert_eq!(host.hud().unwrap().cards[0].rows[0].value, "?");
        let heading = host.hud().unwrap().cards[1]
            .compass
            .as_ref()
            .unwrap()
            .frame
            .as_ref()
            .unwrap()
            .heading;
        assert_eq!(host.render().0.primitives.ribbons.len(), 1);
        let budget = if lifecycle {
            super::super::defer_view_closed(&mut host)
        } else {
            let budget = CallbackBudget::new(0);
            host.set_frame_authority(budget.clone(), false, false, 0, HudClaims::default());
            budget
        };
        let remaining = budget.remaining();
        native_frame(&mut host, 1, false, true, 90.);
        assert!(host.is_active());
        assert_eq!(host.last_fuel_used(), 0);
        assert_eq!(budget.remaining(), remaining);
        assert_eq!(host.hud().unwrap().cards[0].rows[0].value, "");
        assert_ne!(
            host.hud().unwrap().cards[1]
                .compass
                .as_ref()
                .unwrap()
                .frame
                .as_ref()
                .unwrap()
                .heading,
            heading
        );
        assert!(host.render().0.primitives.ribbons.is_empty());
        native_frame(&mut host, 2, true, false, 0.);
        assert!(
            host.hud().unwrap().cards[1]
                .compass
                .as_ref()
                .unwrap()
                .frame
                .is_none()
        );
        assert!(host.render().0.primitives.ribbons.is_empty());
        host.frame(false).unwrap();
        assert!(host.hud().unwrap().cards[0].rows.is_empty());
        assert!(host.render().0.primitives.ribbons.is_empty());
    }
}
