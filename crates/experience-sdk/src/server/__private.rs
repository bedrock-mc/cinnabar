//! Implementation details for the unchanged server guest ABI.

use super::{Callback, Experience, GuestError, PlayerId, ValueNode, nodes, values};
use crate::{Value, message::ToServer};

pub use super::bindings::cinnabar::experience_server::world_access::Callback as RawCallback;

/// Generated routing, kept separate from the authored server callbacks.
pub trait Dispatch {
    /// Rejects messages without a matching typed handler.
    fn dispatch(
        ctx: &Callback,
        player: PlayerId,
        channel: String,
        schema: u16,
        payload: Vec<Value>,
    ) -> Result<(), GuestError>;
}

/// Checks direction at compile time and revision and fields before calling authored code.
pub fn decode<M: ToServer>(schema: u16, payload: Vec<Value>) -> Result<M, GuestError> {
    if schema != M::CHANNEL.schema {
        return Err(GuestError::Rejected("unknown message schema".into()));
    }
    M::decode(payload).map_err(GuestError::Rejected)
}

/// Preserves registry negotiation and adapts the existing wire nodes to generated routing.
pub fn receive<T: Experience + Dispatch>(
    ctx: &RawCallback,
    player: PlayerId,
    channel: String,
    schema: u16,
    payload: Vec<ValueNode>,
) -> Result<(), GuestError> {
    let payload = values(payload)
        .ok_or_else(|| GuestError::Rejected("malformed client message payload".into()))?;
    if let Some(registry) = T::registry()
        && channel == registry.request.id
    {
        if schema != registry.request.schema {
            return Err(GuestError::Rejected("unknown registry schema".into()));
        }
        crate::message::validate(&registry.request, &payload).map_err(GuestError::Rejected)?;
        return ctx
            .send_control(
                &player,
                registry.channel.id,
                registry.channel.schema,
                &nodes(vec![Value::Text(registry.fingerprint.into())]),
            )
            .map_err(|_| GuestError::Failed("registry confirmation unavailable".into()));
    }
    T::dispatch(&Callback::from_raw(ctx), player, channel, schema, payload)
}

/// Exposes the canonical import only to explicit low-level ABI fixtures.
pub fn raw<'a>(ctx: &'a Callback<'a>) -> &'a RawCallback {
    ctx.0
}
