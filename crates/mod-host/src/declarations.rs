//! A component's own requests and Mods-library metadata, read by parsing its sections only.

use crate::{MAX_COMPONENT_SOURCE_BYTES, ModGrants, local_files};
use anyhow::{Result, anyhow, ensure};
use std::{io::Cursor, path::Path, sync::Arc};
use ui::mod_metadata::{GitHubRepository, MAX_VERSION_BYTES, ModMetadata, ModUpdateUrl};
use wasmparser::{ComponentName, KnownCustom, Name, Parser, Payload};

/// The optional top-level component custom section containing a JSON grant request.
pub const REQUESTED_GRANTS_SECTION: &str = "cinnabar:grants";
const MAX_REQUESTED_GRANTS_BYTES: usize = 16 * 1024;
/// Byte bounds of the standard metadata fields; a longer field is ignored, not truncated.
const MAX_NAME_BYTES: usize = 64;
const MAX_DESCRIPTION_BYTES: usize = 512;
const MAX_AUTHORS_BYTES: usize = 256;
const MAX_LICENSES_BYTES: usize = 128;

/// What one component file declares about itself.
pub struct Declarations {
    /// Absence means undeclared, rather than a request for every supported capability.
    /// A malformed request stays an error so it never reads as undeclared.
    pub requested: Result<Option<ModGrants>>,
    /// Malformed or oversized fields and icons are left empty.
    pub metadata: ModMetadata,
}

/// Reads a component's declarations in one bounded read, without instantiating or granting it.
/// Only an unreadable file or malformed module framing is an error.
pub fn declarations(path: &Path) -> Result<Declarations> {
    let bytes = local_files::read(path, MAX_COMPONENT_SOURCE_BYTES)?;
    declarations_snapshot(&bytes)
}

/// Parses declarations from the exact bytes selected for admission.
pub fn declarations_snapshot(bytes: &[u8]) -> Result<Declarations> {
    ensure!(
        bytes.len() <= MAX_COMPONENT_SOURCE_BYTES,
        "component declarations exceed byte limit"
    );
    let mut metadata = ModMetadata::default();
    // Developer text fixtures do not contain binary custom sections.
    if !bytes.starts_with(b"\0asm") {
        return Ok(Declarations {
            requested: Ok(None),
            metadata,
        });
    }
    let mut depth = 0usize;
    let mut grants = Vec::new();
    let mut icons = Vec::new();
    let mut update_repositories = Vec::new();
    let mut update_urls = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        let section = match payload? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => {
                depth += 1;
                continue;
            }
            Payload::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Payload::CustomSection(section) if depth == 0 => section,
            _ => continue,
        };
        // Standard fields follow `wasm-metadata`: the last top-level occurrence wins.
        let data = section.data();
        match (section.name(), section.as_known()) {
            (_, KnownCustom::ComponentName(names)) => {
                for name in names {
                    match name {
                        Ok(ComponentName::Component { name, .. }) => {
                            metadata.name = text(name.as_bytes(), MAX_NAME_BYTES);
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            (_, KnownCustom::Name(names)) => {
                for name in names {
                    match name {
                        Ok(Name::Module { name, .. }) => {
                            metadata.name = text(name.as_bytes(), MAX_NAME_BYTES);
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            ("description", _) => metadata.description = text(data, MAX_DESCRIPTION_BYTES),
            ("version", _) => metadata.version = text(data, MAX_VERSION_BYTES),
            ("authors", _) => metadata.authors = text(data, MAX_AUTHORS_BYTES),
            ("licenses", _) => metadata.licenses = text(data, MAX_LICENSES_BYTES),
            (REQUESTED_GRANTS_SECTION, _) => grants.push(data),
            (mod_api::ICON_SECTION, _) => icons.push(data),
            (mod_api::UPDATE_REPOSITORY_SECTION, _) => update_repositories.push(data),
            (mod_api::UPDATE_URL_SECTION, _) => update_urls.push(data),
            _ => {}
        }
    }
    if let [icon] = icons.as_slice() {
        metadata.icon = valid_icon(icon).then(|| Arc::from(*icon));
    }
    if let [repository] = update_repositories.as_slice() {
        metadata.update_repository = std::str::from_utf8(repository)
            .ok()
            .and_then(GitHubRepository::parse);
    }
    if let [url] = update_urls.as_slice() {
        metadata.update_url = std::str::from_utf8(url).ok().and_then(ModUpdateUrl::parse);
    }
    let requested = match grants.as_slice() {
        [] => Ok(None),
        [data] => requested(data).map(Some),
        _ => Err(anyhow!("duplicate requested grants section")),
    };
    Ok(Declarations {
        requested,
        metadata,
    })
}

/// Parses and validates one bounded JSON grant request.
fn requested(data: &[u8]) -> Result<ModGrants> {
    ensure!(
        data.len() <= MAX_REQUESTED_GRANTS_BYTES,
        "requested grants exceed byte limit"
    );
    let grants: ModGrants = serde_json::from_slice(data)?;
    grants.validate()?;
    Ok(grants)
}

/// One display line: whitespace runs collapse, and other control characters reject the field.
fn text(data: &[u8], max: usize) -> Option<String> {
    if data.len() > max {
        return None;
    }
    let line = std::str::from_utf8(data)
        .ok()?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!line.is_empty() && !line.chars().any(char::is_control)).then_some(line)
}

/// Whether the icon is a PNG within its byte and side limits that decodes completely.
fn valid_icon(data: &[u8]) -> bool {
    if data.len() > mod_api::MAX_ICON_BYTES {
        return false;
    }
    let side = mod_api::MAX_ICON_SIDE;
    let mut decoder = png::Decoder::new(Cursor::new(data));
    // Sixteen-bit RGBA at the largest accepted side.
    decoder.set_limits(png::Limits {
        bytes: side as usize * side as usize * 8,
    });
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > side || height > side {
        return false;
    }
    let Some(length) = reader.output_buffer_size() else {
        return false;
    };
    let mut pixels = vec![0; length];
    reader.next_frame(&mut pixels).is_ok()
}

#[cfg(test)]
mod tests;
