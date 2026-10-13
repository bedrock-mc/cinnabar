//! Startup icon admission must also reach the held sword mesh and its artwork atlas.

use super::{
    BlockIcons, compile_session_icons,
    startup_fixture::{ICON, IDENTIFIER, NETWORK_ID, PLAYER_ID, local_view, selected_inventory},
};
use client_presentation::{
    presentation::equipment::test_support::{FirstPersonFrame, render_first_person_session_item},
    session_assets::SessionItems,
};
use resource_pack::LayeredPackView;
use std::sync::Arc;

#[test]
fn startup_custom_item_survives_empty_registry_duplicate_in_first_person() {
    for entries in [
        [
            (IDENTIFIER, NETWORK_ID, Some("test:gem")),
            (IDENTIFIER, NETWORK_ID, None),
        ],
        [
            (IDENTIFIER, NETWORK_ID, None),
            (IDENTIFIER, NETWORK_ID, Some("test:gem")),
        ],
    ] {
        let frame = held_frame(&super::tests::view(), &entries);
        assert_held_mesh(&frame);
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| { pixel[0] > 0 && pixel[0] <= 15 && pixel[1] == 0 && pixel[2] == 0 }),
            "the held mesh must sample the original fixture's red texture"
        );
    }
}

#[test]
fn startup_custom_sword_survives_empty_registry_duplicate_in_first_person() {
    let Some(view) =
        local_view("startup_custom_sword_survives_empty_registry_duplicate_in_first_person")
    else {
        return;
    };
    let frame = held_frame(
        &view,
        &[
            (IDENTIFIER, NETWORK_ID, Some(ICON)),
            (IDENTIFIER, NETWORK_ID, None),
        ],
    );
    if let Some(directory) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") {
        image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba8.clone())
            .expect("complete frame pixels")
            .save(std::path::Path::new(&directory).join("first-person-custom-sword.png"))
            .expect("write held sword snapshot");
    }
    assert_held_mesh(&frame);
}

/// Resolves wire startup facts and the pack before invoking the production held-item path.
fn held_frame(view: &LayeredPackView, entries: &[(&str, i16, Option<&str>)]) -> FirstPersonFrame {
    let startup = protocol::test_support::items::startup_with_item_icons(entries);
    let icons = compile_session_icons(view, &startup.items.icon_keys, BlockIcons::default());
    let items = SessionItems {
        components: Arc::new(startup.items.components.into_iter().collect()),
        icons,
    };
    let inventory = selected_inventory(startup.items.registry.expect("admitted startup registry"));
    let selected = client_presentation::presentation::equipment::local_input(
        &inventory.player,
        &inventory.stream,
        Some(&inventory.runtime),
        PLAYER_ID,
    )
    .main
    .expect("registry-backed selected sword");
    assert_eq!(selected.identifier.as_ref(), IDENTIFIER);
    render_first_person_session_item(&items, &selected)
}

/// Requires a visible extruded model rather than a successful inventory icon lookup alone.
fn assert_held_mesh(frame: &FirstPersonFrame) {
    assert!(
        frame.item_drawn,
        "the custom sword must reach the first-person hand"
    );
    assert!(
        frame.vertex_count > 6,
        "the held sprite must have extruded geometry"
    );
    assert!(
        frame.visible_pixels > 0,
        "the held texture must be visible in the frame"
    );
}
