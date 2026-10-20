use super::*;
use bevy::render::MainWorld;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Advances only when fixture work executes, so the timer never depends on wall time.
#[derive(Resource, Clone, Default)]
struct WorkClock(Arc<AtomicU64>);

/// Models the blocking drawable acquisition with a deterministic twenty-millisecond wait.
fn acquire(clock: Res<WorkClock>) {
    clock.0.fetch_add(20, Ordering::Relaxed);
}

/// Models attachment or view-target preparation that must stay in CPU feeding work.
fn prepare_targets(clock: Res<WorkClock>) {
    clock.0.fetch_add(10, Ordering::Relaxed);
}

#[test]
fn latency_review_keeps_unrelated_view_preparation_in_cpu_work() {
    use bevy::ecs::schedule::SingleThreadedExecutor;
    for preparation_first in [true, false] {
        let mut world = World::new();
        let adaptive = AdaptiveFrameLatency::default();
        let epoch = adaptive.epoch;
        let clock = WorkClock::default();
        let timer = clock.clone();
        world.insert_resource(clock);
        world.insert_resource(adaptive);
        world.insert_resource(PresentModePolicy::default());
        let mut schedule = Schedule::default();
        schedule.set_executor(SingleThreadedExecutor::new());
        // Preparation is permitted as soon as acquisition returns.
        if preparation_first {
            schedule.add_systems(prepare_targets.after(acquire));
        }
        schedule.add_systems(measure_acquisition(acquire, move || {
            epoch + Duration::from_millis(timer.0.load(Ordering::Relaxed))
        }));
        if !preparation_first {
            schedule.add_systems(prepare_targets.after(acquire));
        }
        schedule.run(&mut world);
        let mut adaptive = world.resource_mut::<AdaptiveFrameLatency>();
        adaptive.frame = 1;
        adaptive.started = Some(epoch);
        adaptive.finish(epoch + Duration::from_millis(35));
        assert_eq!(adaptive.cpu[1].unwrap().render, Duration::from_millis(15));
    }
}

#[test]
fn latency_cadence_partial_gpu_samples_leave_headroom_qualification_running() {
    let (mut app, _) = fixture();
    let policy = app.world().resource::<PresentModePolicy>().clone();
    let mut adaptive = app.world_mut().resource_mut::<AdaptiveFrameLatency>();
    for frame in 1..=250 {
        let at = Duration::from_micros(frame * 16_667);
        let index = frame as usize % adaptive.cpu.len();
        adaptive.cpu[index] = Some(FrameWorkSample {
            frame,
            at,
            render: Duration::from_millis(4),
            gpu: Duration::ZERO,
        });
        let now = adaptive.epoch + at;
        adaptive.record_gpu(
            frame,
            (frame % 2 == 1).then_some(Duration::from_millis(5)),
            now,
            &policy,
        );
    }
    assert_eq!(policy.frame_latency.latency.load(Ordering::Acquire), 1);
}

/// Creates fixed-refresh context and a shared recommendation without a native surface.
fn fixture() -> (SubApp, Entity) {
    let mut main = MainWorld::default();
    main.init_resource::<Messages<WindowOccluded>>();
    let window = main.spawn((Window::default(), PrimaryWindow)).id();
    let mut app = SubApp::new();
    let policy = PresentModePolicy::default();
    policy.publish_display_timing(DisplayTiming {
        refresh: render_model::FrameRate::from_hz(60),
        vrr: VrrStatus::Inactive,
    });
    app.insert_resource(policy).insert_resource(main);
    install(&mut app);
    app.world_mut().run_schedule(ExtractSchedule);
    (app, window)
}

#[test]
fn acquisition_and_surface_recreation_do_not_become_render_work() {
    let mut adaptive = AdaptiveFrameLatency::default();
    let epoch = adaptive.epoch;
    adaptive.frame = 1;
    adaptive.started = Some(epoch);
    adaptive.surface_elapsed = Duration::from_millis(20);
    adaptive.finish(epoch + Duration::from_millis(25));
    assert_eq!(adaptive.cpu[1].unwrap().render, Duration::from_millis(5));
    assert_eq!(adaptive.frame(), 1);

    adaptive.frame = 2;
    adaptive.started = Some(epoch);
    adaptive.discard_current_frame();
    adaptive.finish(epoch + Duration::from_secs(1));
    assert!(adaptive.cpu[2].is_none());
}

