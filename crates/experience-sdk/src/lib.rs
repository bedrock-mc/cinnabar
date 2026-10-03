//! Guest SDK for Cinnabar server Experiences, generated from `wit/server.wit`.
//!
//! Implement [`Experience`] on a type and export it with [`export_experience!`].
//! Every callback receives a [`Callback`] that is valid only for that call.
//! [`Callback::send_client`] stages a typed message for the actor's client part;
//! [`Experience::client_message`] receives what that client part sends back.

/// Bindings generated from `wit/server.wit`.
// The canonical-ABI shims for `on-place` and `on-break` take the flattened
// `block-change` record, which exceeds Clippy's argument limit.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "server",
        pub_export_macro: true,
    });
}

pub use bindings::cinnabar::experience_server::{
    diagnostics::log,
    types::{
        BlockChange, BlockDef, BlockPos, CallbackInfo, ChangeCause, Face, GuestError, LogLevel,
        Mining, PlayerId, Scalar, TextureBinding, WorldError,
    },
    world_access::Callback,
};

/// One server Experience, mirroring the exports of the `server` world.
///
/// Only [`Experience::register`] is required; the block callbacks default to
/// `Ok(())`, which accepts the event without staging anything.
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
    /// `schema`. `ctx` has no snapshot, so it reads and writes no blocks; it may `tell` and
    /// `send-client` to `player`.
    fn client_message(
        _ctx: &Callback,
        _player: PlayerId,
        _channel: String,
        _schema: u16,
        _payload: Vec<Scalar>,
    ) -> Result<(), GuestError> {
        Ok(())
    }
}

/// Implements the generated [`bindings::Guest`] for `$ty` by delegating to its
/// [`Experience`] impl, then exports `$ty` as the component's `server` world.
#[macro_export]
macro_rules! export_experience {
    ($ty:ident) => {
        impl $crate::bindings::Guest for $ty {
            fn register() -> ::core::result::Result<
                ::std::vec::Vec<$crate::BlockDef>,
                $crate::GuestError,
            > {
                <$ty as $crate::Experience>::register()
            }

            fn on_place(
                ctx: &$crate::Callback,
                change: $crate::BlockChange,
            ) -> ::core::result::Result<(), $crate::GuestError> {
                <$ty as $crate::Experience>::on_place(ctx, change)
            }

            fn on_break(
                ctx: &$crate::Callback,
                change: $crate::BlockChange,
            ) -> ::core::result::Result<(), $crate::GuestError> {
                <$ty as $crate::Experience>::on_break(ctx, change)
            }

            fn on_interact(
                ctx: &$crate::Callback,
                player: $crate::PlayerId,
                pos: $crate::BlockPos,
                clicked_face: $crate::Face,
            ) -> ::core::result::Result<(), $crate::GuestError> {
                <$ty as $crate::Experience>::on_interact(ctx, player, pos, clicked_face)
            }

            fn on_neighbor_changed(
                ctx: &$crate::Callback,
                pos: $crate::BlockPos,
                neighbor: $crate::BlockPos,
            ) -> ::core::result::Result<(), $crate::GuestError> {
                <$ty as $crate::Experience>::on_neighbor_changed(ctx, pos, neighbor)
            }

            fn client_message(
                ctx: &$crate::Callback,
                player: $crate::PlayerId,
                channel: ::std::string::String,
                schema: u16,
                payload: ::std::vec::Vec<$crate::Scalar>,
            ) -> ::core::result::Result<(), $crate::GuestError> {
                <$ty as $crate::Experience>::client_message(ctx, player, channel, schema, payload)
            }
        }

        $crate::bindings::export!($ty with_types_in $crate::bindings);
    };
}
