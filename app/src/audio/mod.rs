//! Composition adapters for the presentation audio plugin.
mod predicted;
mod synchronized;
mod systems;
mod weather;

pub(crate) use systems::configure;

#[cfg(test)]
mod local_bank_tests;

#[cfg(test)]
pub(crate) use predicted::drive_block_cues;
