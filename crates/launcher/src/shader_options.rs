//! The Shader Pack Settings screen over Video settings: its commands and the immutable view
//! the app publishes while it is open. With the `shader-pack-options` feature, [`Editor`]
//! runs a pack's [`shaderpack::OptionSession`] behind them.

#[cfg(feature = "shader-pack-options")]
mod editor;
#[cfg(all(test, feature = "shader-pack-options"))]
mod tests;

#[cfg(feature = "shader-pack-options")]
pub use editor::{Editor, Outcome};

/// What a press on an element does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Flips a switch, opens a link, or steps a value or the profile forwards.
    Next,
    /// Steps a value or the profile backwards; flips a switch.
    Previous,
    /// Moves a slider to this choice.
    Select(u16),
}

/// Screen commands. Element presses name the screen they were drawn on, so a press queued
/// before the screen changed cannot reach an element of the new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Opens the screen for the selected pack (the Renderer row's button).
    Open,
    Element {
        screen: u32,
        index: u16,
        input: Input,
    },
    /// Leaves a sub-screen, or closes the main screen applying changes, as Iris does.
    Back,
    /// Saves and reloads with the changed values.
    Apply,
    /// Applies, then closes.
    Done,
    /// Drops the unapplied changes and closes.
    Cancel,
    /// Saves the pack's defaults and reloads.
    Reset,
}

/// How an element is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// A blank cell.
    Empty,
    /// Opens a sub-screen.
    Link,
    /// An on/off option.
    Switch { on: bool },
    /// A value option stepped through its choices; `value` is its display text.
    Choice { value: String },
    /// A value option with a single choice (such as a pack's name and author): shown as a
    /// heading, not a control.
    Fixed { value: String },
    /// A value option on a slider at choice `index` of `count`.
    Slider {
        value: String,
        index: u16,
        count: u16,
    },
    /// The profile selector showing the current profile (or "Custom").
    Profile { value: String },
}

/// One cell of the shown screen. Text keeps Minecraft formatting codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub label: String,
    /// Shown while the element is hovered or focused.
    pub comment: Option<String>,
    pub control: Control,
    /// The shown value differs from the one the pack runs with.
    pub modified: bool,
}

/// Whether the pack's menu is ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    /// Edits are waiting for storage to accept them.
    Saving,
    /// Edits remain available and can be saved again.
    SaveFailed(String),
    Ready,
    /// The pack could not be read; the reason is shown.
    Failed(String),
}

/// The open screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// The pack's display name.
    pub pack: String,
    /// The shown screen's title: the pack name, or the sub-screen's label.
    pub title: String,
    /// Identifies the shown screen for [`Action::Element`].
    pub screen: u32,
    /// A sub-screen is shown, so Back returns to its parent.
    pub nested: bool,
    /// Elements per row.
    pub columns: u16,
    pub elements: Vec<Element>,
    /// Changes wait to be applied.
    pub changed: bool,
    pub status: Status,
}

impl View {
    /// The screen while `pack` is being read.
    pub fn loading(pack: &str) -> View {
        View {
            pack: pack.to_owned(),
            title: pack.to_owned(),
            screen: 0,
            nested: false,
            columns: 1,
            elements: Vec::new(),
            changed: false,
            status: Status::Loading,
        }
    }

    /// The screen when `pack` could not be read.
    pub fn failed(pack: &str, reason: String) -> View {
        View {
            status: Status::Failed(reason),
            ..View::loading(pack)
        }
    }
}
