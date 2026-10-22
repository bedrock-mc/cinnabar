//! Anonymous Home and Pause states for action-column comparisons.

use super::{Fixture, GUI_SCALE, MenuScreen, MenuView, SIZE, State};

/// Opens a supported menu route without an account or a live connection.
fn menu(screen: MenuScreen) -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = screen;
    view.over_world = screen == MenuScreen::Pause;
    view.focused_action = None;
    view.navigation_focus_visible = false;
    Fixture::Menu(Box::new(view))
}

pub(super) const STATES: &[State] = &[
    State {
        name: "home-buttons",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || menu(MenuScreen::Home),
    },
    State {
        name: "pause-buttons",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || menu(MenuScreen::Pause),
    },
    State {
        name: "home-buttons-small",
        size: [480, 270],
        gui_scale: 1,
        fixture: || menu(MenuScreen::Home),
    },
    State {
        name: "pause-buttons-small",
        size: [480, 270],
        gui_scale: 1,
        fixture: || menu(MenuScreen::Pause),
    },
    State {
        name: "home-buttons-portrait",
        size: [480, 640],
        gui_scale: GUI_SCALE,
        fixture: || menu(MenuScreen::Home),
    },
    State {
        name: "pause-buttons-portrait",
        size: [480, 640],
        gui_scale: GUI_SCALE,
        fixture: || menu(MenuScreen::Pause),
    },
    State {
        name: "home-buttons-square",
        size: [240, 240],
        gui_scale: 1,
        fixture: || menu(MenuScreen::Home),
    },
    State {
        name: "pause-buttons-compact",
        size: [
            ui::MIN_WINDOW_LOGICAL_SIZE[0] as u32,
            ui::MIN_WINDOW_LOGICAL_SIZE[1] as u32,
        ],
        gui_scale: 1,
        fixture: || menu(MenuScreen::Pause),
    },
];
