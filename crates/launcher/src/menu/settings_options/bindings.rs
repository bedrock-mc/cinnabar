//! One binding registry owns names, device rows and supplemental defaults.

use semantic_input::{Action, PhysicalControl};

/// Whether this build offers shader-pack controls; the app enables it with its
/// `shaderpacks` feature, alongside `render_model::SHADER_PACKS_ENABLED`.
pub const SHADER_PACK_BINDINGS: bool = cfg!(feature = "shaderpacks");

/// A row in a device's action or supplemental binding grid.
#[derive(Clone, Copy, Debug)]
pub enum DeviceRow {
    Action(usize),
    Extra(usize, Option<PhysicalControl>),
}

/// Shared metadata for a remappable control.
#[derive(Clone, Copy, Debug)]
pub struct BindingDefinition {
    pub id: BindingId,
    pub name: &'static str,
    pub action: Option<Action>,
    pub keyboard: Option<DeviceRow>,
    pub gamepad: Option<DeviceRow>,
    pub secondary: Option<PhysicalControl>,
    pub late_default: bool,
    pub label: Option<&'static str>,
}

macro_rules! bindings {
    ($($id:ident => $name:literal, $action:expr, $key:expr, $pad:expr,
        $secondary:expr, $late:expr, $label:expr;)+) => {
        /// A known control name; row order and saved names come from the registry.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum BindingId { $($id,)+ }

        impl BindingId {
            /// Returns this control's stable saved key and language key.
            pub const fn name(self) -> &'static str { match self { $(Self::$id => $name,)+ } }

            /// Validates a binding name at an external input or saved-file boundary.
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$id),)+ _ => None }
            }

            /// Returns metadata from the same registry that declares the id.
            pub const fn definition(self) -> BindingDefinition { BINDINGS[self as usize] }

            /// Finds a binding id for a remappable semantic action.
            pub fn from_action(action: Action) -> Option<Self> {
                BINDINGS.iter().find(|row| row.action == Some(action)).map(|row| row.id)
            }

            /// Returns the unchanged row index for the selected device.
            pub const fn row(self, gamepad: bool) -> Option<usize> {
                let row = self.definition();
                let device = if gamepad { row.gamepad } else { row.keyboard };
                let offset = if gamepad { super::GAMEPAD_OFFSET } else { 0 };
                match device {
                    Some(DeviceRow::Action(index)) => Some(offset + index),
                    Some(DeviceRow::Extra(index, _)) => Some(offset + action_count(gamepad) + index),
                    None => None,
                }
            }
        }

        pub const BINDINGS: &[BindingDefinition] = &[$(
            BindingDefinition { id: BindingId::$id, name: $name, action: $action,
                keyboard: $key, gamepad: $pad, secondary: $secondary,
                late_default: $late, label: $label },
        )+];
    }
}

