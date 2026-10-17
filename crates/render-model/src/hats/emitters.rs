//! Particle emitters authored in a cosmetic animation and placed on its posed bones.
use super::*;
use serde::Deserialize;

/// Most emitters one catalog item may declare.
pub const MAX_COSMETIC_EMITTERS: usize = 4;
/// Largest authored offset from the emitter bone's pivot, in model pixels.
const MAX_OFFSET_PIXELS: f32 = 64.0;
/// Namespace of rewritten effect identifiers; never used by vanilla or pack effects.
pub const COSMETIC_EFFECT_NAMESPACE: &str = "cinnabar_cosmetic";

/// One authored `particles` entry of a cosmetic animation file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    bone: String,
    offset: [f32; 3],
    effect: serde_json::Value,
}

/// A validated emitter: its client-owned effect and a fixed point on one authored bone.
#[derive(Debug)]
pub struct CosmeticEmitter {
    /// Unique effect identifier derived from the animation digest and entry index.
    pub identifier: Arc<str>,
    /// The complete Bedrock particle effect file, rewritten to use `identifier`.
    pub effect: Arc<[u8]>,
    bone: usize,
    offset: [f32; 3],
}

/// Validates every authored emitter; any malformed entry rejects the whole item.
pub(super) fn build(
    definitions: Vec<Definition>,
    names: &[&str],
    animation_sha256: Option<&str>,
) -> Option<Box<[CosmeticEmitter]>> {
    if definitions.is_empty() {
        return Some(Box::default());
    }
    let digest = animation_sha256?.get(..16)?;
    if definitions.len() > MAX_COSMETIC_EMITTERS {
        return None;
    }
    definitions
        .into_iter()
        .enumerate()
        .map(|(index, definition)| {
            let bone = names.iter().position(|name| *name == definition.bone)?;
            if definition
                .offset
                .iter()
                .any(|value| !value.is_finite() || value.abs() > MAX_OFFSET_PIXELS)
            {
                return None;
            }
            let mut effect = definition.effect;
            let identifier: Arc<str> =
                format!("{COSMETIC_EFFECT_NAMESPACE}:{digest}_{index}").into();
            let slot = effect
                .as_object_mut()?
                .get_mut("particle_effect")?
                .as_object_mut()?
                .get_mut("description")?
                .as_object_mut()?
                .get_mut("identifier")?;
            slot.as_str()?;
            *slot = serde_json::Value::String(identifier.to_string());
            Some(CosmeticEmitter {
                identifier,
                effect: serde_json::to_vec(&effect).ok()?.into(),
                bone,
                offset: definition.offset,
            })
        })
        .collect()
}

impl HatRig {
    /// Authored particle emitters in declaration order; empty for most items.
    pub fn emitters(&self) -> &[CosmeticEmitter] {
        &self.emitters
    }

    /// Places an emitter on this rig's posed bones with the actor vertex path's interpolation.
    /// `previous` and `current` are the poses submitted for this item; `world_from_actor` is the
    /// submission's placement. The basis carries the bone's rotation and inherited scale.
    pub fn emitter_transform(
        &self,
        index: usize,
        previous: &[RenderBoneTransform],
        current: &[RenderBoneTransform],
        world_from_actor: [[f32; 4]; 3],
        alpha: f32,
    ) -> Option<crate::ActorEmitterTransform> {
        let emitter = self.emitters.get(index)?;
        let [x, y, z] = emitter.offset;
        crate::actor::sampled_bone_offset(
            [-x / 16.0, y / 16.0, z / 16.0],
            *previous.get(emitter.bone)?,
            *current.get(emitter.bone)?,
            world_from_actor,
            alpha,
        )
    }
}
