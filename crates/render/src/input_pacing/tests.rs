use super::*;
use bevy::app::SubApp;
use std::sync::atomic::{AtomicU64, Ordering};

type Event = Box<dyn FnOnce() + Send>;

/// Advances only when the simulated frame works or waits, firing scheduled events on the way.
struct FakeClock {
    base: Instant,
    nanos: AtomicU64,
    events: Mutex<Vec<(Instant, Event)>>,
}

impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            nanos: AtomicU64::new(0),
            events: Mutex::default(),
        })
    }

    fn schedule(&self, at: Instant, event: impl FnOnce() + Send + 'static) {
        self.events.lock().unwrap().push((at, Box::new(event)));
    }

    fn next_event(&self, until: Instant) -> Option<Instant> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .map(|(at, _)| *at)
            .filter(|at| *at <= until)
            .min()
    }

    fn set(&self, time: Instant) {
        let target = u64::try_from(time.saturating_duration_since(self.base).as_nanos()).unwrap();
        self.nanos.fetch_max(target, Ordering::SeqCst);
    }

    fn advance(&self, by: Duration) {
        self.advance_to(self.now() + by);
    }

    fn advance_to(&self, time: Instant) {
        while let Some(at) = self.next_event(time) {
            self.set(at);
            let event = {
                let mut events = self.events.lock().unwrap();
                let index = events.iter().position(|(when, _)| *when == at).unwrap();
                events.swap_remove(index).1
            };
            event();
        }
        self.set(time);
    }
}

impl PacingClock for FakeClock {
    fn now(&self) -> Instant {
        self.base + Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
    }

    fn wait_until(&self, deadline: Instant, _precise: bool, done: &mut dyn FnMut() -> bool) {
        while !done() {
            match self.next_event(deadline) {
                Some(at) => self.advance_to(at),
                None => return self.advance_to(deadline),
            }
        }
    }
}

/// Recorded main-thread durations per update.
#[derive(Resource)]
struct Script {
    main: Vec<Duration>,
}

/// The look input sampled this update; its value encodes when it was read.
#[derive(Resource, Default)]
struct Look {
    update: usize,
    sampled_at: Option<Instant>,
}

#[derive(Clone, Copy)]
struct Extracted {
    update: usize,
    sampled_at: Instant,
    extracted_at: Instant,
    /// When the frame handed off in this update finished rendering.
    rendered_at: Instant,
}

struct Run {
    extracted: Vec<Extracted>,
    main: Vec<Duration>,
}

#[derive(Resource)]
struct Clock(Arc<FakeClock>);

fn sample_look(clock: Res<Clock>, mut look: ResMut<Look>) {
    look.update += 1;
    look.sampled_at = Some(clock.0.now());
}

fn simulate_main_work(clock: Res<Clock>, look: Res<Look>, script: Res<Script>) {
    clock.0.advance(script.main[look.update - 1]);
}

/// Runs the recorded frames through a simulated render thread with the pacer on or off.
fn run(main: &[Duration], render: &[Duration], enabled: bool) -> Run {
    run_paced(main, render, enabled, FramePacing::default())
}

/// As [`run`], admitting frames at `pacing`'s cadence.
fn run_paced(main: &[Duration], render: &[Duration], enabled: bool, pacing: FramePacing) -> Run {
    let clock = FakeClock::new();
    let pacer = InputPacer::new(clock.clone(), enabled);
    let extracted = Arc::new(Mutex::new(Vec::new()));
    let mut app = App::new();
    app.insert_resource(Clock(clock.clone()))
        .insert_resource(Script {
            main: main.to_vec(),
        })
        .init_resource::<Look>()
        .insert_resource(pacing)
        .add_systems(PreUpdate, sample_look)
        .add_systems(Update, simulate_main_work);
    let mut render_thread = SubApp::new();
    let (sink, handoff_clock, handoff_pacer, durations) = (
        extracted.clone(),
        clock.clone(),
        pacer.clone(),
        render.to_vec(),
    );
    let mut in_flight: Option<Instant> = None;
    render_thread.set_extract(move |main: &mut World, _: &mut World| {
        // Bevy's handoff blocks until the previous frame finishes rendering.
        if let Some(done) = in_flight.take() {
            handoff_clock.advance_to(done);
        }
        let look = main.resource::<Look>();
        let now = handoff_clock.now();
        let rendered_at = now + durations[look.update - 1];
        sink.lock().unwrap().push(Extracted {
            update: look.update,
            sampled_at: look.sampled_at.unwrap(),
            extracted_at: now,
            rendered_at,
        });
        handoff_pacer.render_started();
        let finished = handoff_pacer.clone();
        handoff_clock.schedule(rendered_at, move || finished.render_finished());
        in_flight = Some(rendered_at);
    });
    app.insert_sub_app(RenderExtractApp, render_thread);
    app.add_plugins(InputPacingPlugin::with_pacer(pacer));
    for _ in main {
        app.update();
    }
    let extracted = extracted.lock().unwrap().clone();
    Run {
        extracted,
        main: main.to_vec(),
    }
}

