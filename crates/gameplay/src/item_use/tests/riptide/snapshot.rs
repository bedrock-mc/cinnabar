//! A deferred release owns its original item, eligibility and effective facing until consumed.

use super::*;

#[test]
fn riptide_snapshot_survives_retry_and_releases_the_original_item_and_facing_once() {
    for charge in [RIPTIDE_CHARGE_TICKS - 1, RIPTIDE_CHARGE_TICKS] {
        for assisted in [false, true] {
            let mut runtime = ItemUseRuntime::default();
            let mut swings = SwingTracker::default();
            runtime.observe_press(true);
            runtime.step(&trident(100, true, 3, true));
            let first = RiptideFacing {
                rotation: [-25.0, 90.0],
                assisted,
            };
            let edge = UseFrame {
                riptide_facing: first,
                ..trident(100 + charge, false, 3, true)
            };
            step_and_send(&mut runtime, &mut swings, &edge, 7, 6, |_| {
                Err(BatchSendError::Full)
            });
            assert_eq!(runtime.riptide_release_facing(), Some(first));
            assert_eq!(runtime.take_riptide_launch(), None);
            let later = UseFrame {
                riptide_facing: RiptideFacing {
                    rotation: [40.0, -90.0],
                    assisted: !assisted,
                },
                selection: Some(stack(2, MENU_ITEM, 2)),
                ..trident(160, false, 1, false)
            };
            for _ in 0..2 {
                step_and_send(&mut runtime, &mut swings, &later, 7, 6, |_| {
                    Err(BatchSendError::Full)
                });
                assert_eq!(runtime.riptide_release_facing(), Some(first));
                assert_eq!(runtime.take_riptide_launch(), None);
            }
            let mut sent = Vec::new();
            step_and_send(&mut runtime, &mut swings, &later, 7, 6, |packets| {
                sent = packets;
                Ok(())
            });
            assert_eq!(sent.len(), 1);
            let transaction =
                protocol::test_support::transactions::observe_transaction(&sent[0]).unwrap();
            let release = transaction.release().unwrap();
            assert_eq!((release.slot, release.item.count), (2, 1));
            assert_eq!(runtime.riptide_release_facing(), None);
            assert_eq!(
                runtime.take_riptide_launch(),
                (charge == RIPTIDE_CHARGE_TICKS).then_some(RiptideLaunch {
                    level: 3,
                    rotation: first.rotation
                })
            );
            assert_eq!(runtime.take_riptide_launch(), None);
            step_and_send(&mut runtime, &mut swings, &later, 7, 6, |_| {
                panic!("a consumed release cannot be sent twice")
            });
        }
    }
}

#[test]
fn riptide_snapshot_is_cleared_by_reselection_respawn_session_and_closed_transport() {
    for cancellation in ["selection", "respawn", "session", "closed"] {
        let mut runtime = ItemUseRuntime::default();
        let mut swings = SwingTracker::default();
        runtime.synchronize(7);
        runtime.observe_press(true);
        runtime.step(&trident(100, true, 3, true));
        let edge = UseFrame {
            riptide_facing: RiptideFacing {
                rotation: [30.0, 90.0],
                assisted: true,
            },
            ..trident(110, false, 3, true)
        };
        step_and_send(&mut runtime, &mut swings, &edge, 7, 6, |_| {
            Err(BatchSendError::Full)
        });
        assert!(runtime.riptide_release_facing().is_some());
        match cancellation {
            "selection" => {
                let changed = UseFrame {
                    selection: Some(selection(3, MENU_ITEM)),
                    ..edge.clone()
                };
                assert!(runtime.step(&changed).packets.is_empty());
            }
            "respawn" => runtime.cancel_for_respawn(),
            "session" => runtime.synchronize(8),
            _ => {
                step_and_send(&mut runtime, &mut swings, &edge, 7, 6, |_| {
                    Err(BatchSendError::Closed)
                });
            }
        }
        assert_eq!(runtime.riptide_release_facing(), None);
        assert_eq!(runtime.take_riptide_launch(), None);
        step_and_send(&mut runtime, &mut swings, &edge, 7, 6, |_| {
            panic!("cancelled release facts cannot be restored by a later frame")
        });
    }
}

#[test]
fn use_progress_leaves_reselected_riptide_release_cleanup_to_the_lifecycle() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_press(true);
    assert!(runtime.step(&trident(100, true, 3, true)).started);
    let edge = UseFrame {
        riptide_facing: RiptideFacing {
            rotation: [30.0, 90.0],
            assisted: false,
        },
        ..trident(110, false, 3, true)
    };
    step_and_send(&mut runtime, &mut swings, &edge, 7, 6, |_| {
        Err(BatchSendError::Full)
    });
    assert_eq!(runtime.riptide_release_facing(), Some(edge.riptide_facing));
    let changed = UseFrame {
        selection: Some(stack(3, MENU_ITEM, 2)),
        ..trident(111, false, 1, true)
    };
    runtime
        .send_progress_tick(
            changed.tick,
            7,
            changed.selection.as_ref(),
            Some(assets::facts::ItemUseAnimationFact::None),
            |_| -> Result<(), ()> { panic!("a trident has no consumption progress") },
        )
        .unwrap();
    step_and_send(&mut runtime, &mut swings, &changed, 7, 6, |_| {
        panic!("reselection cancels the old release")
    });
    assert!(!runtime.is_using());
    assert_eq!(runtime.riptide_release_facing(), None);
    assert_eq!(runtime.take_riptide_launch(), None);

    let next = UseFrame {
        tick: 130,
        now_millis: 6500,
        held: true,
        riptide_facing: RiptideFacing {
            rotation: [-20.0, -90.0],
            assisted: false,
        },
        ..changed
    };
    runtime.observe_press(true);
    assert!(runtime.step(&next).started);
    let released = runtime.step(&UseFrame {
        tick: 140,
        now_millis: 7000,
        held: false,
        ..next.clone()
    });
    assert_eq!(kinds(&released), ["release"]);
    let released = transaction(&released.packets[0]);
    let released = released.release().unwrap();
    assert_eq!((released.slot, released.item.count), (3, 2));
    assert_eq!(
        runtime.take_riptide_launch(),
        Some(RiptideLaunch {
            level: 1,
            rotation: next.riptide_facing.rotation,
        })
    );
}
