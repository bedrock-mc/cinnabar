use super::*;
use bevy::prelude::{Res, ResMut, Resource};
use client_runtime::network::NetworkHandle;
use gameplay::melee::{ActorHit, Crosshair, MeleeRuntime, PressContext, SwingTracker};

#[derive(Resource, Default)]
struct Attack {
    runtime: MeleeRuntime,
    swings: SwingTracker,
    admitted_at: Option<std::time::Duration>,
}

#[derive(Resource)]
struct Transport {
    packets: client_session::CapturedPackets,
    bytes: Vec<Vec<Vec<u8>>>,
}

/// Resolves finalized physical attack input against a fixed, reachable actor.
fn admit_attack(
    input: Res<SemanticInputSnapshot>,
    network: Res<NetworkHandle<()>>,
    time: Res<Time<Real>>,
    mut attack: ResMut<Attack>,
) {
    let phase = input.phase(Action::Attack);
    if !attack.runtime.observe_input(phase.pressed, phase.held) {
        return;
    }
    let stack = protocol::NetworkItemStack::empty();
    let press = PressContext {
        refused_destroy: None,
        tick: 100,
        player_position: [0.0, 1.62, 0.0],
        input_mode: protocol::PlayerInputMode::Mouse,
        local_runtime_id: 42,
        selection: Some(gameplay::mining::FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: actor_animation::ACTOR_SWING_TICKS,
        now_millis: time.elapsed().as_millis().try_into().unwrap(),
        item_attack: None,
    };
    let Attack {
        runtime,
        swings,
        admitted_at,
    } = &mut *attack;
    gameplay::melee::resolve_and_send(
        runtime,
        swings,
        Crosshair::Actor(ActorHit {
            runtime_id: 7,
            distance: 2.0,
            point: [0.0, 1.62, -2.0],
        }),
        &press,
        press.now_millis,
        |packets| {
            network.send_inventory_packets(packets)?;
            *admitted_at = Some(time.elapsed());
            Ok(())
        },
    );
}

/// Collects complete submitted frames before the next frame's admission wait.
fn capture_submission(mut transport: ResMut<Transport>) {
    while let Some(frame) = transport.packets.take_flushed_frame() {
        transport.bytes.push(
            frame
                .iter()
                .map(|packet| protocol::encode(packet).unwrap().to_vec())
                .collect(),
        );
    }
}

#[test]
fn physical_attack_submits_bytes_in_its_input_frame_without_a_physics_tick() {
    let mut app = gameplay_app(false);
    let (network, packets) = NetworkHandle::<()>::stub_capturing_packets();
    app.insert_resource(network)
        .init_resource::<Attack>()
        .insert_resource(Transport {
            packets,
            bytes: Vec::new(),
        })
        .add_systems(Update, admit_attack.in_set(ClientFrameSet::Interaction))
        .add_systems(render::FrameEnd, capture_submission);
    client_runtime::network::configure_network_frame_flush::<()>(&mut app);
    app.update();
    let at = std::time::Duration::from_millis(1_007);
    app.world_mut().resource_mut::<Time<Real>>().advance_by(at);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(app.world().resource::<Attack>().admitted_at, Some(at));
    let transport = app.world().resource::<Transport>();
    assert_eq!(transport.bytes.len(), 1);
    assert_eq!(transport.bytes[0].len(), 2);
    assert!(transport.bytes[0].iter().all(|packet| !packet.is_empty()));
    assert!(!app.world().resource::<Attack>().runtime.press_pending());
}
