//! Packs a session adds beside its server stack, admitted apart so they never fail it.

use crate::{
    AdmissionError, LibraryError, ValidatedPack, ValidatedPackStack,
    import::read_import,
    manifest::Version,
    parser::validate_archive_parts,
    subpacks::{device_memory_tier, select},
};
use std::{collections::HashSet, path::Path};

/// The name added archives are read under; it only labels errors.
const SOURCE: &str = "session-pack.mcpack";

/// One unencrypted pack archive a session adds.
pub struct AddedPack {
    pub archive: Vec<u8>,
    /// Applies above the server's packs rather than below them.
    pub above_server: bool,
}

/// Composes `added`, listed highest priority first, around `server`: the packs below the
/// server's, then the server's own, then the packs above. Each archive is admitted under its own
/// manifest identity with the device's subpack. One that fails admission, repeats an identity
/// already in the stack, or would exceed the stack's bounds is left out with its reason, so the
/// server's packs and rejections always pass through unchanged.
pub fn compose_added_packs(
    server: &ValidatedPackStack,
    added: Vec<AddedPack>,
    physical_memory_bytes: u64,
) -> (ValidatedPackStack, Vec<LibraryError>) {
    let memory = device_memory_tier(physical_memory_bytes);
    let mut seen: HashSet<_> = server.packs().iter().map(ValidatedPack::pack_id).collect();
    let (mut below, mut above) = (Vec::new(), Vec::new());
    let mut refused = Vec::new();
    // Higher priority comes first and wins a repeated identity; each kept layer goes beneath
    // those kept before it.
    for pack in added {
        let layers = match admit(pack.archive, memory) {
            Ok(layers) => layers,
            Err(error) => {
                refused.push(error);
                continue;
            }
        };
        for layer in layers {
            let id = layer.pack_id();
            if !seen.insert(id) {
                refused.push(AdmissionError::DuplicatePack.into());
                continue;
            }
            let side = if pack.above_server {
                &mut above
            } else {
                &mut below
            };
            side.insert(0, layer);
            if let Err(error) = assemble(&below, server, &above) {
                if pack.above_server {
                    above.remove(0);
                } else {
                    below.remove(0);
                }
                seen.remove(&id);
                refused.push(error.into());
            }
        }
    }
    // The kept packs are exactly those of the last layering that fit.
    let stack = assemble(&below, server, &above).expect("every kept pack fits the bounds");
    (stack, refused)
}

/// Admits every resource pack an archive carries.
fn admit(archive: Vec<u8>, memory: u32) -> Result<Vec<ValidatedPack>, LibraryError> {
    let (_, archives) = read_import(archive, Path::new(SOURCE))?;
    archives
        .into_iter()
        .map(|(metadata, bytes)| {
            let subpack = select(&metadata.subpacks, memory);
            let (pack, _) = validate_archive_parts(
                metadata.id,
                Version(metadata.version),
                subpack,
                bytes,
                None,
                None,
            )?;
            Ok(pack)
        })
        .collect()
}

/// Layers the kept packs around the server stack, checking the whole stack's bounds.
fn assemble(
    below: &[ValidatedPack],
    server: &ValidatedPackStack,
    above: &[ValidatedPack],
) -> Result<ValidatedPackStack, AdmissionError> {
    let layer = |packs: &[ValidatedPack]| ValidatedPackStack {
        packs: packs.into(),
        rejections: Box::default(),
        fingerprint: Default::default(),
    };
    let lower = ValidatedPackStack::compose(&layer(below), server)?;
    ValidatedPackStack::compose(&lower, &layer(above))
}

#[cfg(all(test, feature = "handoff"))]
mod tests;
