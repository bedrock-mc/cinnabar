//! The browser keeps the login ID in the page's local storage instead of the user data folder.

use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc,
};

use protocol::ClientRandomId;

/// The local storage key; every tab of the page's origin shares the one ID.
const STORAGE_KEY: &str = "cinnabar.clientId";

/// Loads at once, as the page has no worker thread for it; the receiver already holds the result.
pub fn load_async(root: PathBuf) -> io::Result<mpsc::Receiver<io::Result<ClientRandomId>>> {
    let (sender, receiver) = mpsc::channel();
    let _ = sender.send(load(&root));
    Ok(receiver)
}

/// Reuses the stored nonzero ID or saves a new one from the browser's cryptographic source.
/// Where local storage is refused, as in some private windows, the ID lasts for this page only.
pub fn load(_root: &Path) -> io::Result<ClientRandomId> {
    let window = web_sys::window().ok_or_else(|| io::Error::other("no browser window"))?;
    let storage = window.local_storage().ok().flatten();
    if let Some(id) = storage
        .as_ref()
        .and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|text| text.trim().parse::<i64>().ok())
        .and_then(ClientRandomId::new)
    {
        return Ok(id);
    }
    let crypto = window
        .crypto()
        .map_err(|_| io::Error::other("the browser has no cryptographic random source"))?;
    let id = loop {
        let mut bytes = [0; 8];
        crypto
            .get_random_values_with_u8_array(&mut bytes)
            .map_err(|_| io::Error::other("the browser's random source failed"))?;
        if let Some(id) = ClientRandomId::new(i64::from_le_bytes(bytes)) {
            break id;
        }
    };
    if let Some(storage) = storage {
        let _ = storage.set_item(STORAGE_KEY, &id.get().to_string());
    }
    Ok(id)
}
