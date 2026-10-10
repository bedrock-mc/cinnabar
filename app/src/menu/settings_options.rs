//! Host adapters for shared launcher settings.

mod actions;
mod chat;
pub(crate) mod control_bindings;
mod language;
mod reset;
mod runtime;
#[cfg(test)]
mod tests;

use launcher::menu::settings_options::*;
