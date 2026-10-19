use super::*;

/// RAII timers report actual nested allocations on their calling thread and drain per frame.
#[test]
fn timers_count_nested_requests_without_allocating_themselves() {
    let profiler = RuntimeStageProfiler::new(true);
    let (_, overhead) = test_alloc::measure(|| drop(profiler.time(RuntimeStage::WorldPoll)));
    assert_eq!(overhead.calls, 0);
    {
        let _outer = profiler.time(RuntimeStage::MainFrame);
        let bytes = std::hint::black_box(vec![0_u8; 64]);
        {
            let _inner = profiler.time(RuntimeStage::UiPreparation);
            std::hint::black_box(vec![0_u8; 32]);
        }
        std::hint::black_box(bytes);
    }
    let frame = profiler.state.allocations.finish_frame();
    let mut fields = String::new();
    frame.append_fields(&mut fields);
    assert!(fields.contains("main_frame:2:96"), "{fields}");
    assert!(fields.contains("ui_preparation:1:32"), "{fields}");
    fields.clear();
    profiler
        .state
        .allocations
        .finish_frame()
        .append_fields(&mut fields);
    assert!(!fields.contains("main_frame:"), "{fields}");
}

/// A timer moved between threads never subtracts unrelated thread-local baselines.
#[test]
fn moved_timer_skips_incomparable_thread_counters() {
    let profiler = RuntimeStageProfiler::new(true);
    let timer = profiler.time(RuntimeStage::WorldPoll);
    std::thread::scope(|scope| scope.spawn(move || drop(timer)).join().unwrap());
    let mut fields = String::new();
    profiler
        .state
        .allocations
        .finish_frame()
        .append_fields(&mut fields);
    assert!(!fields.contains("world_poll:"), "{fields}");
}

/// Schedule-spanning stages count work between their actual start and end systems.
#[test]
fn system_spans_count_requests_between_boundaries() {
    use bevy::prelude::*;
    const STAGE: usize = RuntimeStage::UiPublication as usize;
    let profiler = RuntimeStageProfiler::new(true);
    let mut world = World::new();
    world.insert_resource(profiler.clone());
    world.init_resource::<RuntimeStageSpans>();
    let mut parameters = bevy::ecs::system::SystemState::<(
        Option<Res<RuntimeStageProfiler>>,
        Option<ResMut<RuntimeStageSpans>>,
    )>::new(&mut world);
    let (profiler_resource, spans) = parameters.get_mut(&mut world).unwrap();
    begin_stage_span::<STAGE>(spans, profiler_resource);
    let bytes = std::hint::black_box(vec![0_u8; 128]);
    let (profiler_resource, spans) = parameters.get_mut(&mut world).unwrap();
    end_stage_span::<STAGE>(profiler_resource, spans);
    std::hint::black_box(bytes);
    assert_stage_allocations(&profiler, RuntimeStage::UiPublication, 1, 128);
}

/// Phase boundaries may migrate while allocating systems run on different executor threads.
#[test]
fn system_spans_count_parallel_requests_across_boundary_threads() {
    use bevy::{
        ecs::{schedule::MultiThreadedExecutor, system::NonSendMarker},
        prelude::*,
        tasks::{ComputeTaskPool, TaskPoolBuilder},
    };
    use std::sync::{Arc, Barrier};

    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(2).build());
    const BYTES: usize = 1024 * 1024;
    for stage in [RuntimeStage::MainUpdate, RuntimeStage::RenderFrame] {
        let profiler = RuntimeStageProfiler::new(true);
        let mut world = World::new();
        world.insert_resource(profiler.clone());
        world.init_resource::<RuntimeStageSpans>();
        let owner = std::thread::current().id();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let begin = move |_: NonSendMarker,
                          spans: Option<ResMut<RuntimeStageSpans>>,
                          profiler: Option<Res<RuntimeStageProfiler>>| {
            assert_eq!(std::thread::current().id(), owner);
            match stage {
                RuntimeStage::MainUpdate => {
                    begin_stage_span::<{ RuntimeStage::MainUpdate as usize }>(spans, profiler);
                }
                _ => begin_stage_span::<{ RuntimeStage::RenderFrame as usize }>(spans, profiler),
            }
        };
        let main_work = move |_: NonSendMarker| {
            assert_eq!(std::thread::current().id(), owner);
            barrier.wait();
            std::hint::black_box(vec![0_u8; BYTES]);
        };
        let worker_work = move || {
            assert_ne!(std::thread::current().id(), owner);
            worker_barrier.wait();
            std::hint::black_box(vec![0_u8; BYTES]);
        };
        let end = move |profiler: Option<Res<RuntimeStageProfiler>>,
                        spans: Option<ResMut<RuntimeStageSpans>>| {
            assert_ne!(std::thread::current().id(), owner);
            match stage {
                RuntimeStage::MainUpdate => {
                    end_stage_span::<{ RuntimeStage::MainUpdate as usize }>(profiler, spans);
                }
                _ => end_render_frame_span(profiler, spans),
            }
        };
        let mut schedule = Schedule::default();
        schedule.set_executor(MultiThreadedExecutor::new());
        schedule.add_systems((begin, (main_work, worker_work), end).chain());
        schedule.run(&mut world);
        assert_stage_allocations(&profiler, stage, 2, 2 * BYTES);
    }
}

/// Process totals include concurrent executor and test-harness work, so known requests are a floor.
fn assert_stage_allocations(
    profiler: &RuntimeStageProfiler,
    stage: RuntimeStage,
    calls: usize,
    bytes: usize,
) {
    let mut fields = String::new();
    profiler
        .state
        .allocations
        .finish_frame()
        .append_fields(&mut fields);
    let allocations = fields
        .split_once("stage_allocations=")
        .unwrap()
        .1
        .split_whitespace()
        .next()
        .unwrap();
    let sample = allocations
        .split(',')
        .find_map(|sample| sample.strip_prefix(&format!("{}:", stage.name())))
        .unwrap_or_else(|| panic!("missing {} allocations: {fields}", stage.name()));
    let (actual_calls, actual_bytes) = sample.split_once(':').unwrap();
    assert!(actual_calls.parse::<usize>().unwrap() >= calls, "{fields}");
    assert!(actual_bytes.parse::<usize>().unwrap() >= bytes, "{fields}");
}