#[test]
fn resize_and_focus_changes_discard_old_context_and_pending_readbacks() {
    let (mut app, window) = fixture();
    let policy = app.world().resource::<PresentModePolicy>().clone();
    for resize in [true, false] {
        policy.publish_frame_latency(std::num::NonZeroU32::MIN);
        app.world_mut().resource_mut::<AdaptiveFrameLatency>().cpu[1] = Some(FrameWorkSample {
            frame: 1,
            ..Default::default()
        });
        app.world_mut().run_schedule(ExtractSchedule);
        assert!(app.world().resource::<AdaptiveFrameLatency>().cpu[1].is_some());
        {
            let mut main = app.world_mut().resource_mut::<MainWorld>();
            let mut window = main.get_mut::<Window>(window).unwrap();
            if resize {
                window.resolution.set_physical_resolution(640, 480);
            } else {
                window.focused = false;
            }
        }
        app.world_mut().run_schedule(ExtractSchedule);
        let mut adaptive = app.world_mut().resource_mut::<AdaptiveFrameLatency>();
        assert!(adaptive.cpu.iter().all(Option::is_none));
        let now = adaptive.epoch;
        adaptive.record_gpu(1, Some(Duration::from_millis(2)), now, &policy);
        assert_eq!(
            policy
                .frame_latency
                .latency
                .load(std::sync::atomic::Ordering::Acquire),
            2
        );
    }
}

#[test]
fn matched_gpu_readbacks_qualify_headroom_but_old_or_partial_ones_do_not() {
    let (mut app, _) = fixture();
    let policy = app.world().resource::<PresentModePolicy>().clone();
    let mut adaptive = app.world_mut().resource_mut::<AdaptiveFrameLatency>();
    for frame in 1..=121 {
        let at = Duration::from_micros(frame * 16_667);
        let index = frame as usize % adaptive.cpu.len();
        adaptive.cpu[index] = Some(FrameWorkSample {
            frame,
            at,
            render: Duration::from_millis(4),
            gpu: Duration::ZERO,
        });
        let now = adaptive.epoch + at;
        adaptive.record_gpu(frame + 1, Some(Duration::from_millis(5)), now, &policy);
        assert!(adaptive.cpu[index].is_some());
        adaptive.record_gpu(frame, Some(Duration::from_millis(5)), now, &policy);
        assert!(adaptive.cpu[index].is_none());
    }
    assert_eq!(
        policy
            .frame_latency
            .latency
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    let context = adaptive.context.as_mut().unwrap();
    context.display.vrr = VrrStatus::Active;
    assert!(adaptive.refresh().is_none());
}

/// Runs real preparation while supplying a deterministic, coarse render/GPU estimate.
pub(crate) fn assert_preparation_qualifies(mut prepare: impl FnMut()) {
    let (mut app, _) = fixture();
    let policy = app.world().resource::<PresentModePolicy>().clone();
    let mut adaptive = app.world_mut().resource_mut::<AdaptiveFrameLatency>();
    for frame in 1..=150 {
        let at = Duration::from_micros(frame * 16_667);
        adaptive.frame = frame;
        adaptive.started = Some(adaptive.epoch + at);
        prepare();
        let now = adaptive.epoch + at + Duration::from_millis(4);
        adaptive.finish(now);
        adaptive.record_gpu(frame, Some(Duration::from_millis(5)), now, &policy);
    }
    assert_eq!(
        policy.frame_latency.latency.load(Ordering::Acquire),
        1,
        "routine preparation uploads must allow a low-latency probe"
    );
}

#[test]
fn latency_cadence_lower_caps_opt_out_and_refresh_rate_caps_remain_eligible() {
    let (mut app, _) = fixture();
    for (hz, eligible) in [(30, false), (60, true), (120, true)] {
        app.world_mut()
            .resource_mut::<MainWorld>()
            .insert_resource(crate::FramePacing {
                rate: render_model::FrameRate::from_hz(hz),
                ..Default::default()
            });
        app.world_mut().run_schedule(ExtractSchedule);
        assert_eq!(
            app.world()
                .resource::<AdaptiveFrameLatency>()
                .refresh()
                .is_some(),
            eligible
        );
    }
}

#[test]
fn latency_cadence_counts_only_the_depth_the_surface_has_adopted() {
    let (mut app, _) = fixture();
    let policy = app.world().resource::<PresentModePolicy>().clone();
    let mut adaptive = app.world_mut().resource_mut::<AdaptiveFrameLatency>();
    for frame in 1..=121 {
        let at = Duration::from_micros(frame * 16_667);
        let index = frame as usize % adaptive.cpu.len();
        adaptive.cpu[index] = Some(FrameWorkSample {
            frame,
            at,
            render: Duration::from_millis(4),
            gpu: Duration::ZERO,
        });
        let now = adaptive.epoch + at;
        adaptive.record_gpu(frame, Some(Duration::from_millis(5)), now, &policy);
    }
    let mut now = adaptive.epoch + Duration::from_secs(3);
    let two = std::num::NonZeroU32::new(2);
    for _ in 0..10 {
        now += Duration::from_millis(33);
        adaptive.record_acquisition(now, two, &policy);
        assert_eq!(policy.frame_latency.latency.load(Ordering::Acquire), 1);
    }
    adaptive.discard_current_frame();
    for index in 0..9 {
        now += Duration::from_millis(33);
        adaptive.record_acquisition(now, Some(std::num::NonZeroU32::MIN), &policy);
        assert_eq!(
            policy.frame_latency.latency.load(Ordering::Acquire),
            if index == 8 { 2 } else { 1 }
        );
    }
}
