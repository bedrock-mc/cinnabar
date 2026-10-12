//! Host adapters for shared launcher settings.

mod actions;
mod chat;
pub mod control_bindings;
mod language;
#[cfg(target_arch = "wasm32")]
mod language_download;
mod reset;
mod runtime;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod macro_tests;
