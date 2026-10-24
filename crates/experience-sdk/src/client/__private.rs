//! Adapts the unchanged client ABI to typed handlers.

use super::{ClientPart, REGISTRY_STATE, active, record};
use crate::{Value, message::ToClient};

/// Generated application routing, outside the authored callback trait.
pub trait Dispatch {
    /// Decodes a declared channel or returns a rejection to discard.
    fn dispatch(channel: String, record: Vec<Value>) -> Result<(), String>;
}

/// Checks a handler's direction at compile time and validates all payload fields.
pub fn decode<M: ToClient>(record: Vec<Value>) -> Result<M, String> {
    M::decode(record)
}

/// Confirms the registry before routing records; the host has already checked wire schemas.
pub fn receive<T: ClientPart + Dispatch>(channel: String, json: Vec<u8>) {
    let Some(record) = record(&json) else {
        return;
    };
    if let Some(registry) = T::registry()
        && channel == registry.channel.id
    {
        let mut state = REGISTRY_STATE.get();
        let confirmation = state.confirm(registry, &record);
        REGISTRY_STATE.set(state);
        match confirmation {
            Ok(true) => T::init(),
            Ok(false) => {}
            Err(error) => panic!("{error}"),
        }
    } else if active::<T>() {
        let _ = T::dispatch(channel, record);
    }
}
