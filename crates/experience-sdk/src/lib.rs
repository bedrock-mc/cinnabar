//! The author SDK for Cinnabar Experiences: one crate for both halves, each a separate component.
//!
//! - [`server`] (feature `server`): the server half, a `server.wasm` that the Experience runtime
//!   runs, generated from `wit/server/server.wit`. Implement [`server::Experience`] and export it
//!   with [`export_experience!`].
//! - [`client`] (feature `client`): the client part, the component of the Experience's `.cxb`
//!   that Cinnabar runs, generated from `wit/client/client.wit`. Implement
//!   [`client::ClientPart`] and export it with [`export_client_part!`].
//! - [`declarations`] (feature `declarations`): for a guest's build script, the channels, actions
//!   and templates that `experience.toml` declares, as constants that [`include_declarations!`]
//!   brings into the guest.
//!
//! Both halves share [`Value`], one value of a channel record, and [`Channel`], a channel as
//! `experience.toml` declares it.
//!
//! Every feature is on by default, so the SDK's own build, tests and docs cover all of it. A
//! crate that uses the SDK turns the defaults off and names what it uses: a guest `server` or
//! `client`, and its build script `declarations`.

pub use channel::{Channel, Direction, Field};
pub use value::Value;

mod channel;
mod value;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "declarations")]
pub mod declarations;
#[cfg(feature = "server")]
pub mod server;

/// Includes the constants that `declarations::generate` wrote in the guest's build script: the
/// modules `channels`, `actions` and `templates`. Invoke it inside a module of its own.
#[macro_export]
macro_rules! include_declarations {
    () => {
        include!(concat!(env!("OUT_DIR"), "/", $crate::__declarations_rs!()));
    };
}

/// The file in `OUT_DIR` that `declarations::generate` writes and [`include_declarations!`]
/// includes.
#[doc(hidden)]
#[macro_export]
macro_rules! __declarations_rs {
    () => {
        "experience.rs"
    };
}
