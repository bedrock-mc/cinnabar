//! The author SDK for Cinnabar Experiences: one crate for both halves, each a separate component.
//!
//! - [`server`] (feature `server`): the server half, a `server.wasm` that the Experience runtime
//!   runs, generated from `wit/server/server.wit`. Implement [`server::Experience`] and export it
//!   with [`export_experience!`].
//! - [`client`] (feature `client`): the client part, the component of the Experience's `.cxb`
//!   that Cinnabar runs, generated from `../mod-api/wit/client.wit`. Implement
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
#[cfg(feature = "declarations")]
pub mod mod_manifest;
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

/// A generated signed-bundle asset identity shared by the two project halves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asset(&'static str);

impl Asset {
    /// Binds one manifest path in generated declarations.
    pub const fn new(path: &'static str) -> Self {
        Self(path)
    }

    /// Reads one asset range; use successive callbacks to stream files larger than the allowance.
    #[cfg(feature = "client")]
    pub fn read_range(
        self,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, client::drawing::DrawError> {
        client::assets::read_range(self.0, offset, length)
    }

    /// Reads the verified length without copying the file.
    #[cfg(feature = "client")]
    pub fn len(self) -> Result<u64, client::drawing::DrawError> {
        client::assets::length(self.0)
    }

    /// Reports whether the verified file is empty.
    #[cfg(feature = "client")]
    pub fn is_empty(self) -> Result<bool, client::drawing::DrawError> {
        self.len().map(|length| length == 0)
    }

    /// Reads verified bytes through the client drawing host.
    #[cfg(feature = "client")]
    pub fn read(self) -> Result<Vec<u8>, client::drawing::DrawError> {
        client::assets::read(self.0)
    }
}
