use super::*;
use crate::ui_runtime::presentation::forms::{recipe_book_shown, snapshot};
use protocol::{InventoryLayout, InventoryOptions};

/// Renders fallback crafting screens with retained preferences and no installed carriers.
fn render_layout(layout: InventoryLayout, workbench: bool, captured: bool) -> image::RgbaImage {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player);
    let options = InventoryOptions {
        inventory_layout: layout,
        crafting_layout: layout,
        ..InventoryOptions::default()
    };
    player.inventory.set_inventory_options(options);
    if workbench {
        runtime
            .enqueue_inventory_event(
                &mut player,
                1,
                1,
                InventoryEvent::Open(ContainerOpenEvent {
                    container: ContainerIdentity::window(7),
                    window_type: protocol::WINDOW_TYPE_WORKBENCH,
                    position: [0, 64, 0],
                    runtime_entity_id: -1,
                }),
            )
            .unwrap();
        runtime.drain_pending_inventory(&mut player);
    } else {
        runtime.toggle_inventory(&mut player, None);
    }
    if !captured {
        runtime.screen_state_mut().inventory = None;
    }
    let expected = matches!(
        layout,
        InventoryLayout::RecipeBook | InventoryLayout::RecipeBookOnly
    );
    assert_eq!(recipe_book_shown(&player, &runtime), expected);
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let input = presentation
        .build(
            &player,
            &runtime,
            500,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(presentation.engine_container_frame().is_none());
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        layout
    );
    assert_eq!(player.inventory.inventory_options(), options);
    assert!(runtime.take_client_packet().is_none());
    snapshot::write(
        &input,
        &format!(
            "fallback-recipe-{}-{}-{captured}",
            if workbench { "workbench" } else { "personal" },
            layout.index()
        ),
    );
    snapshot::rasterize(&input)
}

#[test]
fn fallback_recipe_visibility_matches_retained_layouts() {
    for workbench in [false, true] {
        for captured in [true, false] {
            let hidden = render_layout(InventoryLayout::InventoryOnly, workbench, captured);
            let shown = render_layout(InventoryLayout::RecipeBook, workbench, captured);
            assert!(
                shown != hidden,
                "recipe-book positive control must draw its panel"
            );
            for value in [-1, i32::MIN] {
                let unknown = render_layout(InventoryLayout::Unknown(value), workbench, captured);
                assert!(
                    unknown == hidden,
                    "unknown layout {value}: fallback drawing must agree with hidden interaction; workbench={workbench}, captured={captured}"
                );
            }
        }
    }
}
