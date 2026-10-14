//! Generates an asset delivery index from the canonical carrier table.
use assets::carriers::startup::delivery::DeliveryManifest;

/// Writes only carrier identities and sizes; compiled game assets stay local.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: manifest <compiled-assets-directory>")?;
    let manifest = DeliveryManifest::from_directory(std::path::Path::new(&directory))?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
