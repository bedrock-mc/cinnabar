//! Launcher frame adapter around the extracted JSON-UI presentation.
#[cfg(test)]
use client_ui::ui_runtime::presentation::forms::ServerUiPack;
#[cfg(test)]
use client_ui::ui_runtime::presentation::forms::snapshot;
#[cfg(test)]
use ui::LoadingStage;
pub mod panorama;
pub(crate) use panorama::drive_menu_panorama;

#[cfg(test)]
pub mod pack_harness;

#[cfg(test)]
pub(crate) mod tests {}

#[cfg(test)]
mod loading_sequence_tests;
#[cfg(test)]
mod menu_gpu_tests;
#[cfg(test)]
mod play_flow_snapshots {}
