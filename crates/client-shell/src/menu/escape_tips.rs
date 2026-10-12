//! Browsers free the captured mouse on Escape and capture it again only for a click or key the
//! player presses, which Escape does not count as. So a browser's Escape leaves the inventory,
//! containers, forms, dialogs, the sign editor and the pause menu open and says how to close
//! them, and closing them captures the mouse again.

use client_ui::ui_runtime::{UiRuntime, presentation::forms::oreui::RESUME_LABEL};
use launcher::menu::settings_options::key_name;

use super::MenuRuntime;

/// Whether Escape closes in-game screens, as it does outside browsers.
pub const ESCAPE_CLOSES_SCREENS: bool = !cfg!(target_arch = "wasm32");

/// A screen a browser's Escape left open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeTip {
    /// The inventory or a container, closed by the inventory key.
    Inventory,
    Pause,
    /// A server form, dialog or other screen with its own close button.
    Screen,
    Sign,
}

impl EscapeTip {
    /// The toast title and message, naming `menu`'s inventory key, or E without a menu.
    pub fn text(self, menu: Option<&MenuRuntime>) -> (&'static str, String) {
        let message = match self {
            Self::Inventory => {
                let key = menu
                    .and_then(|menu| menu.settings_options.named_key_control("key.inventory"))
                    .map_or_else(|| "E".to_owned(), key_name);
                format!("Press {key} to close the inventory.")
            }
            Self::Pause => format!("Choose {RESUME_LABEL} to keep playing."),
            Self::Screen => "Use its close button to close it.".to_owned(),
            Self::Sign => "Click outside the sign to finish editing.".to_owned(),
        };
        ("Escape frees the mouse in browsers", message)
    }

    /// Shows the tip as a client toast at `now_millis`.
    pub fn show(self, runtime: &mut UiRuntime, menu: Option<&MenuRuntime>, now_millis: u64) {
        let (title, message) = self.text(menu);
        runtime.show_client_toast(title, &message, now_millis);
    }
}

impl MenuRuntime {
    /// Notes that Escape left `tip`'s screen open; the menu input shows its tip.
    pub(super) fn keep_open_on_escape(&mut self, tip: EscapeTip) {
        self.escape_tip = Some(tip);
    }

    /// The tip Escape left to show, once.
    pub fn take_escape_tip(&mut self) -> Option<EscapeTip> {
        self.escape_tip.take()
    }
}
