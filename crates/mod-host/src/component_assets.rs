//! Optional static presentation data admitted separately from executable component bytes.

use crate::{MAX_COMPONENT_BYTES, MAX_COMPONENT_SOURCE_BYTES};
use anyhow::{Result, bail, ensure};
use std::{borrow::Cow, collections::BTreeSet, sync::Arc};
use ui::mod_panel::PanelBundle;
use wasmparser::{Encoding, Parser, Payload};

pub(super) struct ComponentAssets<'a> {
    pub(super) executable: Cow<'a, [u8]>,
    pub(super) panel_bundle: Option<Arc<PanelBundle>>,
    pub(super) font: Option<Arc<[u8]>>,
    pub(super) logo: Option<Arc<[u8]>>,
    pub(super) title: Option<Arc<str>>,
    pub(super) notices: Option<Arc<str>>,
}

/// Accounts for each recognized section's ID and two bounded u32 lengths without accepting
/// extra executable metadata through the source envelope.
pub(super) const MAX_SECTION_FRAMING_BYTES: usize = {
    let names = mod_api::COMPONENT_ASSET_SECTIONS;
    let mut size = 0;
    let mut index = 0;
    while index < names.len() {
        size += names[index].len() + 11;
        index += 1;
    }
    size
};

impl<'a> ComponentAssets<'a> {
    /// Directory packages retain their existing asset lifecycle and executable ceiling.
    /// Embedded presentation resources belong to the separately reloadable single-file form.
    pub(super) fn read_package(bytes: &'a [u8]) -> Result<Self> {
        let assets = Self::read(bytes)?;
        ensure!(
            assets.executable.len() == bytes.len(),
            "embedded presentation resources require a single-file component, not a directory package"
        );
        Ok(assets)
    }

    /// Validates singleton top-level assets and excludes only those sections from compilation.
    /// Unknown custom sections remain executable metadata subject to the original byte ceiling.
    pub(super) fn read(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_COMPONENT_SOURCE_BYTES,
            "component source exceeds byte limit"
        );
        let mut assets = Self {
            executable: Cow::Borrowed(bytes),
            panel_bundle: None,
            font: None,
            logo: None,
            title: None,
            notices: None,
        };
        // Wasmtime also accepts developer WAT fixtures. They have no binary asset sections.
        if !bytes.starts_with(b"\0asm") {
            ensure!(
                bytes.len() <= MAX_COMPONENT_BYTES,
                "component exceeds byte limit"
            );
            return Ok(assets);
        }
        let mut depth = 0usize;
        let mut component = false;
        let mut removed = BTreeSet::new();
        for payload in Parser::new(0).parse_all(bytes) {
            match payload? {
                Payload::Version { encoding, .. } if depth == 0 => {
                    component = encoding == Encoding::Component;
                }
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) => depth = depth.saturating_sub(1),
                Payload::CustomSection(section) => {
                    let name = section.name();
                    if !mod_api::COMPONENT_ASSET_SECTIONS.contains(&name) {
                        ensure!(
                            !mod_api::COMPONENT_ASSET_SECTIONS
                                .iter()
                                .any(|known| name.starts_with(
                                    known.rsplit_once('@').expect("versioned asset name").0
                                )),
                            "unsupported embedded presentation asset version"
                        );
                        continue;
                    }
                    ensure!(
                        component && depth == 0,
                        "presentation assets must be top-level component sections"
                    );
                    let data = section.data();
                    if name == mod_api::PANEL_BUNDLE_SECTION {
                        ensure!(
                            assets.panel_bundle.is_none(),
                            "duplicate embedded panel bundle"
                        );
                        ensure!(
                            data.len() <= ui::mod_panel::MAX_PANEL_BUNDLE_BYTES,
                            "embedded panel bundle exceeds byte limit"
                        );
                        let json = std::str::from_utf8(data)?;
                        assets.panel_bundle = Some(Arc::new(
                            PanelBundle::parse(json).map_err(anyhow::Error::msg)?,
                        ));
                    } else if name == mod_api::PANEL_FONT_SECTION {
                        ensure!(assets.font.is_none(), "duplicate embedded panel font");
                        ensure!(
                            !data.is_empty() && data.len() <= mod_api::MAX_PANEL_FONT_BYTES,
                            "embedded panel font exceeds byte limit"
                        );
                        assets.font = Some(Arc::from(data));
                    } else if name == mod_api::MENU_LOGO_SECTION {
                        ensure!(assets.logo.is_none(), "duplicate embedded menu logo");
                        ensure!(
                            !data.is_empty() && data.len() <= mod_api::MAX_MENU_LOGO_BYTES,
                            "embedded menu logo exceeds byte limit"
                        );
                        assets.logo = Some(Arc::from(data));
                    } else if name == mod_api::MENU_TITLE_SECTION {
                        ensure!(assets.title.is_none(), "duplicate embedded menu title");
                        ensure!(
                            !data.is_empty() && data.len() <= mod_api::MAX_MENU_TITLE_BYTES,
                            "embedded menu title exceeds byte limit"
                        );
                        let title = std::str::from_utf8(data)?;
                        ensure!(
                            !title.trim().is_empty() && !title.chars().any(char::is_control),
                            "embedded menu title must be a nonempty line"
                        );
                        assets.title = Some(Arc::from(title));
                    } else {
                        ensure!(assets.notices.is_none(), "duplicate embedded notices");
                        ensure!(
                            !data.is_empty() && data.len() <= mod_api::MAX_NOTICES_BYTES,
                            "embedded notices exceed byte limit"
                        );
                        assets.notices = Some(Arc::from(std::str::from_utf8(data)?));
                    }
                    removed.insert(section.range().start);
                }
                _ => {}
            }
        }
        if !removed.is_empty() {
            assets.executable = Cow::Owned(strip_sections(bytes, &removed)?);
        }
        ensure!(
            assets.executable.len() <= MAX_COMPONENT_BYTES,
            "component exceeds executable byte limit"
        );
        Ok(assets)
    }
}

/// Copies original section framing verbatim, including non-minimal valid length encodings.
fn strip_sections(bytes: &[u8], removed: &BTreeSet<usize>) -> Result<Vec<u8>> {
    ensure!(bytes.len() >= 8, "truncated component header");
    let mut code = bytes[..8].to_vec();
    let mut cursor = 8usize;
    let mut stripped = 0usize;
    while cursor < bytes.len() {
        let start = cursor;
        let id = bytes[cursor];
        cursor += 1;
        let length = section_length(bytes, &mut cursor)?;
        let data_start = cursor;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| anyhow::anyhow!("truncated component section"))?;
        if removed.contains(&data_start) {
            ensure!(id == 0, "embedded asset is not a custom section");
            stripped += 1;
        } else {
            code.extend_from_slice(&bytes[start..end]);
        }
        cursor = end;
    }
    ensure!(
        stripped == removed.len(),
        "embedded section framing differs"
    );
    Ok(code)
}

/// Reads one bounded unsigned WebAssembly length after the standard parser admitted framing.
fn section_length(bytes: &[u8], cursor: &mut usize) -> Result<usize> {
    let mut value = 0u32;
    for index in 0..5 {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| anyhow::anyhow!("truncated section length"))?;
        *cursor += 1;
        ensure!(
            index != 4 || byte & 0xf0 == 0,
            "section length overflows u32"
        );
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(value as usize);
        }
    }
    bail!("section length is too long")
}

#[cfg(test)]
mod tests;