fn ms(value: f64) -> Duration {
    Duration::from_secs_f64(value / 1_000.0)
}

fn age(frame: &Extracted) -> Duration {
    frame.extracted_at - frame.sampled_at
}

/// After history fills, the steady part of a run.
fn settled(run: &Run) -> impl Iterator<Item = (usize, &Extracted)> {
    run.extracted.iter().enumerate().skip(MIN_SAMPLES + 2)
}

#[test]
fn render_bound_frames_sample_input_one_margin_before_the_handoff() {
    let main = vec![ms(1.66); 64];
    let render = vec![ms(8.2); 64];
    let paced = run(&main, &render, true);
    let unpaced = run(&main, &render, false);
    for (index, frame) in settled(&paced) {
        assert_eq!(
            frame.update,
            index + 1,
            "look reaches its own update's extraction"
        );
        assert!(
            age(frame) <= paced.main[index] + MARGIN,
            "frame {index}: input aged {:?}",
            age(frame)
        );
        // The render thread never idles waiting for a late update.
        let interval = frame.extracted_at - paced.extracted[index - 1].extracted_at;
        assert_eq!(interval, render[index - 1], "frame {index}");
    }
    for (index, frame) in settled(&unpaced) {
        assert!(age(frame) > unpaced.main[index] + MARGIN * 4);
    }
}

#[test]
fn main_bound_frames_never_wait() {
    let main = vec![ms(6.0); 48];
    let render = vec![ms(3.0); 48];
    let paced = run(&main, &render, true);
    for (index, frame) in settled(&paced) {
        assert_eq!(age(frame), main[index]);
        let interval = frame.extracted_at - paced.extracted[index - 1].extracted_at;
        assert_eq!(
            interval, main[index],
            "pacing added latency to frame {index}"
        );
    }
}

#[test]
fn recorded_jitter_keeps_input_fresh_without_costing_throughput() {
    // Main 1.4–2.0 ms and render 7.6–9.0 ms, from a fixed pseudo-random sequence.
    let mut seed = 0x2545_f491_u32;
    let mut next = |low: f64, high: f64| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        low + (high - low) * f64::from(seed >> 8) / f64::from(1u32 << 24)
    };
    let (main, render): (Vec<_>, Vec<_>) = (0..256)
        .map(|_| (ms(next(1.4, 2.0)), ms(next(7.6, 9.0))))
        .unzip();
    let paced = run(&main, &render, true);
    let unpaced = run(&main, &render, false);
    let mut excess: Vec<Duration> = settled(&paced)
        .map(|(index, frame)| age(frame).saturating_sub(paced.main[index]))
        .collect();
    excess.sort_unstable();
    // Jitter on either side spreads ages around one margin beyond main time.
    let median = excess[excess.len() / 2];
    assert!(median <= MARGIN + MARGIN / 10, "median excess {median:?}");
    assert!(*excess.last().unwrap() <= MARGIN * 2, "excess {excess:?}");
    let unpaced_median = {
        let mut ages: Vec<_> = settled(&unpaced).map(|(_, frame)| age(frame)).collect();
        ages.sort_unstable();
        ages[ages.len() / 2]
    };
    assert!(unpaced_median > ms(5.0));
    let span =
        |run: &Run| run.extracted.last().unwrap().extracted_at - run.extracted[0].extracted_at;
    assert!(
        span(&paced).as_secs_f64() <= span(&unpaced).as_secs_f64() * 1.01,
        "paced {:?} unpaced {:?}",
        span(&paced),
        span(&unpaced)
    );
}

