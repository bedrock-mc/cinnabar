//! Local damage reaches UI preparation after the production actor tick.
use super::*;
use protocol::{HudEvent, PlayerStatus, UiEvent, WorldEvent};
use std::time::Duration;

#[derive(Resource, Default)]
struct DamageAtUiPreparation(Option<client_world::ActorDamageState>);

/// Captures the damage snapshot consumed by this frame's UI preparation.
fn observe_damage(ui: Res<UiRuntime>, mut damage: ResMut<DamageAtUiPreparation>) {
    damage.0 = ui.local_actor_damage();
}

#[test]
fn actor_damage_publication_advances_before_ui_and_clears_with_the_stream() {
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    crate::app::configure_actor_render_systems(&mut app);
    app.add_systems(
        Update,
        observe_damage.in_set(crate::app::ClientFrameSet::UiPreparation),
    );
    let mut schedule = app
        .world_mut()
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .remove(Update)
        .unwrap();
    let mut world = custom_emotes::fixture();
    world.init_resource::<DamageAtUiPreparation>();
    world.init_resource::<render::ActorRenderFrame>();
    world.init_resource::<render::ActorRuntimeWitness>();
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(
                3,
                WorldEvent::Ui(UiEvent::Hud(HudEvent::PlayerStatus(
                    PlayerStatus::PlayerSpawn,
                ))),
            )
            .unwrap();
        stream
            .submit(
                4,
                WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 7 })),
            )
            .unwrap();
    }
    schedule.run(&mut world);
    let initial = world.resource::<DamageAtUiPreparation>().0.unwrap();
    assert_eq!(initial.previous_health, client_world::DEFAULT_PLAYER_HEALTH);
    assert!(initial.flash_active());
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(50));
    schedule.run(&mut world);
    let next = world.resource::<DamageAtUiPreparation>().0.unwrap();
    assert_eq!(next.remaining_ticks, initial.remaining_ticks - 1);
    assert!(!next.flash_active());
    world.resource_mut::<ClientWorld>().stream = None;
    schedule.run(&mut world);
    assert_eq!(world.resource::<DamageAtUiPreparation>().0, None);
}

#[test]
fn hud_health_uses_the_actors_attribute_range_after_a_health_packet() {
    let mut world = custom_emotes::fixture();
    world
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            3,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 40 })),
        )
        .unwrap();
    world.resource_scope(|world, mut ui: Mut<UiRuntime>| {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        ui.apply(
            &mut player,
            client_ui::ui_runtime::SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 3,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 40 }),
            },
        )
        .unwrap();
    });
    world.run_system_cached(publish_local_actor_damage).unwrap();
    let actor_health = &world
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor(1)
        .unwrap()
        .attributes["minecraft:health"];
    let displayed = world.resource::<UiRuntime>().hud().health().unwrap();
    assert_eq!(
        f32::from(displayed.current()) / f32::from(displayed.scale()),
        actor_health.current
    );
    assert_eq!(
        f32::from(displayed.maximum()) / f32::from(displayed.scale()),
        actor_health.max
    );
}
