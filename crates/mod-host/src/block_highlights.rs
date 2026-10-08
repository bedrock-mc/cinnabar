//! Retained loaded-block selections, committed only after successful guest callbacks.
use super::{MAX_IMPORT_WRITES, State, cinnabar::extension::render as wit};
use anyhow::{Result, bail};
use mod_api::{
    BlockHighlightSpec, MAX_BLOCK_HIGHLIGHT_IDENTIFIER_BYTES, MAX_BLOCK_HIGHLIGHT_IDENTIFIERS,
    MAX_BLOCK_HIGHLIGHT_RANGE,
};

#[derive(Default)]
pub(super) struct HighlightState {
    pub committed: Option<BlockHighlightSpec>,
    pending: Option<Option<BlockHighlightSpec>>,
    writes: u32,
}

impl HighlightState {
    pub fn begin_frame(&mut self) {
        self.pending = None;
        self.writes = 0;
    }
    pub fn commit(&mut self) {
        if let Some(spec) = self.pending.take() {
            // Repeated identical requests retain the original strings and allocation.
            if self.committed != spec {
                self.committed = spec;
            }
        }
    }
    pub fn revoke(&mut self) {
        *self = Self::default();
    }
}

pub(super) fn set(
    state: &mut State,
    spec: Option<wit::BlockHighlightSpec>,
) -> Result<Result<(), String>> {
    state.block_highlights.writes += 1;
    if state.block_highlights.writes > MAX_IMPORT_WRITES {
        bail!("block highlight import budget exhausted");
    }
    if !state.grants.block_highlights {
        return Ok(Err("block highlights capability denied".into()));
    }
    let spec = match spec {
        None => None,
        Some(spec) => {
            let color = [spec.color.r, spec.color.g, spec.color.b, spec.color.a];
            if !(1..=MAX_BLOCK_HIGHLIGHT_IDENTIFIERS).contains(&spec.identifiers.len())
                || !spec.identifiers.iter().all(|name| identifier_valid(name))
                || !spec.range.is_finite()
                || !(1.0..=MAX_BLOCK_HIGHLIGHT_RANGE).contains(&spec.range)
                || !color
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            {
                return Ok(Err(
                    "block highlights need bounded names, range and RGBA colour".into(),
                ));
            }
            let mut identifiers = spec.identifiers;
            identifiers.sort_unstable();
            identifiers.dedup();
            Some(BlockHighlightSpec {
                identifiers,
                range: spec.range,
                color,
            })
        }
    };
    state.block_highlights.pending = Some(spec);
    Ok(Ok(()))
}

fn identifier_valid(name: &str) -> bool {
    if name.len() > MAX_BLOCK_HIGHLIGHT_IDENTIFIER_BYTES {
        return false;
    }
    let Some((namespace, path)) = name.split_once(':') else {
        return false;
    };
    let valid = |value: &str, slash: bool| {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
                    || (slash && byte == b'/')
            })
    };
    valid(namespace, false) && valid(path, true)
}

#[cfg(test)]
#[path = "block_highlights_tests.rs"]
mod tests;
