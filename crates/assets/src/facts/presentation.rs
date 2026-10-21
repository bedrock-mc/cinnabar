//! Presentation identities and sign texture grammar compiled from the shared policy.

use super::presentation_generated::{SIGN_SUFFIXES, SIGN_WOOD_ALIASES, SIGN_WOOD_FIRST};

/// A legacy block callback, admitted before fixed-tick sampling.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AmbientBlock {
    #[default]
    Other,
    Fire,
    Portal,
}

/// Resolves a leaf species' authored effect through the pinned particle vocabulary.
/// Unrelated blocks and leaf species without a dedicated effect remain unresolved.
pub fn leaf_particle(identifier: &str) -> Option<super::ParticleEffect> {
    identifier.strip_suffix("_leaves")?;
    super::ParticleEffect::from_name(&format!("{identifier}_particle"))
}

/// Classifies the pinned vocabulary's dedicated and biome-tinted falling-leaf effects.
pub fn is_leaf_particle(effect: super::ParticleEffect) -> bool {
    effect.name().ends_with("leaves_particle")
}

/// Resolves sign editor artwork using the compiled texture naming contract.
pub fn sign_texture(identifier: Option<&str>) -> (String, bool) {
    let name = identifier.unwrap_or_default();
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let hanging = name.ends_with(SIGN_SUFFIXES[1]);
    let mut wood = name;
    for suffix in SIGN_SUFFIXES {
        wood = wood.trim_end_matches(suffix);
    }
    let wood = SIGN_WOOD_ALIASES
        .iter()
        .find(|(name, _)| *name == wood)
        .map_or(wood, |(_, alias)| *alias);
    let texture = if hanging {
        if wood.is_empty() {
            "hanging_sign".to_owned()
        } else {
            format!("hanging_sign_{wood}")
        }
    } else if wood.is_empty() {
        "sign".to_owned()
    } else if SIGN_WOOD_FIRST.contains(&wood) {
        format!("{wood}_sign")
    } else {
        format!("sign_{wood}")
    };
    (format!("textures/ui/{texture}"), hanging)
}

#[cfg(test)]
mod tests;
