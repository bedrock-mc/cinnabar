use super::*;
use crate::ui_runtime::presentation::{forms, inventory_pointer::InventoryScreen, screens};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryEvent, InventoryLayout, InventoryOptions,
};

/// Renders an admitted inventory with original fixtures and no JSON-UI carrier.
fn fallback_layout(workbench: bool, layout: InventoryLayout) -> image::RgbaImage {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let options = InventoryOptions {
        inventory_layout: if workbench {
            InventoryLayout::InventoryOnly
        } else {
            layout
        },
        crafting_layout: if workbench {
            layout
        } else {
            InventoryLayout::InventoryOnly
        },
        ..Default::default()
    };
    let packet = protocol::inventory_options_packet(options);
    let packet = protocol::decode_batch(protocol::encode(&packet).unwrap())
        .unwrap()
        .pop()
        .unwrap();
    let Some(protocol::WorldEvent::Inventory(event)) =
        protocol::into_world_event(packet, 0).unwrap()
    else {
        panic!("inventory options event")
    };
    runtime
        .enqueue_inventory_event(&mut player, 1, 1, event)
        .unwrap();
    if workbench {
        runtime
            .enqueue_inventory_event(
                &mut player,
                1,
                2,
                InventoryEvent::Open(ContainerOpenEvent {
                    container: ContainerIdentity::window(7),
                    window_type: protocol::WINDOW_TYPE_WORKBENCH,
                    position: [0, 64, 0],
                    runtime_entity_id: -1,
                }),
            )
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    if !workbench {
        runtime.toggle_inventory(&mut player, None);
    }
    assert!(runtime.inventory_open());
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        layout
    );
    assert_eq!(player.inventory.inventory_options(), options);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let size = [1280, 720];
    let input = presentation
        .build(&player, &runtime, 500, size, DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(presentation.engine_container_frame().is_none());
    let shown = forms::recipe_book_shown(&player, &runtime);
    assert_eq!(shown, layout == InventoryLayout::RecipeBook);
    let screen = InventoryScreen::of_runtime(&player, &runtime);
    let geometry = HudGeometry::new(
        size,
        1.0,
        presentation.safe_area,
        presentation.gui_scale_preference,
    )
    .unwrap();
    let origin = screens::panel_origin(screen, [geometry.gui_width, geometry.gui_height]);
    let cell = screens::book_cell_origin(0, screens::book_origin(origin));
    assert_eq!(
        presentation
            .inventory_book_hit([cell[0] + 1.0, cell[1] + 1.0], size, 1.0, screen, shown)
            .is_some(),
        shown
    );
    forms::snapshot::write(
        &input,
        &format!(
            "fallback-layout-{}-{}",
            if workbench { "workbench" } else { "personal" },
            layout.index()
        ),
    );
    forms::snapshot::rasterize(&input)
}

#[test]
fn fallback_recipe_panel_matches_layout_visibility() {
    let mut mismatches = Vec::new();
    for workbench in [false, true] {
        let hidden = fallback_layout(workbench, InventoryLayout::InventoryOnly);
        let shown = fallback_layout(workbench, InventoryLayout::RecipeBook);
        assert!(shown != hidden, "a supported book layout draws its panel");
        for index in [-1, i32::MIN] {
            let retained = fallback_layout(workbench, InventoryLayout::from_index(index));
            if retained != hidden {
                mismatches.push((workbench, index));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "retained layouts draw a recipe panel without book controls: {mismatches:?}"
    );
}
