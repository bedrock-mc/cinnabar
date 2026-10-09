use super::*;
use bevy::time::{TimePlugin, TimeReceiver, TimeSender, TimeSystems, create_time_channels};

#[derive(Resource)]
struct RenderClock {
    sender: TimeSender,
    pending: TimeReceiver,
}

#[derive(Resource, Default)]
struct TickSends {
    frames: Vec<Instant>,
    pending: Vec<Instant>,
    sent: Vec<(Instant, Instant)>,
}

/// Supplies deterministic fallback time when the simulated renderer has not returned a stamp.
fn supply_fallback(clock: Res<Clock>, renderer: Res<RenderClock>) {
    if renderer.pending.0.is_empty() {
        renderer.sender.0.try_send(clock.0.now()).unwrap();
    }
}

/// Records frame admission separately from tick production and transport.
fn admit_frame(clock: Res<Clock>, mut sends: ResMut<TickSends>) {
    sends.frames.push(clock.0.now());
}

/// Records each fixed simulation tick before its frame's transport flush.
fn tick(clock: Res<Clock>, mut sends: ResMut<TickSends>) {
    sends.pending.push(clock.0.now());
}

/// Models one millisecond of input and packet preparation, then an immediately runnable writer.
fn flush(clock: Res<Clock>, mut sends: ResMut<TickSends>) {
    clock.0.advance(ms(1.0));
    let at = clock.0.now();
    let pending = std::mem::take(&mut sends.pending);
    sends
        .sent
        .extend(pending.into_iter().map(|tick| (tick, at)));
}

/// Runs actual frame admission and Bevy time with a renderer blocked by FIFO presentation.
fn cadence_run(refresh: Option<u32>) -> Vec<(Instant, Instant)> {
    let clock = FakeClock::new();
    clock.advance(ms(0.1));
    let pacer = InputPacer::new(clock.clone(), true);
    let (sender, receiver) = create_time_channels();
    let render_sender = sender.0.clone();
    let render_clock = RenderClock {
        sender,
        pending: TimeReceiver(receiver.0.clone()),
    };
    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .insert_resource(Time::<Fixed>::from_hz(20.0))
        .insert_resource(receiver)
        .insert_resource(render_clock)
        .insert_resource(Clock(clock.clone()))
        .init_resource::<TickSends>()
        .add_systems(First, supply_fallback.before(TimeSystems))
        .add_systems(PreUpdate, admit_frame)
        .add_systems(FixedUpdate, tick)
        .add_systems(Update, flush);
    let mut render_thread = SubApp::new();
    let handoff_clock = clock.clone();
    let handoff_pacer = pacer.clone();
    let mut in_flight = None;
    let mut frame = 0;
    render_thread.set_extract(move |_: &mut World, _: &mut World| {
        if let Some(done) = in_flight.take() {
            handoff_clock.advance_to(done);
        }
        frame += 1;
        let now = handoff_clock.now();
        let done = if let Some(hz) = refresh {
            let elapsed = (now - handoff_clock.base).as_nanos();
            let slot = (elapsed + 1) * u128::from(hz) / 1_000_000_000 + 1;
            let vblank = handoff_clock.base
                + Duration::from_nanos((slot * 1_000_000_000 / u128::from(hz)) as u64);
            vblank
                + if frame % 5 < 2 {
                    ms(0.2)
                } else {
                    Duration::ZERO
                }
        } else {
            now + ms(2.0)
        };
        handoff_pacer.render_started();
        let finished = handoff_pacer.clone();
        let sender = render_sender.clone();
        handoff_clock.schedule(done, move || {
            sender.try_send(done).unwrap();
            finished.render_finished();
        });
        in_flight = Some(done);
    });
    app.insert_sub_app(RenderExtractApp, render_thread);
    app.add_plugins(InputPacingPlugin::with_pacer(pacer));
    for _ in 0..360 {
        app.update();
    }
    let mut sends = app.world_mut().resource_mut::<TickSends>();
    let period = refresh.map_or(ms(2.0), |hz| Duration::from_secs_f64(1.0 / f64::from(hz)));
    for pair in sends.frames[32..].windows(2) {
        let interval = pair[1] - pair[0];
        assert!(
            interval.abs_diff(period) <= ms(0.5),
            "frame admission: {interval:?}"
        );
    }
    sends.sent.split_off(0)
}

/// Checks both prompt flushing and even movement arrival after the prediction history settles.
fn assert_cadence(refresh: Option<u32>) {
    let sends = cadence_run(refresh);
    let sends = &sends[10..];
    let intervals: Vec<_> = sends.windows(2).map(|pair| pair[1].1 - pair[0].1).collect();
    let min = intervals.iter().min().unwrap();
    let max = intervals.iter().max().unwrap();
    eprintln!("refresh={refresh:?}: send intervals {min:?}..{max:?}");
    for &(tick, sent) in sends {
        assert!(sent - tick <= ms(3.0), "tick to flush: {:?}", sent - tick);
    }
    for interval in intervals {
        assert!(
            interval.abs_diff(ms(50.0)) <= ms(3.0),
            "refresh={refresh:?}: {interval:?}"
        );
    }
}

#[test]
fn fifo_60_hz_keeps_tick_sends_even() {
    assert_cadence(Some(60));
}

#[test]
fn fifo_120_hz_keeps_tick_sends_even() {
    assert_cadence(Some(120));
}

#[test]
fn uncapped_rendering_keeps_tick_sends_even() {
    assert_cadence(None);
}

/// Builds a clock-only app whose render stamps can be controlled independently of frame time.
fn clock_app(clock: Arc<FakeClock>) -> (App, TimeSender) {
    let (sender, receiver) = create_time_channels();
    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .insert_resource(Time::<Fixed>::from_hz(20.0))
        .insert_resource(receiver)
        .add_plugins(InputPacingPlugin::with_pacer(InputPacer::new(clock, true)));
    (app, sender)
}

#[test]
fn simulation_time_samples_the_current_frame_and_drains_render_stamps() {
    let clock = FakeClock::new();
    let (mut app, sender) = clock_app(clock.clone());
    sender.0.try_send(clock.now()).unwrap();
    app.update();
    clock.advance(ms(50.0));
    sender.0.try_send(clock.base + ms(1.0)).unwrap();
    sender.0.try_send(clock.base + ms(2.0)).unwrap();
    app.update();
    assert_eq!(app.world().resource::<Time<Real>>().delta(), ms(50.0));
    assert!(sender.0.is_empty(), "all render stamps must be consumed");
    sender.0.try_send(clock.now()).unwrap();
    sender.0.try_send(clock.now()).unwrap();
}

#[test]
fn manual_recording_clocks_keep_their_requested_steps() {
    use bevy::time::TimeUpdateStrategy;

    for mode in 0..3 {
        let clock = FakeClock::new();
        let (mut app, _sender) = clock_app(clock.clone());
        let step = ms(17.0);
        for frame in 0..4 {
            clock.advance(ms(3.0));
            app.insert_resource(match mode {
                0 => TimeUpdateStrategy::ManualDuration(step),
                1 => TimeUpdateStrategy::ManualInstant(clock.base + step * frame),
                _ => TimeUpdateStrategy::FixedTimesteps(1),
            });
            app.update();
            let expected = if mode == 2 { ms(50.0) } else { step } * frame;
            assert_eq!(app.world().resource::<Time<Real>>().elapsed(), expected);
        }
    }
}
