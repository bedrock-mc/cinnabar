//! Content-pinned native material witnesses, shared by admission and GPU routing.
use std::sync::OnceLock;

use crate::{EntityAssetSource, RuntimeEntityAssets};

#[derive(serde::Deserialize)]
struct Policy {
    color_mask_sources: Vec<Source>,
    multitexture_sources: Vec<Source>,
    multitexture_entities: Vec<Source>,
    multitexture_controllers: Vec<Source>,
    blended_sources: Vec<Source>,
}

#[derive(serde::Deserialize)]
struct Source {
    path: Box<str>,
    source_sha256: Box<str>,
}

/// Only the exact witnessed vanilla rasters may bypass binary-opacity admission. A custom
/// texture at the same path does not inherit a material contract from its filename.
pub fn native_actor_texture_uses_color_mask(source: &EntityAssetSource) -> bool {
    matches(&policy().color_mask_sources, source)
}

/// Native three-sampler texture admission, independently pinned from dye masks.
pub fn native_actor_texture_uses_multitexture(source: &EntityAssetSource) -> bool {
    matches(&policy().multitexture_sources, source)
}

/// Native rasters with an identified material that preserves fractional alpha.
pub fn native_actor_texture_preserves_fractional_alpha(source: &EntityAssetSource) -> bool {
    native_actor_texture_uses_color_mask(source)
        || native_actor_texture_uses_multitexture(source)
        || matches(&policy().blended_sources, source)
}

/// Only the witnessed vanilla entity/controller pair can group texture slots. Reusing the
/// same raster in a custom material does not turn that controller into a three-sampler draw.
pub fn native_actor_uses_multitexture(entities: &RuntimeEntityAssets, binding: usize) -> bool {
    let Some(rig) = entities.rig_bindings().get(binding) else {
        return false;
    };
    let source = |symbol: u32| {
        let symbol = entities.symbols().get(symbol as usize)?;
        entities.sources().get(symbol.source_index as usize)
    };
    source(rig.entity_symbol).is_some_and(|source| matches(&policy().multitexture_entities, source))
        && source(rig.render_controller)
            .is_some_and(|source| matches(&policy().multitexture_controllers, source))
}

fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        serde_json::from_slice(super::POLICY).expect("embedded actor material policy is valid")
    })
}

fn matches(witnesses: &[Source], source: &EntityAssetSource) -> bool {
    witnesses.iter().any(|witness| {
        witness.path == source.path
            && witness.source_sha256.len() == source.source_sha256.len() * 2
            && witness
                .source_sha256
                .as_bytes()
                .chunks_exact(2)
                .zip(source.source_sha256)
                .all(|(hex, byte)| {
                    let digit = |value: u8| match value {
                        b'0'..=b'9' => Some(value - b'0'),
                        b'a'..=b'f' => Some(value - b'a' + 10),
                        _ => None,
                    };
                    digit(hex[0])
                        .zip(digit(hex[1]))
                        .is_some_and(|(a, b)| (a << 4) | b == byte)
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarly_named_custom_rasters_cannot_claim_native_mask_alpha() {
        let policy: Policy = serde_json::from_slice(super::super::POLICY).unwrap();
        for witness in policy.color_mask_sources {
            let source = EntityAssetSource {
                path: witness.path,
                source_bytes: 0,
                source_sha256: [0; 32],
            };
            assert!(!native_actor_texture_uses_color_mask(&source));
        }
    }

    #[test]
    fn similarly_named_custom_rasters_cannot_claim_native_multitexture_alpha() {
        for witness in &policy().multitexture_sources {
            let source = EntityAssetSource {
                path: witness.path.clone(),
                source_bytes: 0,
                source_sha256: [0; 32],
            };
            assert!(!native_actor_texture_uses_multitexture(&source));
        }
    }

    #[test]
    fn similarly_named_custom_rasters_cannot_claim_native_blended_alpha() {
        for witness in &policy().blended_sources {
            let source = EntityAssetSource {
                path: witness.path.clone(),
                source_bytes: 0,
                source_sha256: [0; 32],
            };
            assert!(!native_actor_texture_preserves_fractional_alpha(&source));
        }
    }
}