#[test]
fn a_render_slowdown_ending_never_holds_input_past_completion() {
    let main = vec![ms(1.0); 64];
    let render: Vec<_> = (0..64)
        .map(|frame| if frame < 32 { ms(30.0) } else { ms(1.0) })
        .collect();
    let paced = run(&main, &render, true);
    for (index, pair) in paced.extracted.windows(2).enumerate().skip(32) {
        let resumed_by = pair[0].extracted_at.max(pair[0].rendered_at);
        assert!(
            pair[1].sampled_at <= resumed_by,
            "update {} waited {:?} past its handed frame's completion",
            index + 2,
            pair[1].sampled_at - resumed_by
        );
    }
}

fn hz(rate: u32) -> FramePacing {
    FramePacing {
        rate: FrameRate::from_hz(rate),
        precise: true,
    }
}

fn intervals(run: &Run) -> Vec<Duration> {
    run.extracted
        .windows(2)
        .map(|pair| pair[1].sampled_at - pair[0].sampled_at)
        .collect()
}

/// One frame per slot: input samples sit on the cadence however fast main and render work run.
#[test]
fn a_cadence_admits_exactly_one_frame_per_slot() {
    let main = vec![ms(1.0); 600];
    let render = vec![ms(1.5); 600];
    let paced = run_paced(&main, &render, true, hz(144));
    let period = FrameRate::from_hz(144).unwrap().period_nanos();
    for (index, interval) in intervals(&paced).into_iter().enumerate() {
        let nanos = interval.as_nanos() as u64;
        assert!(
            nanos == period || nanos == period + 1,
            "frame {index}: {interval:?}"
        );
    }
    let span = paced.extracted.last().unwrap().sampled_at - paced.extracted[0].sampled_at;
    let exact = Duration::from_nanos(1_000_000_000_000 * 599 / 144_000);
    assert!(
        span.abs_diff(exact) <= Duration::from_nanos(1),
        "drift {span:?}"
    );
}

/// A slow frame restarts the cadence instead of rendering missed slots back to back.
#[test]
fn an_overrun_restarts_the_cadence_without_a_catch_up_burst() {
    let mut main = vec![ms(1.0); 64];
    main[20] = ms(40.0);
    let render = vec![ms(1.0); 64];
    let paced = run_paced(&main, &render, true, hz(120));
    let period = Duration::from_nanos(FrameRate::from_hz(120).unwrap().period_nanos());
    for (index, interval) in intervals(&paced).into_iter().enumerate() {
        assert!(interval >= period / 2, "frame {index}: {interval:?}");
    }
    let resumed = intervals(&paced)[21];
    assert!(
        resumed.abs_diff(period) <= Duration::from_nanos(1),
        "{resumed:?}"
    );
}

/// A finished render never cuts a cadence slot short, and the pacing switch leaves the cap alone.
#[test]
fn render_completion_and_the_pacing_switch_never_shorten_the_cadence() {
    let main = vec![ms(0.5); 96];
    let render = vec![ms(0.5); 96];
    for enabled in [true, false] {
        let paced = run_paced(&main, &render, enabled, hz(60));
        let period = Duration::from_nanos(FrameRate::from_hz(60).unwrap().period_nanos());
        for interval in intervals(&paced) {
            assert!(interval >= period, "enabled={enabled}: {interval:?}");
        }
    }
}

/// Render-bound frames slower than the cadence keep the just-in-time sample.
#[test]
fn render_bound_frames_under_a_loose_cadence_still_sample_late() {
    let main = vec![ms(1.66); 64];
    let render = vec![ms(8.2); 64];
    let paced = run_paced(&main, &render, true, hz(60));
    for (index, frame) in settled(&paced) {
        assert!(
            age(frame) <= paced.main[index] + MARGIN + ms(0.01),
            "frame {index}: input aged {:?}",
            age(frame)
        );
    }
}
