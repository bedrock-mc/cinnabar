use super::*;
use crate::{
    item_use::{AirUse, ItemUseRuntime, UseFrame},
    mining::FrozenMiningSelection,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Starts a verified thirty-two-tick held use in slot zero.
fn food_use() -> (ItemUseRuntime, UseFrame) {
    let extra_data: Arc<[u8]> = Arc::from([]);
    let stack = protocol::NetworkItemStack {
        network_id: 300,
        metadata: 0,
        stack_network_id: 41,
        count: 2,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id: 0,
        extra_data,
    };
    let frame = UseFrame {
        tick: 100,
        now_millis: 5000,
        position: [0.0; 3],
        held: true,
        selection: Some(FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        air_use: Some(AirUse::Hold {
            max_ticks: 32,
            needs: assets::item_use::ItemUseRequirement::Nothing,
            slowdown: 0.35,
            cooldown: None,
        }),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
        riptide: None,
        riptide_facing: Default::default(),
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(runtime.step(&frame).started);
    (runtime, frame)
}

/// Correction sync observes a progress cancellation, then a later independent server write.
#[test]
fn sync_reports_progress_reselection_and_later_server_use() {
    for animation in [
        assets::facts::ItemUseAnimationFact::Eat,
        assets::facts::ItemUseAnimationFact::Drink,
    ] {
        let (mut runtime, mut frame) = food_use();
        frame.selection.as_mut().unwrap().slot = 1;
        runtime
            .send_progress_tick(
                107,
                7,
                frame.selection.as_ref(),
                Some(animation),
                |_| -> Result<(), ()> { panic!("cancelled progress has no packet") },
            )
            .unwrap();
        let mut physics = due_physics();
        for using in [false, true] {
            if using {
                runtime.adopt_server_using_item(Some(true));
                runtime
                    .send_progress_tick(111, 7, frame.selection.as_ref(), Some(animation), |_| {
                        Ok::<_, ()>(())
                    })
                    .unwrap();
            }
            send_movement_prediction_sync(
                &mut physics,
                &local_actor(),
                None,
                None,
                runtime.reported_using_item(),
                |packet| {
                    let flags =
                        protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
                    assert_eq!(flag_set(&flags, ActorFlag::UsingItem), using);
                    false
                },
            );
        }
    }
}
