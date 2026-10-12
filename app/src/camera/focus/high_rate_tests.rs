use bevy::{
    ecs::schedule::SingleThreadedExecutor,
    input::{
        InputPlugin,
        mouse::{AccumulatedMouseMotion, MouseMotion},
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused},
};
use test_alloc::{Allocations, measure};

const EVENTS_PER_SECOND: usize = 8_000;
const FRAMES_PER_SECOND: usize = 60;
const MAX_EVENTS_PER_FRAME: usize = EVENTS_PER_SECOND.div_ceil(FRAMES_PER_SECOND);
const DELTA: Vec2 = Vec2::new(0.125, -0.25);

#[derive(Resource, Default)]
struct Samples {
    input_passes: usize,
    updates: usize,
    delta: Vec2,
}

/// Counts input schedule passes independently of the gameplay sample.
fn count_input_pass(mut samples: ResMut<Samples>) {
    samples.input_passes += 1;
}

/// Observes the motion available to gameplay once the input and focus systems have run.
fn sample_motion(motion: Res<AccumulatedMouseMotion>, mut samples: ResMut<Samples>) {
    samples.updates += 1;
    samples.delta = motion.delta;
}

/// Installs production accumulation and focus arbitration without native windows or clocks.
fn input_app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .init_resource::<Samples>()
        .add_systems(PreUpdate, count_input_pass)
        .add_systems(Update, sample_motion);
    super::install(&mut app);
    // Keep allocation accounting on the measured thread, including Bevy's input systems.
    app.edit_schedule(Main, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    app.edit_schedule(First, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    app.edit_schedule(PreUpdate, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    app.edit_schedule(Update, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    app.edit_schedule(PostUpdate, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    let window = app
        .world_mut()
        .spawn((
            Window::default(),
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

/// Writes at the typed-message boundary used by Bevy's native input forwarding.
fn write_motion(app: &mut App, count: usize, delta: Vec2) -> Allocations {
    measure(|| {
        for _ in 0..count {
            app.world_mut().write_message(MouseMotion { delta });
        }
    })
    .1
}

/// Warms both rotating message buffers and the schedules at the largest simulated batch.
fn warm(app: &mut App) {
    for _ in 0..4 {
        write_motion(app, MAX_EVENTS_PER_FRAME, DELTA);
        app.update();
    }
    *app.world_mut().resource_mut::<Samples>() = Samples::default();
}

#[test]
fn high_rate_motion_has_bounded_allocation_growth_and_one_sample_per_frame() {
    let (mut app, _) = input_app();
    app.update();
    let cold = write_motion(&mut app, MAX_EVENTS_PER_FRAME, DELTA);
    // Message buffers grow geometrically rather than allocating once for each event.
    assert!(cold.calls <= 16, "cold message allocations: {cold:?}");
    warm(&mut app);
    // Compare high-rate batches with an ordinary input frame on the same populated path.
    assert_eq!(write_motion(&mut app, 1, DELTA), Allocations::default());
    let baseline = measure(|| app.update()).1;
    let initial_updates = app.world().resource::<Samples>().updates;
    let initial_input_passes = app.world().resource::<Samples>().input_passes;
    let mut events = 0;
    let mut total = Vec2::ZERO;
    for frame in 0..FRAMES_PER_SECOND {
        let count = (frame + 1) * EVENTS_PER_SECOND / FRAMES_PER_SECOND
            - frame * EVENTS_PER_SECOND / FRAMES_PER_SECOND;
        let delta = if frame % 2 == 0 { DELTA } else { -DELTA };
        let before = app.world().resource::<Samples>().updates;
        let input_before = app.world().resource::<Samples>().input_passes;
        // Native callbacks enqueue messages; they do not execute the app's input schedules.
        for _ in 0..count {
            assert_eq!(write_motion(&mut app, 1, delta), Allocations::default());
            assert_eq!(app.world().resource::<Samples>().updates, before);
            assert_eq!(app.world().resource::<Samples>().input_passes, input_before);
        }
        let frame_allocations = measure(|| app.update()).1;
        assert_eq!(frame_allocations, baseline);
        let samples = app.world().resource::<Samples>();
        assert_eq!(samples.updates, before + 1);
        assert_eq!(samples.input_passes, input_before + 1);
        assert_eq!(samples.delta, delta * count as f32);
        total += samples.delta;
        events += count;
    }
    assert_eq!(events, EVENTS_PER_SECOND);
    assert_eq!(
        app.world().resource::<Samples>().updates - initial_updates,
        FRAMES_PER_SECOND
    );
    assert_eq!(total, Vec2::ZERO);
    assert_eq!(
        app.world().resource::<Samples>().input_passes - initial_input_passes,
        FRAMES_PER_SECOND
    );
    app.update();
    assert_eq!(app.world().resource::<Samples>().delta, Vec2::ZERO);
    eprintln!(
        "mouse input: events={events}, frames={FRAMES_PER_SECOND}, cold={cold:?}, warmed event allocations=0, frame={baseline:?}"
    );
}

/// Prints a warmed throughput observation without making timing a correctness gate.
#[cfg(feature = "reports")]
#[test]
fn high_rate_motion_timing_report() {
    let (mut app, _) = input_app();
    warm(&mut app);
    let started = std::time::Instant::now();
    let (_, allocations) = measure(|| {
        for frame in 0..FRAMES_PER_SECOND {
            let count = (frame + 1) * EVENTS_PER_SECOND / FRAMES_PER_SECOND
                - frame * EVENTS_PER_SECOND / FRAMES_PER_SECOND;
            for _ in 0..count {
                app.world_mut().write_message(MouseMotion { delta: DELTA });
            }
            app.update();
            std::hint::black_box(app.world().resource::<Samples>().delta);
        }
    });
    let elapsed = started.elapsed();
    assert_eq!(app.world().resource::<Samples>().updates, FRAMES_PER_SECOND);
    eprintln!(
        "mouse input throughput: {EVENTS_PER_SECOND} events, {FRAMES_PER_SECOND} frames, {elapsed:?}, {allocations:?}"
    );
}

#[test]
fn high_rate_motion_is_discarded_on_focus_loss_and_not_replayed() {
    let (mut app, window) = input_app();
    warm(&mut app);
    write_motion(&mut app, MAX_EVENTS_PER_FRAME, DELTA);
    app.world_mut().write_message(WindowFocused {
        window,
        focused: false,
    });
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_eq!(app.world().resource::<Samples>().delta, Vec2::ZERO);
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
    write_motion(&mut app, MAX_EVENTS_PER_FRAME, DELTA);
    app.update();
    assert_eq!(app.world().resource::<Samples>().delta, Vec2::ZERO);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut().write_message(WindowFocused {
        window,
        focused: true,
    });
    app.update();
    assert_eq!(app.world().resource::<Samples>().delta, Vec2::ZERO);
}
