//! App adapters for presentation observations.
pub(crate) use client_presentation::presentation::equipment;
#[cfg(test)]
pub(crate) use client_presentation::presentation::{
    actors, cape, entity_layers, skin_layers, skin_rig,
};
pub(crate) mod viewmodel;