bindings! {
    Attack => "key.attack", Some(Action::Attack), Some(DeviceRow::Action(0)), Some(DeviceRow::Action(0)),
        None, false, None;
    Use => "key.use", Some(Action::Use), Some(DeviceRow::Action(1)), Some(DeviceRow::Action(1)),
        None, false, None;
    Forward => "key.forward", Some(Action::MoveForward), Some(DeviceRow::Action(2)), None,
        None, false, None;
    Back => "key.back", Some(Action::MoveBackward), Some(DeviceRow::Action(3)), None,
        None, false, None;
    Left => "key.left", Some(Action::MoveLeft), Some(DeviceRow::Action(4)), None,
        None, false, None;
    Right => "key.right", Some(Action::MoveRight), Some(DeviceRow::Action(5)), None,
        None, false, None;
    Jump => "key.jump", Some(Action::Jump), Some(DeviceRow::Action(6)), Some(DeviceRow::Action(2)),
        None, false, None;
    Sneak => "key.sneak", Some(Action::Sneak), Some(DeviceRow::Action(7)), Some(DeviceRow::Action(3)),
        None, false, None;
    Sprint => "key.sprint", Some(Action::Sprint), Some(DeviceRow::Action(8)), Some(DeviceRow::Action(4)),
        None, false, None;
    TogglePerspective => "key.togglePerspective", Some(Action::CyclePerspective), Some(DeviceRow::Action(9)), Some(DeviceRow::Action(5)),
        None, false, None;
    Hotbar1 => "key.hotbar.1", Some(Action::Hotbar1), Some(DeviceRow::Action(10)), None,
        None, false, None;
    Hotbar2 => "key.hotbar.2", Some(Action::Hotbar2), Some(DeviceRow::Action(11)), None,
        None, false, None;
    Hotbar3 => "key.hotbar.3", Some(Action::Hotbar3), Some(DeviceRow::Action(12)), None,
        None, false, None;
    Hotbar4 => "key.hotbar.4", Some(Action::Hotbar4), Some(DeviceRow::Action(13)), None,
        None, false, None;
    Hotbar5 => "key.hotbar.5", Some(Action::Hotbar5), Some(DeviceRow::Action(14)), None,
        None, false, None;
    Hotbar6 => "key.hotbar.6", Some(Action::Hotbar6), Some(DeviceRow::Action(15)), None,
        None, false, None;
    Hotbar7 => "key.hotbar.7", Some(Action::Hotbar7), Some(DeviceRow::Action(16)), None,
        None, false, None;
    Hotbar8 => "key.hotbar.8", Some(Action::Hotbar8), Some(DeviceRow::Action(17)), None,
        None, false, None;
    Hotbar9 => "key.hotbar.9", Some(Action::Hotbar9), Some(DeviceRow::Action(18)), None,
        None, false, None;
    Freelook => "key.freelook", Some(Action::Freelook), Some(DeviceRow::Action(19)), None,
        None, true, Some("Freelook");
    InteractWithToast => "key.interactwithtoast", Some(Action::InteractWithToast), Some(DeviceRow::Action(20)), None,
        None, true, None;
    PlayerList => "key.playerList", Some(Action::PlayerList), Some(DeviceRow::Action(21)), None,
        None, false, Some("Player List");
    Inventory => "key.inventory", None, Some(DeviceRow::Extra(0, Some(PhysicalControl::KeyboardUsage(0x08)))), Some(DeviceRow::Extra(0, Some(PhysicalControl::GamepadButton(2)))),
        None, false, None;
    Chat => "key.chat", None, Some(DeviceRow::Extra(1, Some(PhysicalControl::KeyboardUsage(0x17)))), Some(DeviceRow::Extra(1, Some(PhysicalControl::GamepadButton(14)))),
        Some(PhysicalControl::KeyboardUsage(0x28)), false, None;
    Command => "key.command", None, Some(DeviceRow::Extra(2, Some(PhysicalControl::KeyboardUsage(0x38)))), None,
        None, false, None;
    Drop => "key.drop", None, Some(DeviceRow::Extra(3, Some(PhysicalControl::KeyboardUsage(0x14)))), Some(DeviceRow::Extra(2, Some(PhysicalControl::GamepadButton(12)))),
        None, false, None;
    PickItem => "key.pickItem", Some(Action::PickItem), Some(DeviceRow::Extra(4, Some(PhysicalControl::MouseButton(3)))), Some(DeviceRow::Extra(3, None)),
        None, false, None;
    Screenshot => "key.screenshot", None, Some(DeviceRow::Extra(5, Some(PhysicalControl::KeyboardUsage(0x3b)))), None,
        None, false, None;
    Fullscreen => "key.fullscreen", None, Some(DeviceRow::Extra(6, Some(PhysicalControl::KeyboardUsage(0x44)))), None,
        None, false, None;
    Emote => "key.emote", None, Some(DeviceRow::Extra(7, Some(PhysicalControl::KeyboardUsage(0x05)))), Some(DeviceRow::Extra(4, Some(PhysicalControl::GamepadButton(13)))),
        None, false, None;
    DebugOverlay => "key.debugOverlay", None, Some(DeviceRow::Extra(8, Some(PhysicalControl::KeyboardUsage(0x3c)))), None,
        None, false, Some("Debug Overlay");
    // Iris's "Reload Shaders" on R; only builds that compile shader packs offer the row.
    ReloadShaders => "key.reloadShaders", None, if SHADER_PACK_BINDINGS { Some(DeviceRow::Extra(9, Some(PhysicalControl::KeyboardUsage(0x15)))) } else { None }, None,
        None, false, Some("Reload Shaders");
    CycleItemLeft => "key.cycleItemLeft", Some(Action::HotbarPrevious), None, Some(DeviceRow::Action(6)),
        None, false, None;
    CycleItemRight => "key.cycleItemRight", Some(Action::HotbarNext), None, Some(DeviceRow::Action(7)),
        None, false, None;
}

