//! Snapshot allocation benchmark owned by the app allocator harness.
use super::*;

/// Seeds the authoritative catalog used by the allocation benchmark.
fn catalog_runtime(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> UiRuntime {
    let mut runtime = client_ui::test_support::creative_with(player_runtime, 1500);
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: protocol::vanilla_item_registry(),
        });
    runtime
}

#[test]
#[ignore = "measures the added pre-send UI snapshot cost without rendering"]
fn ui_publication_capture_bench() {
    use crate::tests::alloc_count::thread_allocations;
    let labels = vec!["Action"; 128];
    let cases = [
        (
            "idle",
            crate::player_runtime::PlayerRuntime::new(1),
            UiRuntime::new(1),
        ),
        {
            let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
            let runtime = catalog_runtime(&mut player_runtime);
            ("creative1500_registry", player_runtime, runtime)
        },
        {
            let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
            let runtime = client_ui::test_support::pack_harness::action_form(
                &mut player_runtime,
                "Actions",
                &labels,
            );
            ("form128", player_runtime, runtime)
        },
    ];
    for (name, player_runtime, runtime) in cases {
        let mut times = Vec::with_capacity(1000);
        let allocations = thread_allocations();
        for _ in 0..1000 {
            let started = std::time::Instant::now();
            drop(std::hint::black_box(
                runtime.capture_presentation_inventory(&player_runtime),
            ));
            times.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        let allocations = thread_allocations() - allocations;
        times.sort_by(f64::total_cmp);
        eprintln!(
            "UI_CAPTURE_BENCH case={name} samples={} p50_us={:.3} p95_us={:.3} allocations_per_frame={:.1}",
            times.len(),
            times[500],
            times[950],
            allocations as f64 / times.len() as f64
        );
    }
}
