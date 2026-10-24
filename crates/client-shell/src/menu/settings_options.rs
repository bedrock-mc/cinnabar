//! Host adapters for shared launcher settings.

mod actions;
mod chat;
pub mod control_bindings;
mod language;
mod reset;
mod runtime;
pub(crate) use runtime::flush_on_exit;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod macro_tests;

#[cfg(test)]
mod preset_tests;

#[cfg(test)]
mod search_tests;
