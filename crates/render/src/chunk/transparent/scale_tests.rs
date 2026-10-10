//! Water at render-distance scale: an ocean of shores past the hard ref ceiling, turned in
//! place, crossed sub-chunk by sub-chunk and streamed through, with each stage's CPU cost.
use super::transparent_strafe::{Fixture, STAGES, SURFACE_SUBCHUNK_Y, drawn_water, fixture_sized};
use super::*;

/// Every sub-chunk is a 272-face shore, so the 101x101 ocean holds more refs than one
/// snapshot can.
const RADIUS: i32 = 50;
const SHORE_FACES: usize = 272;

/// Reports frame-phase costs and missed visible water, including arena uploads deferred to later passes.
#[derive(Default)]
struct PhaseReport {
    frames: usize,
    missing_frames: usize,
    jobs: usize,
    sorted_refs: usize,
    requests: u64,
    upload_bytes: u64,
    deferred_uploads: usize,
    most_retired: usize,
    upload_pass: std::time::Duration,
}

/// Reads the fixture sort counters after a measured frame phase.
fn metrics(fixture: &Fixture) -> TransparentSortMetricsSnapshot {
    fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot()
}

/// Runs `frames` frames, each first streaming what `stream` returns and then drawing from
/// the camera `view` gives, and prints the phase's transparent stage costs.
fn run_phase(
    fixture: &mut Fixture,
    label: &str,
    frames: usize,
    mut stream: impl FnMut(usize) -> (Vec<SubChunkKey>, Vec<(SubChunkKey, bool)>),
    mut view: impl FnMut(usize) -> (Vec3, Vec3),
) -> PhaseReport {
    let profiler = fixture
        .app
        .world()
        .resource::<RuntimeStageProfiler>()
        .clone();
    let _ = profiler.take_snapshot_if_due(std::time::Duration::ZERO);
    let before = metrics(fixture);
    let (jobs, sorted_refs) = (fixture.jobs, fixture.sorted_refs);
    let mut report = PhaseReport {
        frames,
        ..default()
    };
    for frame in 0..frames {
        let (out, into) = stream(frame);
        let started = std::time::Instant::now();
        report.deferred_uploads += into.len() - fixture.stream(&out, &into);
        report.upload_pass += started.elapsed();
        let (camera, forward) = view(frame);
        fixture.frame_looking(camera, forward);
        if drawn_water(fixture)
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != fixture.visible_water(camera, forward)
        {
            report.missing_frames += 1;
        }
        fixture.complete_gpu_frame();
        report.most_retired = report.most_retired.max(
            fixture
                .app
                .world()
                .resource::<ChunkGpuArena>()
                .retired_allocations
                .len(),
        );
    }
    let after = metrics(fixture);
    report.jobs = fixture.jobs - jobs;
    report.sorted_refs = fixture.sorted_refs - sorted_refs;
    report.requests = after.request_generation - before.request_generation;
    report.upload_bytes = after.upload_bytes - before.upload_bytes;
    let snapshot = profiler
        .take_snapshot_if_due(std::time::Duration::ZERO)
        .unwrap();
    for stage in STAGES {
        let sample = snapshot.samples[stage as usize];
        println!(
            "{label} {}: count={} total={:.3}ms mean={:.1}us max={:.1}us",
            stage.name(),
            sample.count,
            sample.total.as_secs_f64() * 1e3,
            sample.total.as_secs_f64() * 1e6 / sample.count.max(1) as f64,
            sample.maximum.as_secs_f64() * 1e6,
        );
    }
    println!(
        "{label}: frames={} frames_missing_visible_water={} sort_requests={} sort_jobs={} \
         sorted_refs={} upload_bytes={} deferred_uploads={} most_retired={} \
         upload_pass_total={:.3}ms committed_refs={} ceiling_rejects={}",
        report.frames,
        report.missing_frames,
        report.requests,
        report.jobs,
        report.sorted_refs,
        report.upload_bytes,
        report.deferred_uploads,
        report.most_retired,
        report.upload_pass.as_secs_f64() * 1e3,
        after.ref_count,
        after.ceiling_reject_count,
    );
    report
}

/// An ocean past the ref ceiling stays fully drawn while the camera turns, crosses
/// sub-chunks and streams rows of water in and out, and turning sorts and uploads nothing.
#[test]
fn an_ocean_past_the_ref_ceiling_turns_crosses_and_streams() {
    let mut fixture = fixture_sized(RADIUS, 1);
    let shores = fixture.surfaces.len();
    assert!(shores * SHORE_FACES > MAX_TRANSPARENT_DRAW_REFS);
    let still = |_| (Vec::new(), Vec::new());
    let centre = Vec3::new(8.5, 64.6, 8.5);
    run_phase(&mut fixture, "settle", 40, still, |_| (centre, Vec3::Z));

    let turn = run_phase(&mut fixture, "turn", 64, still, |frame| {
        let yaw = frame as f32 * std::f32::consts::TAU / 16.0;
        (centre, Vec3::new(yaw.sin(), 0.0, yaw.cos()))
    });
    assert_eq!(
        turn.missing_frames, 0,
        "visible water was missing while turning"
    );
    assert_eq!(
        (turn.requests, turn.jobs, turn.upload_bytes),
        (0, 0, 0),
        "turning in place sorted or uploaded water"
    );

    // Four blocks a frame crosses into a new sub-chunk every four frames.
    let cross = run_phase(&mut fixture, "cross", 64, still, |frame| {
        (centre + Vec3::X * 4.0 * frame as f32, Vec3::Z)
    });
    assert_eq!(
        cross.missing_frames, 0,
        "visible water was missing while crossing"
    );

    // One row of shores streams in ahead and one out behind every fourth frame.
    let row =
        |z: i32| (-RADIUS..=RADIUS).map(move |x| SubChunkKey::new(0, x, SURFACE_SUBCHUNK_Y, z));
    let start = centre + Vec3::X * 256.0;
    let streaming = run_phase(
        &mut fixture,
        "stream",
        64,
        |frame| {
            if frame % 4 != 0 {
                return (Vec::new(), Vec::new());
            }
            let step = frame as i32 / 4;
            (
                row(step - RADIUS).collect(),
                row(step + RADIUS + 1).map(|key| (key, true)).collect(),
            )
        },
        |frame| (start + Vec3::Z * 4.0 * frame as f32, Vec3::Z),
    );
    assert_eq!(
        streaming.missing_frames, 0,
        "visible water was missing while streaming"
    );
    let arena = fixture.app.world().resource::<ChunkGpuArena>();
    assert!(
        fixture
            .surfaces
            .iter()
            .all(|(entity, _)| arena.allocations.contains_key(entity)),
        "streamed water never uploaded"
    );
    // Each row is released before the next one streams out.
    assert!(
        streaming.most_retired <= 2 * RADIUS as usize + 1,
        "{} removed sub-chunks were held at once",
        streaming.most_retired
    );
}