/// Counts action rows for one device from the binding definitions.
const fn action_count(gamepad: bool) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < BINDINGS.len() {
        let row = if gamepad {
            BINDINGS[index].gamepad
        } else {
            BINDINGS[index].keyboard
        };
        if matches!(row, Some(DeviceRow::Action(_))) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Counts supplemental rows for one device from the binding definitions.
const fn extra_count(gamepad: bool) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < BINDINGS.len() {
        let row = if gamepad {
            BINDINGS[index].gamepad
        } else {
            BINDINGS[index].keyboard
        };
        if matches!(row, Some(DeviceRow::Extra(..))) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Collects semantic rows at their stable device indices.
const fn action_rows<const N: usize>(gamepad: bool) -> [(Action, &'static str); N] {
    let mut rows = [(Action::Attack, ""); N];
    let mut index = 0;
    while index < BINDINGS.len() {
        let definition = BINDINGS[index];
        let device = if gamepad {
            definition.gamepad
        } else {
            definition.keyboard
        };
        if let Some(DeviceRow::Action(row)) = device {
            let Some(action) = definition.action else {
                panic!("action row needs an action")
            };
            rows[row] = (action, definition.name);
        }
        index += 1;
    }
    rows
}

/// Collects supplemental rows at their stable device indices.
const fn extra_rows<const N: usize>(gamepad: bool) -> [(&'static str, Option<PhysicalControl>); N] {
    let mut rows = [("", None); N];
    let mut index = 0;
    while index < BINDINGS.len() {
        let definition = BINDINGS[index];
        let device = if gamepad {
            definition.gamepad
        } else {
            definition.keyboard
        };
        if let Some(DeviceRow::Extra(row, control)) = device {
            rows[row] = (definition.name, control);
        }
        index += 1;
    }
    rows
}

pub const KEY_BINDINGS: &[(Action, &str)] = &action_rows::<{ action_count(false) }>(false);
pub const GAMEPAD_BINDINGS: &[(Action, &str)] = &action_rows::<{ action_count(true) }>(true);
pub const EXTRA_GAMEPAD: &[(&str, Option<PhysicalControl>)] =
    &extra_rows::<{ extra_count(true) }>(true);

/// Resolves keyboard extras, which always provide a physical default.
const fn keyboard_extras() -> [(&'static str, PhysicalControl); extra_count(false)] {
    let rows = extra_rows::<{ extra_count(false) }>(false);
    let mut result = [("", PhysicalControl::KeyboardUsage(0)); extra_count(false)];
    let mut index = 0;
    while index < rows.len() {
        let Some(control) = rows[index].1 else {
            panic!("keyboard extras need a default")
        };
        result[index] = (rows[index].0, control);
        index += 1;
    }
    result
}

pub const EXTRA_KEYS: &[(&str, PhysicalControl)] = &keyboard_extras();

/// Counts actions whose defaults yield to legacy saved layouts.
const fn late_count() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < BINDINGS.len() {
        if BINDINGS[index].late_default {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Collects yielding defaults from the same rows used for key resolution.
const fn late_defaults() -> [Action; late_count()] {
    let mut result = [Action::Attack; late_count()];
    let mut row = 0;
    let mut index = 0;
    while index < BINDINGS.len() {
        if BINDINGS[index].late_default {
            let Some(action) = BINDINGS[index].action else {
                panic!("late defaults need an action")
            };
            result[row] = action;
            row += 1;
        }
        index += 1;
    }
    result
}

pub(super) const LATE_DEFAULTS: [Action; late_count()] = late_defaults();
