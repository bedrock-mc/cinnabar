use super::*;

/// Captures all authority fields the scoped rendering view must restore.
fn inventory_state(runtime: &UiRuntime) -> (String, String, bool, Option<[f32; 2]>) {
    (
        format!("{:?}", runtime.inventory_ledger()),
        format!("{:?}", runtime.server_forms()),
        runtime.inventory_open(),
        runtime.inventory_pointer_gui(),
    )
}

#[test]
fn captured_inventory_keeps_pre_send_pixels_and_restores_post_send_authority() {
    let mut immediate = super::super::forms::tests::mini_engine_presentation();
    let mut deferred = super::super::forms::tests::mini_engine_presentation();
    let mut runtime = super::super::forms::pack_harness::action_form("before send", &["keep"]);
    let dpi = DpiScale::new(1.0).unwrap();
    let expected = immediate.build(&runtime, 100, [800, 600], dpi).unwrap();
    assert!(
        !expected.vertices.is_empty(),
        "fixture must render before capture"
    );
    let captured = runtime.capture_presentation_inventory();
    runtime.server_forms_mut().clear();
    runtime.inventory_open = true;
    runtime.inventory_pointer_gui = Some([23.0, 42.0]);
    runtime.inventory_ledger.begin_session(2);
    let after_send = inventory_state(&runtime);
    let actual = runtime.with_presentation_inventory(captured, |before_send| {
        deferred.build(before_send, 100, [800, 600], dpi).unwrap()
    });
    assert_eq!(actual, expected);
    assert_eq!(inventory_state(&runtime), after_send);
    assert_ne!(
        immediate.build(&runtime, 100, [800, 600], dpi).unwrap(),
        expected
    );
}

#[test]
fn inventory_authority_is_restored_on_render_error_and_unwind() {
    let mut runtime = UiRuntime::new(1);
    for unwind in [false, true] {
        let captured = runtime.capture_presentation_inventory();
        runtime.inventory_open = !runtime.inventory_open;
        runtime.inventory_pointer_gui = Some([37.0, 19.0]);
        runtime.inventory_ledger.begin_session(2);
        let after_send = inventory_state(&runtime);
        if unwind {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.with_presentation_inventory(captured, |_| panic!("render panic"))
            }));
            assert!(result.is_err());
        } else {
            let result =
                runtime.with_presentation_inventory(captured, |_| Err::<(), _>("render error"));
            assert_eq!(result, Err("render error"));
        }
        assert_eq!(inventory_state(&runtime), after_send);
    }
}

#[test]
fn deferred_and_immediate_ui_match_across_retained_frames() {
    let mut immediate = super::super::forms::tests::mini_engine_presentation();
    let mut deferred = super::super::forms::tests::mini_engine_presentation();
    let mut runtime = super::super::forms::pack_harness::action_form("retained", &["one", "two"]);
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 16, 50, 100, 1_000] {
        let expected = immediate.build(&runtime, now, [800, 600], dpi).unwrap();
        assert!(
            !expected.vertices.is_empty(),
            "fixture must render retained frames"
        );
        let captured = runtime.capture_presentation_inventory();
        let actual = runtime.with_presentation_inventory(captured, |runtime| {
            deferred.build(runtime, now, [800, 600], dpi).unwrap()
        });
        assert_eq!(actual, expected);
    }
}

/// Uses the existing large creative fixture with the complete pinned registry.
fn catalog_runtime() -> UiRuntime {
    let mut runtime = container_screen_tests::creative_with(1500);
    runtime
        .inventory_ledger_mut()
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: protocol::vanilla_item_registry(),
        });
    runtime
}

#[test]
fn publication_snapshot_shares_registry_and_creative_catalogs() {
    let mut runtime = catalog_runtime();
    let first = protocol::vanilla_item_registry()[0].network_id;
    let entry = runtime
        .inventory_ledger()
        .negotiated_item_entry(first)
        .unwrap() as *const _;
    let creative = runtime
        .inventory_ledger()
        .creative_catalog()
        .unwrap()
        .items
        .as_ptr();
    let snapshot = runtime.capture_presentation_inventory();
    runtime.with_presentation_inventory(snapshot, |captured| {
        assert!(std::ptr::eq(
            entry,
            captured
                .inventory_ledger()
                .negotiated_item_entry(first)
                .unwrap()
        ));
        assert_eq!(
            creative,
            captured
                .inventory_ledger()
                .creative_catalog()
                .unwrap()
                .items
                .as_ptr()
        );
    });
}

#[test]
#[ignore = "measures the added pre-send UI snapshot cost without rendering"]
fn ui_publication_capture_bench() {
    use crate::tests::alloc_count::thread_allocations;
    let labels = vec!["Action"; 128];
    let cases = [
        ("idle", UiRuntime::new(1)),
        ("creative1500_registry", catalog_runtime()),
        (
            "form128",
            super::super::forms::pack_harness::action_form("Actions", &labels),
        ),
    ];
    for (name, runtime) in cases {
        let mut times = Vec::with_capacity(1000);
        let allocations = thread_allocations();
        for _ in 0..1000 {
            let started = std::time::Instant::now();
            drop(std::hint::black_box(
                runtime.capture_presentation_inventory(),
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
