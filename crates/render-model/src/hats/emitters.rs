//! Particle emitters authored in a cosmetic animation and placed on its posed bones.
use super::*;
use serde::Deserialize;

/// Most emitters one catalog item may declare.
pub const MAX_COSMETIC_EMITTERS: usize = 8;
/// Largest authored offset from the emitter bone's pivot, in model pixels.
const MAX_OFFSET_PIXELS: f32 = 64.0;
/// Namespace of rewritten effect identifiers; never used by vanilla or pack effects.
pub const COSMETIC_EFFECT_NAMESPACE: &str = "cinnabar_cosmetic";
/// Effect texture name that samples the emitter's sprite cut from the item's own texture.
pub const COSMETIC_PARTICLE_TEXTURE: &str = "atlas.cosmetic";
/// Side of the square sprite region an emitter cuts from the item's texture, in texels.
pub const COSMETIC_SPRITE_SIDE: u32 = 16;

/// One authored `particles` entry of a cosmetic animation file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    bone: String,
    offset: [f32; 3],
    effect: serde_json::Value,
    #[serde(default)]
    sprites: Option<[u32; 2]>,
}

/// A sprite region cut from the item's original texture for `atlas.cosmetic` effects.
#[derive(Debug)]
pub struct CosmeticSprite {
    /// Stable particle-atlas tile key derived from the texture digest and region origin.
    pub key: u64,
    /// `COSMETIC_SPRITE_SIDE`² RGBA8 texels, row-major.
    pub pixels: Arc<[u8]>,
}

/// A validated emitter: its client-owned effect and a fixed point on one authored bone.
#[derive(Debug)]
pub struct CosmeticEmitter {
    /// Unique effect identifier derived from the animation digest and entry index.
    pub identifier: Arc<str>,
    /// The complete Bedrock particle effect file, rewritten to use `identifier`.
    pub effect: Arc<[u8]>,
    /// The sprite an `atlas.cosmetic` effect samples; absent for atlas-path effects.
    pub sprite: Option<CosmeticSprite>,
    bone: usize,
    offset: [f32; 3],
}

/// Validates every authored emitter; any malformed entry rejects the whole item. Sprites are
/// cut from `texture`, the item's original decoded texture identified by `texture_sha256`.
pub(super) fn build(
    definitions: Vec<Definition>,
    names: &[&str],
    animation_sha256: Option<&str>,
    (texture, texture_sha256): (&CapeImage, &str),
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
            let cosmetic_texture = effect["particle_effect"]["description"]
                ["basic_render_parameters"]["texture"]
                == COSMETIC_PARTICLE_TEXTURE;
            if cosmetic_texture != definition.sprites.is_some() {
                return None;
            }
            let sprite = match definition.sprites {
                Some(origin) => Some(sprite(texture, texture_sha256, origin)?),
                None => None,
            };
            Some(CosmeticEmitter {
                identifier,
                effect: serde_json::to_vec(&effect).ok()?.into(),
                sprite,
                bone,
                offset: definition.offset,
            })
        })
        .collect()
}

/// Cuts the sprite whose top-left texel is `[u, v]`, rejecting regions outside the texture.
fn sprite(texture: &CapeImage, texture_sha256: &str, [u, v]: [u32; 2]) -> Option<CosmeticSprite> {
    use sha2::Digest;
    let side = COSMETIC_SPRITE_SIDE;
    if u.checked_add(side)? > texture.width || v.checked_add(side)? > texture.height {
        return None;
    }
    let row = texture.width as usize * 4;
    let pixels = (v..v + side)
        .flat_map(|y| {
            let start = y as usize * row + u as usize * 4;
            texture.rgba8[start..start + side as usize * 4]
                .iter()
                .copied()
        })
        .collect();
    let digest = sha2::Sha256::digest(format!("cosmetic-sprite:{texture_sha256}:{u}:{v}"));
    Some(CosmeticSprite {
        key: u64::from_le_bytes(digest[..8].try_into().ok()?),
        pixels,
    })
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
