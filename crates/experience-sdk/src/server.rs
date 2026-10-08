//! The server half: a guest of the `server` world, which the Experience runtime runs as
//! `server.wasm`, generated from `wit/server/server.wit`.
//!
//! Implement [`Experience`] on a type and export it with
//! [`export_experience!`](crate::export_experience). Every callback receives a [`Callback`] that
//! is valid only for that call. [`Callback::send_client`] stages a typed message for the actor's
//! client part; build its payload as [`Value`]s and pass their [`nodes`].
//! [`Experience::client_message`] receives what that client part sends back, and
//! [`Experience::epoch`] when it moved to a new world epoch; both may read and write the block
//! of the player's focus, [`Callback::focus`].

/// Bindings generated from `wit/server/server.wit`.
// The canonical-ABI shims for `on-place` and `on-break` take the flattened
// `block-change` record, which exceeds Clippy's argument limit.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit/server",
        world: "server",
        pub_export_macro: true,
    });
}

pub use bindings::cinnabar::experience_server::{
    diagnostics::log,
    types::{
        BlockChange, BlockDef, BlockPos, CallbackInfo, ChangeCause, Face, GuestError, LogLevel,
        Mining, PlayerId, Scalar, TextureBinding, ValueNode, WorldError,
    },
    world_access::Callback,
};
pub use nodes::{nodes, values};

use crate::Value;

mod nodes;

/// One server Experience, mirroring the exports of the `server` world.
///
/// Only [`Experience::register`] is required; the callbacks default to `Ok(())`, which accepts
/// the event without staging anything.
pub trait Experience {
    /// Declares the blocks this Experience owns. It runs once, at startup.
    fn register() -> Result<Vec<BlockDef>, GuestError>;

    /// Handles `on-place` for one of this Experience's blocks.
    fn on_place(_ctx: &Callback, _change: BlockChange) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-break` for one of this Experience's blocks.
    fn on_break(_ctx: &Callback, _change: BlockChange) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-interact`: `player` used the block at `pos`.
    fn on_interact(
        _ctx: &Callback,
        _player: PlayerId,
        _pos: BlockPos,
        _clicked_face: Face,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-neighbor-changed`: the block at `neighbor`, next to `pos`, changed.
    fn on_neighbor_changed(
        _ctx: &Callback,
        _pos: BlockPos,
        _neighbor: BlockPos,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `client-message`: `player`'s client part sent `payload` on `channel`, revision
    /// `schema`. [`Callback::focus`] is the block of `player`'s focus, the block of this
    /// Experience that `player` last used, if it is still valid: `ctx` then has its snapshot and
    /// reads and writes it as [`Experience::on_interact`] would. Without a focus `ctx` reads and
    /// writes no blocks. Either way it may `tell` and `send-client` to `player`.
    fn client_message(
        _ctx: &Callback,
        _player: PlayerId,
        _channel: String,
        _schema: u16,
        _payload: Vec<Value>,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `epoch`: `player`'s client part moved to a new world epoch, such as another
    /// dimension, and kept running, so it may have missed what was sent before; resend its state.
    /// `ctx` has the snapshot of `player`'s focus like [`Experience::client_message`]'s.
    fn epoch(_ctx: &Callback, _player: PlayerId) -> Result<(), GuestError> {
        Ok(())
    }
}

/// Implements the generated [`bindings::Guest`] for `$ty` by delegating to its
/// [`Experience`] impl, then exports `$ty` as the component's `server` world.
#[macro_export]
macro_rules! export_experience {
    ($ty:ident) => {
        impl $crate::server::bindings::Guest for $ty {
            fn register() -> ::core::result::Result<
                ::std::vec::Vec<$crate::server::BlockDef>,
                $crate::server::GuestError,
            > {
                <$ty as $crate::server::Experience>::register()
            }

            fn on_place(
                ctx: &$crate::server::Callback,
                change: $crate::server::BlockChange,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_place(ctx, change)
            }

            fn on_break(
                ctx: &$crate::server::Callback,
                change: $crate::server::BlockChange,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_break(ctx, change)
            }

            fn on_interact(
                ctx: &$crate::server::Callback,
                player: $crate::server::PlayerId,
                pos: $crate::server::BlockPos,
                clicked_face: $crate::server::Face,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_interact(ctx, player, pos, clicked_face)
            }

            fn on_neighbor_changed(
                ctx: &$crate::server::Callback,
                pos: $crate::server::BlockPos,
                neighbor: $crate::server::BlockPos,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_neighbor_changed(ctx, pos, neighbor)
            }

            fn client_message(
                ctx: &$crate::server::Callback,
                player: $crate::server::PlayerId,
                channel: ::std::string::String,
                schema: u16,
                payload: ::std::vec::Vec<$crate::server::ValueNode>,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                // The runtime delivers only well-formed payloads.
                let payload = $crate::server::values(payload).ok_or_else(|| {
                    $crate::server::GuestError::Failed("malformed client message payload".into())
                })?;
                <$ty as $crate::server::Experience>::client_message(
                    ctx, player, channel, schema, payload,
                )
            }

            fn epoch(
                ctx: &$crate::server::Callback,
                player: $crate::server::PlayerId,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::epoch(ctx, player)
            }
        }

        $crate::server::bindings::export!($ty with_types_in $crate::server::bindings);
    };
}
