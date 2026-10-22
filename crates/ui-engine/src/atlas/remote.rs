//! Downloaded sources borrow the host's body and identify decoded artwork by its contents.

use std::borrow::Cow;

use sha2::{Digest, Sha256};

use super::{RemoteImage, ServerAtlas, Source, is_remote, source};

impl ServerAtlas {
    /// Parses the current body's header without retaining downloaded bytes or reusable addresses.
    pub(super) fn remote_source(&self, key: &str) -> Option<Source> {
        match self.remote.as_ref()?.state(key) {
            RemoteImage::Ready(bytes) => source(bytes),
            RemoteImage::Failed | RemoteImage::Loading => None,
        }
    }
}

/// Separates a URL's changed bodies in fitted pixels, failed decodes and worker completions.
pub(super) fn decode_key<'a>(key: &'a str, bytes: &[u8]) -> Cow<'a, str> {
    if is_remote(key) {
        Cow::Owned(format!("{key}#{:x}", Sha256::digest(bytes)))
    } else {
        Cow::Borrowed(key)
    }
}
