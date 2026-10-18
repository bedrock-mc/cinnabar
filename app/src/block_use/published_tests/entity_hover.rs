//! Hover reports share the retained attack pick and run at the local tick boundary.
use super::*;
use attack_progress_tests::{buttons, interaction_app, ticks};
use protocol::test_support::packets::entity_hover;

/// Creates the production interaction schedule with one previously published actor pick.
fn hover_app() -> (App, client_session::CapturedPackets) {
    let (mut world, captured) = attack_hit_position_tests::attack_fixture(7.5);
    buttons(&mut world, vec![]);
    (interaction_app(world), captured)
}

/// Advances one local tick and returns only its hover reports.
fn hover_tick(
    app: &mut App,
    captured: &mut client_session::CapturedPackets,
    tick: u64,
) -> Vec<(u64, Option<[f32; 3]>)> {
    ticks(app.world_mut(), tick, tick + 1);
    app.world_mut().run_schedule(Update);
    captured.drain().iter().filter_map(entity_hover).collect()
}

#[test]
fn entity_hover_new_actor_reports_the_retained_world_point_once() {
    let (mut app, mut captured) = hover_app();
    let point = attack_hit_position_tests::picked_point(app.world());
    assert_eq!(
        hover_tick(&mut app, &mut captured, 102),
        [(99, Some(point))]
    );
    assert!(hover_tick(&mut app, &mut captured, 103).is_empty());
}

#[test]
fn entity_hover_held_item_change_reports_the_same_actor_again() {
    let (mut app, mut captured) = hover_app();
    assert_eq!(hover_tick(&mut app, &mut captured, 102).len(), 1);
    item_use_tests::hold_snowball(app.world_mut());
    let point = attack_hit_position_tests::picked_point(app.world());
    assert_eq!(
        hover_tick(&mut app, &mut captured, 103),
        [(99, Some(point))]
    );
    // A different stack count does not change the held item identity.
    item_use_tests::hold_air_item(app.world_mut(), "minecraft:snowball", 8, [].into());
    assert!(hover_tick(&mut app, &mut captured, 104).is_empty());
}

#[test]
fn entity_hover_leaving_an_actor_clears_without_a_position() {
    let (mut app, mut captured) = hover_app();
    assert_eq!(hover_tick(&mut app, &mut captured, 102).len(), 1);
    {
        let mut origin = app.world_mut().resource_mut::<InteractionOriginSnapshot>();
        origin.assist_direction(Vec3::X);
    }
    // Publish the look-away frame; the next local tick reads that retained pick.
    app.world_mut().run_schedule(Update);
    captured.drain();
    assert_eq!(hover_tick(&mut app, &mut captured, 103), [(0, None)]);
    assert!(hover_tick(&mut app, &mut captured, 104).is_empty());
}

#[test]
fn entity_hover_follows_attack_and_swing_and_precedes_its_movement() {
    let (mut app, mut captured) = hover_app();
    let point = attack_hit_position_tests::picked_point(app.world());
    buttons(app.world_mut(), vec![1]);
    ticks(app.world_mut(), 102, 103);
    app.world_mut().run_schedule(Update);
    let packets = captured.drain();
    let attack = packets
        .iter()
        .position(|packet| {
            protocol::test_support::transactions::observe_transaction(packet)
                .is_some_and(|tx| tx.actor_use().is_some())
        })
        .unwrap();
    let swing = packets
        .iter()
        .position(|packet| {
            packet.kind() == protocol::swing_arm_packet(42, protocol::SwingSource::Attack).kind()
        })
        .unwrap();
    let hover = packets
        .iter()
        .position(|packet| entity_hover(packet).is_some())
        .unwrap();
    let movement = packets
        .iter()
        .position(|packet| protocol::player_auth_input_trace_sample(packet).is_some())
        .unwrap();
    assert!(attack < swing && swing < hover && hover < movement);
    assert_eq!(entity_hover(&packets[hover]), Some((99, Some(point))));
    assert_eq!(
        protocol::test_support::transactions::observe_transaction(&packets[attack])
            .unwrap()
            .actor_use()
            .unwrap()
            .hit_position,
        point
    );
}
