use super::*;
use bevy::prelude::{Last, ResMut, Update};

#[derive(Resource)]
struct Submission {
    captured: client_session::CapturedPackets,
    frames: Vec<Vec<Vec<u8>>>,
}

/// Queues an early frame action through the production session FIFO.
fn queue_action(network: Res<NetworkHandle<()>>) {
    network
        .send_inventory_packet(protocol::modal_form_cancel_response(1))
        .unwrap();
}

/// Adds a late action that must remain in the same frame batch.
fn queue_late_action(network: Res<NetworkHandle<()>>) {
    network
        .send_inventory_packet(protocol::modal_form_cancel_response(2))
        .unwrap();
}

/// Observes submitted bytes at the point where the next frame may start waiting.
fn observe_before_wait(mut submission: ResMut<Submission>) {
    let frame = submission
        .captured
        .take_flushed_frame()
        .expect("the current frame must be submitted before its pacing wait");
    submission.frames.push(
        frame
            .iter()
            .map(|packet| protocol::encode(packet).unwrap().to_vec())
            .collect(),
    );
    assert!(submission.captured.take_flushed_frame().is_none());
}

#[test]
fn current_frame_is_submitted_before_pacing_in_either_install_order() {
    for pacing_first in [false, true] {
        let mut app = App::new();
        if pacing_first {
            render::FrameEnd::install(&mut app);
        }
        configure_network_frame_flush::<()>(&mut app);
        render::FrameEnd::install(&mut app);
        let (network, captured) = NetworkHandle::<()>::stub_capturing_packets();
        app.insert_resource(network)
            .insert_resource(Submission {
                captured,
                frames: Vec::new(),
            })
            .add_systems(Update, queue_action)
            .add_systems(Last, queue_late_action)
            .add_systems(render::FrameEnd, observe_before_wait);
        app.update();
        let expected: Vec<_> = [1, 2]
            .into_iter()
            .map(|id| {
                protocol::encode(&protocol::modal_form_cancel_response(id))
                    .unwrap()
                    .to_vec()
            })
            .collect();
        assert_eq!(app.world().resource::<Submission>().frames, [expected]);
    }
}

/// A frame counts as presented only when presenting took a swap-chain texture held before
/// rendering; no surface, or a present the renderer skipped, is not a presented frame.
#[test]
fn presentation_needs_a_released_swap_chain() {
    assert!(super::presented(1, 0));
    assert!(!super::presented(0, 0), "no window held a surface");
    assert!(!super::presented(1, 1), "the renderer skipped presenting");
}
