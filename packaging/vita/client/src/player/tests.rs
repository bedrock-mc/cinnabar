use super::*;
use crate::controls::{CROSS, PadSample, VitaInput};
use semantic_input::InputContext;
use sim::{Aabb, CollisionQuery, WorldQueryError};

struct Floor;
impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64., 0., -64.), Vec3::new(64., 1., 64.));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

fn grounded() -> PlayerController {
    let mut state = PlayerState::new(Vec3::new(8., 1., 8.));
    state.on_ground = true;
    PlayerController::new(state)
}

#[test]
fn right_stick_down_aims_below_the_horizon_and_up_aims_above() {
    for (right_y, expected_sign) in [(255, -1.), (0, 1.)] {
        let mut input = VitaInput::default();
        let mut controller = grounded();
        let frame = input
            .sample(
                PadSample {
                    right_y,
                    ..Default::default()
                },
                0,
                InputContext::Gameplay,
            )
            .unwrap();
        controller
            .advance(&frame.actions, TICK_DURATION, true, 120., &Floor)
            .unwrap();
        let [pitch, yaw] = controller.rotation();
        assert_eq!(sim::view_direction(pitch, yaw).y.signum(), expected_sign);
        // A point along the same aiming ray must project to the centre of the view.
        let eye = controller.eye(1.5);
        let ahead = eye + sim::view_direction(pitch, yaw) * 4.;
        let matrix = controller.view_matrix(1.5);
        let camera = std::array::from_fn::<_, 3, _>(|row| {
            matrix[row] * ahead.x as f32
                + matrix[row + 4] * ahead.y as f32
                + matrix[row + 8] * ahead.z as f32
                + matrix[row + 12]
        });
        assert!(camera[0].abs() < 0.001 && camera[1].abs() < 0.001);
        assert!(camera[2] < -3.99);
    }
}

#[test]
fn render_cadence_does_not_change_fixed_tick_travel() {
    let run = |dt| {
        let mut controller = grounded();
        let mut input = VitaInput::default();
        for n in 0..(1_000_000 / dt) {
            let frame = input
                .sample(
                    PadSample {
                        left_y: 0,
                        ..Default::default()
                    },
                    n * dt,
                    InputContext::Gameplay,
                )
                .unwrap();
            controller
                .advance(
                    &frame.actions,
                    Duration::from_micros(dt),
                    true,
                    120.,
                    &Floor,
                )
                .unwrap();
        }
        controller.state
    };
    let slow = run(50_000);
    assert_eq!(slow, run(10_000));
    assert_eq!(slow.tick, 20);
    assert!(slow.position.z > 11. && slow.position.y == 1.);
}

#[test]
fn quick_jump_between_ticks_is_retained_and_menu_clears_it() {
    let mut input = VitaInput::default();
    let mut controller = grounded();
    let pressed = input
        .sample(
            PadSample {
                buttons: CROSS,
                ..Default::default()
            },
            0,
            InputContext::Gameplay,
        )
        .unwrap();
    controller
        .advance(
            &pressed.actions,
            Duration::from_millis(10),
            true,
            120.,
            &Floor,
        )
        .unwrap();
    let released = input
        .sample(PadSample::default(), 10_000, InputContext::Gameplay)
        .unwrap();
    controller
        .advance(
            &released.actions,
            Duration::from_millis(40),
            true,
            120.,
            &Floor,
        )
        .unwrap();
    assert!(controller.state.position.y > 1.);

    let mut paused = grounded();
    paused
        .advance(
            &pressed.actions,
            Duration::from_millis(10),
            true,
            120.,
            &Floor,
        )
        .unwrap();
    paused
        .advance(
            &released.actions,
            Duration::from_secs(30),
            false,
            120.,
            &Floor,
        )
        .unwrap();
    assert_eq!(
        paused
            .advance(&released.actions, TICK_DURATION, true, 120., &Floor)
            .unwrap(),
        1
    );
    assert_eq!(paused.state.position.y, 1.);
}

#[test]
fn authority_release_clears_pending_motion_and_frame_hitches_are_bounded() {
    let mut input = VitaInput::default();
    let mut controller = grounded();
    let moving = input
        .sample(
            PadSample {
                left_y: 0,
                ..Default::default()
            },
            0,
            InputContext::Gameplay,
        )
        .unwrap();
    assert_eq!(
        controller
            .advance(&moving.actions, Duration::from_secs(5), true, 120., &Floor)
            .unwrap(),
        (MAX_FRAME_ELAPSED.as_nanos() / TICK_DURATION.as_nanos()) as u32
    );
    let before = controller.state.clone();
    let menu = input
        .sample(
            PadSample {
                left_y: 0,
                ..Default::default()
            },
            1,
            InputContext::UiFocused,
        )
        .unwrap();
    controller
        .advance(&menu.actions, TICK_DURATION, true, 120., &Floor)
        .unwrap();
    assert_eq!(controller.state, before);
}
