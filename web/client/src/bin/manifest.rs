//! Generates an asset delivery index from the canonical carrier table.
#![expect(
    clippy::print_stdout,
    reason = "the manifest tool writes the delivery index JSON to stdout"
)]
use assets::carriers::{self, startup::delivery::DeliveryManifest};
use std::path::PathBuf;

/// Writes only carrier identities and sizes; compiled game assets stay local.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(
        args.next()
            .ok_or("usage: manifest <compiled-assets-directory> [--copy-to <directory>]")?,
    );
    let directory = match (args.next(), args.next(), args.next()) {
        (None, None, None) => directory,
        (Some(flag), Some(destination), None) if flag == "--copy-to" => {
            let destination = PathBuf::from(destination);
            // Check admission before copying, then hash the delivered files themselves.
            let manifest = DeliveryManifest::from_directory(&directory)?;
            std::fs::create_dir(&destination)?;
            // The sound bank is served by ranges, outside the eager-download index.
            let bank = std::iter::once(&carriers::AUDIO_BANK)
                .filter(|carrier| directory.join(carrier.output).is_file());
            for carrier in manifest.carriers().chain(bank) {
                std::fs::copy(
                    directory.join(carrier.output),
                    destination.join(carrier.output),
                )?;
            }
            destination
        }
        _ => {
            return Err(
                "usage: manifest <compiled-assets-directory> [--copy-to <directory>]".into(),
            );
        }
    };
    let manifest = DeliveryManifest::from_directory(&directory)?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
