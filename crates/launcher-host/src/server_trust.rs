//! Native server trust presentation and the caller's explicit answer.

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub use native::NativeSessionTrust;

use launcher::menu::view::ServerTrustPrompt;

/// Where the menu reads a pending trust question and sends its answer.
pub trait TrustSource {
    fn prompt(&self) -> Option<ServerTrustPrompt>;
    fn answer(&self, id: u64, trusted: bool);
}
