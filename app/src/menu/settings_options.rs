//! Host adapters for shared launcher settings.

mod actions;
mod chat;
mod control_bindings;
mod language;
mod reset;
mod runtime;
#[cfg(test)]
mod tests;

#[cfg(feature = "developer-control")]
pub(crate) use control_bindings::named_control;
pub(crate) use control_bindings::{
    binding_gamepad, binding_key, binding_mouse, binding_mouse_button, binding_pressed,
    gamepad_button, hotbar_control_slots,
};
pub(crate) use launcher::menu::settings_options::*;
