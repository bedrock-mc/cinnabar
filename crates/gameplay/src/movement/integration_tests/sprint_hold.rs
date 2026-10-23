use super::*;

/// A server reset followed by either correction path preserves the held sprint request.
#[test]
fn held_sprint_resumes_after_metadata_and_position_corrections() {
    for toggle_sprint in [false, true] {
        for stamp in [0, 102] {
            for mode in [
                PhysicsCorrectionMode::Snap,
                PhysicsCorrectionMode::ReplayIfRetained,
            ] {
                let (mut physics, mut ticker) = walked_physics(0);
                let mut locals = super::super::LocomotionState::default();
                let mut effects = super::super::LocalMovementEffectTimeline::default();
                let mut speed = super::super::LocalMovementSpeedAuthority::default();
                let held = semantic_input::ActionPhase {
                    held: true,
                    ..Default::default()
                };
                for pressed in [true, false, false, false] {
                    assert!(locals.advance(
                        super::super::PhysicsFrameInput {
                            toggle_sprint,
                            ..super::speed_frame(semantic_input::ActionPhase { pressed, ..held })
                        },
                        &mut physics,
                        &mut ticker,
                        &mut effects,
                        &mut speed,
                        &VersionedFloor(1),
                    ));
                }
                let position = physics.sample_at(102).unwrap().position;
                if let Some(rewind) = physics.apply_server_movement_flags(
                    stamp,
                    flags(|flags| flags.sprinting = Some(false)),
                ) {
                    reconcile_timeline_rewind(
                        &mut ticker,
                        &mut physics,
                        rewind,
                        &VersionedFloor(1),
                    )
                    .unwrap();
                }
                reconcile_candidate_physics_correction(
                    &mut ticker,
                    &mut physics,
                    position,
                    102,
                    true,
                    mode,
                    &VersionedFloor(1),
                )
                .unwrap();
                let tick = physics.state().unwrap().tick + 1;
                let mut frame = super::speed_frame(held);
                frame.toggle_sprint = toggle_sprint;
                assert!(locals.advance(
                    frame,
                    &mut physics,
                    &mut ticker,
                    &mut effects,
                    &mut speed,
                    &VersionedFloor(1),
                ));
                let sample = physics.sample_at(tick).unwrap();
                assert!(
                    sample.input.sprint_down,
                    "toggle {toggle_sprint}, stamp {stamp}, mode {mode:?}"
                );
                assert!(sample.processed.sprinting);
                let packet = ticker
                    .pending_snapshots()
                    .into_iter()
                    .find(|packet| packet.tick == tick)
                    .unwrap();
                let held_flags = PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING;
                assert_eq!(packet.flags.bits() & held_flags.bits(), held_flags.bits());
                assert_eq!(
                    packet.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
                    0
                );
            }
        }
    }
}
