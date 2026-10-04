use super::*;

/// Captures all authority fields the scoped rendering view must restore.
fn inventory_state(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> (String, String, bool, Option<[f32; 2]>) {
    (
        format!("{:?}", runtime.inventory_ledger(player_runtime)),
        format!("{:?}", runtime.server_forms()),
        runtime.inventory_open(),
        runtime.inventory_pointer_gui(),
    )
}

#[test]
fn captured_inventory_keeps_pre_send_pixels_and_restores_post_send_authority() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut immediate = super::super::forms::tests::mini_engine_presentation();
    let mut deferred = super::super::forms::tests::mini_engine_presentation();
    let mut runtime = super::super::forms::pack_harness::action_form(
        &mut player_runtime,
        "before send",
        &["keep"],
    );
    let dpi = DpiScale::new(1.0).unwrap();
    let expected = immediate
        .build(&player_runtime, &runtime, 100, [800, 600], dpi)
        .unwrap();
    assert!(
        !expected.vertices.is_empty(),
        "fixture must render before capture"
    );
    let captured = runtime.capture_presentation_inventory(&player_runtime);
    runtime.server_forms_mut().clear();
    runtime.inventory_open = true;
    runtime.inventory_pointer_gui = Some([23.0, 42.0]);
    player_runtime.inventory.ledger_mut().begin_session(2);
    let after_send = inventory_state(&player_runtime, &runtime);
    let actual = runtime.with_presentation_inventory(
        &player_runtime,
        captured,
        |before_send, player_runtime| {
            deferred
                .build(player_runtime, before_send, 100, [800, 600], dpi)
                .unwrap()
        },
    );
    assert_eq!(actual, expected);
    assert_eq!(inventory_state(&player_runtime, &runtime), after_send);
    assert_ne!(
        immediate
            .build(&player_runtime, &runtime, 100, [800, 600], dpi)
            .unwrap(),
        expected
    );
}

#[test]
fn inventory_authority_is_restored_on_render_error_and_unwind() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    for unwind in [false, true] {
        let captured = runtime.capture_presentation_inventory(&player_runtime);
        runtime.inventory_open = !runtime.inventory_open;
        runtime.inventory_pointer_gui = Some([37.0, 19.0]);
        player_runtime.inventory.ledger_mut().begin_session(2);
        let after_send = inventory_state(&player_runtime, &runtime);
        if unwind {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.with_presentation_inventory(&player_runtime, captured, |_, _| {
                    panic!("render panic")
                })
            }));
            assert!(result.is_err());
        } else {
            let result = runtime.with_presentation_inventory(&player_runtime, captured, |_, _| {
                Err::<(), _>("render error")
            });
            assert_eq!(result, Err("render error"));
        }
        assert_eq!(inventory_state(&player_runtime, &runtime), after_send);
    }
}

#[test]
fn deferred_and_immediate_ui_match_across_retained_frames() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut immediate = super::super::forms::tests::mini_engine_presentation();
    let mut deferred = super::super::forms::tests::mini_engine_presentation();
    let mut runtime = super::super::forms::pack_harness::action_form(
        &mut player_runtime,
        "retained",
        &["one", "two"],
    );
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 16, 50, 100, 1_000] {
        let expected = immediate
            .build(&player_runtime, &runtime, now, [800, 600], dpi)
            .unwrap();
        assert!(
            !expected.vertices.is_empty(),
            "fixture must render retained frames"
        );
        let captured = runtime.capture_presentation_inventory(&player_runtime);
        let actual = runtime.with_presentation_inventory(
            &player_runtime,
            captured,
            |runtime, player_runtime| {
                deferred
                    .build(player_runtime, runtime, now, [800, 600], dpi)
                    .unwrap()
            },
        );
        assert_eq!(actual, expected);
    }
}

/// Uses the existing large creative fixture with the complete pinned registry.
fn catalog_runtime(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = container_screen_tests::creative_with(player_runtime, 1500);
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: protocol::vanilla_item_registry(),
        });
    runtime
}

#[test]
fn publication_snapshot_shares_registry_and_creative_catalogs() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = catalog_runtime(&mut player_runtime);
    let first = protocol::vanilla_item_registry()[0].network_id;
    let entry = runtime
        .inventory_ledger(&player_runtime)
        .negotiated_item_entry(first)
        .unwrap() as *const _;
    let creative = runtime
        .inventory_ledger(&player_runtime)
        .creative_catalog()
        .unwrap()
        .items
        .as_ptr();
    let snapshot = runtime.capture_presentation_inventory(&player_runtime);
    runtime.with_presentation_inventory(&player_runtime, snapshot, |captured, player_runtime| {
        assert!(std::ptr::eq(
            entry,
            captured
                .inventory_ledger(player_runtime)
                .negotiated_item_entry(first)
                .unwrap()
        ));
        assert_eq!(
            creative,
            captured
                .inventory_ledger(player_runtime)
                .creative_catalog()
                .unwrap()
                .items
                .as_ptr()
        );
    });
}
