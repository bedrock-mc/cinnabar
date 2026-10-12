//! Process supervision, crash capture and update checks.

pub mod children;
pub mod core_health;
#[cfg(not(target_arch = "wasm32"))]
pub mod crash;
#[cfg(not(target_arch = "wasm32"))]
pub mod update;
