#![doc = include_str!("../README.md")]
#![cfg(not(target_arch = "wasm32"))]

mod account;

mod backend;
mod bearer;
mod budget;
mod control;
mod play;
mod prefix;
mod registry;
mod session;
mod socket;
mod upstream;

pub use backend::Gateway;

#[cfg(feature = "operator")]
pub mod operator;

#[cfg(test)]
mod tests;
