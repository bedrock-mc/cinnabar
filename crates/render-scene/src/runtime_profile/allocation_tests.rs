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
    let (_, spans) = parameters.get_mut(&mut world);
    begin_stage_span::<STAGE>(spans);
    let bytes = std::hint::black_box(vec![0_u8; 128]);
    let (profiler_resource, spans) = parameters.get_mut(&mut world);
    end_stage_span::<STAGE>(profiler_resource, spans);
    std::hint::black_box(bytes);
    let mut fields = String::new();
    profiler
        .state
        .allocations
        .finish_frame()
        .append_fields(&mut fields);
    assert!(fields.contains("ui_publication:1:128"), "{fields}");
}
