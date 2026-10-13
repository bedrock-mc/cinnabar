use super::parse_pack_json;
use assets::{ParticleEffectFile, ParticleTexture, RuntimeParticleAssets};
use pack_runtime::pack_textures::DecodedTexture;
use resource_pack::LayeredPackView;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Rebuilds particle effects and their referenced textures from the base plus winning layers.
pub(super) fn prepare_particles(
    view: &LayeredPackView,
    base: Option<&RuntimeParticleAssets>,
) -> ::particles::ParticleSystem {
    let mut effects: BTreeMap<Box<str>, ParticleEffectFile> = base
        .into_iter()
        .flat_map(RuntimeParticleAssets::effects)
        .map(|effect| (effect.identifier.clone(), effect.clone()))
        .collect();
    let mut textures: BTreeMap<Box<str>, ParticleTexture> = base
        .into_iter()
        .flat_map(RuntimeParticleAssets::textures)
        .map(|texture| (texture.path.clone(), texture.clone()))
        .collect();
    let mut effect_bytes: usize = effects.values().map(|effect| effect.bytes.len()).sum();
    let texture_bytes: usize = textures.values().map(|texture| texture.rgba8.len()).sum();
    for (_, bytes) in super::layered_json(view, "particles/") {
        if bytes.len() > assets::MAX_PARTICLE_EFFECT_BYTES {
            continue;
        }
        let Some(root) = parse_pack_json(&bytes) else {
            continue;
        };
        let Some(identifier) = root["particle_effect"]["description"]["identifier"].as_str() else {
            continue;
        };
        if identifier.is_empty()
            || identifier.len() > assets::MAX_PARTICLE_KEY_BYTES
            || (effects.len() >= assets::MAX_PARTICLE_EFFECTS && !effects.contains_key(identifier))
        {
            continue;
        }
        let next_bytes = effect_bytes
            - effects
                .get(identifier)
                .map_or(0, |effect| effect.bytes.len())
            + bytes.len();
        if !effects_fit(
            next_bytes,
            texture_bytes,
            assets::MAX_PARTICLE_CARRIER_BYTES,
        ) {
            continue;
        }
        effect_bytes = next_bytes;
        effects.insert(
            identifier.into(),
            ParticleEffectFile {
                identifier: identifier.into(),
                bytes: Arc::from(bytes),
            },
        );
    }
    prepare_textures(
        &effects,
        &mut textures,
        effect_bytes,
        texture_bytes,
        |path| {
            pack_runtime::pack_textures::decode_pack_texture_with_policy(
                view,
                path,
                assets::texture::policy::device_policy(),
                false,
            )
        },
    );
    let textures: Vec<_> = textures.into_values().collect();
    let effects: Vec<_> = effects.into_values().collect();
    let identity = base.map_or_else(
        || Sha256::digest(b"Cinnabar optional particle layer").into(),
        RuntimeParticleAssets::source_manifest_sha256,
    );
    match assets::encode_particle_catalog(identity, &textures, &effects)
        .and_then(|bytes| RuntimeParticleAssets::decode(&bytes))
    {
        Ok(assets) => {
            let mut system = ::particles::ParticleSystem::from_assets(&assets);
            for (_, bytes) in super::layered_json(view, "entity/") {
                if let Some(root) = parse_pack_json(&bytes) {
                    system.actor_bindings.insert_entity(&root);
                }
            }
            for (_, bytes) in super::layered_json(view, "animation_controllers/") {
                if let Some(root) = parse_pack_json(&bytes) {
                    system.actor_bindings.insert_controllers(&root);
                }
            }
            if system.actor_bindings.unsupported > 0 {
                bevy::log::warn!(
                    unsupported = system.actor_bindings.unsupported,
                    "optional actor particle bindings contain unsupported fragments"
                );
            }
            system
        }
        Err(error) => {
            bevy::log::warn!(%error, "optional particle layers ignored");
            base.map_or_else(
                ::particles::ParticleSystem::default,
                ::particles::ParticleSystem::from_assets,
            )
        }
    }
}

/// Checks effect admission against the retained texture payload.
fn effects_fit(effect_bytes: usize, texture_bytes: usize, limit: usize) -> bool {
    effect_bytes
        .checked_add(texture_bytes)
        .is_some_and(|total| total <= limit)
}

/// Resolves referenced textures while retaining the shared catalog budget.
fn prepare_textures(
    effects: &BTreeMap<Box<str>, ParticleEffectFile>,
    textures: &mut BTreeMap<Box<str>, ParticleTexture>,
    effect_bytes: usize,
    mut texture_bytes: usize,
    mut decode: impl FnMut(&str) -> Option<DecodedTexture>,
) {
    let mut attempted = BTreeSet::new();
    for effect in effects.values() {
        let Some(root) = parse_pack_json(&effect.bytes) else {
            continue;
        };
        let Some(path) =
            root["particle_effect"]["description"]["basic_render_parameters"]["texture"].as_str()
        else {
            continue;
        };
        if path.is_empty()
            || path.len() > assets::MAX_PARTICLE_KEY_BYTES
            || (textures.len() >= assets::MAX_PARTICLE_TEXTURES && !textures.contains_key(path))
        {
            continue;
        }
        if !attempted.insert(path.to_owned()) {
            continue;
        }
        if let Some(texture) = decode(path) {
            let next_bytes = texture_bytes
                - textures.get(path).map_or(0, |texture| texture.rgba8.len())
                + texture.rgba8.len();
            if next_bytes + effect_bytes > assets::MAX_PARTICLE_CARRIER_BYTES {
                continue;
            }
            let replacement = ParticleTexture {
                path: path.into(),
                width: texture.width,
                height: texture.height,
                rgba8: Arc::from(texture.rgba8),
            };
            if !::particles::particle_textures_fit(
                textures
                    .values()
                    .filter(|texture| texture.path.as_ref() != path)
                    .chain(std::iter::once(&replacement)),
                assets::texture::policy::device_policy(),
            ) {
                continue;
            }
            texture_bytes = next_bytes;
            textures.insert(path.into(), replacement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_particle_override_retains_pixels_and_bad_override_keeps_the_base() {
        let path = "textures/particle/particles";
        let bytes: Arc<[u8]> = Arc::from(br#"{"particle_effect":{"description":{"identifier":"test:particle","basic_render_parameters":{"texture":"textures/particle/particles"}}}}"#.as_slice());
        let base = RuntimeParticleAssets::decode(
            &assets::encode_particle_catalog(
                [1; 32],
                &[ParticleTexture {
                    path: path.into(),
                    width: 16,
                    height: 16,
                    rgba8: vec![7; 16 * 16 * 4].into(),
                }],
                &[ParticleEffectFile {
                    identifier: "test:particle".into(),
                    bytes,
                }],
            )
            .unwrap(),
        )
        .unwrap();
        for (width, height, admitted) in
            [(2048, 2048, true), (2048, 4096, false), (1024, 4096, false)]
        {
            let image = image::RgbaImage::from_pixel(width, height, image::Rgba([91, 73, 57, 255]));
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            let view =
                super::super::tests::view(&[("textures/particle/particles.png", png.get_ref())]);
            if (width, height) == (1024, 4096) {
                assert!(
                    pack_runtime::pack_textures::decode_pack_texture_with_policy(
                        &view,
                        path,
                        assets::texture::policy::device_policy(),
                        false,
                    )
                    .is_some(),
                    "the fallback case must pass source-image admission"
                );
            }
            let system = prepare_particles(&view, Some(&base));
            let placement = system.atlas().placement(path).unwrap();
            assert_eq!(
                [placement.width, placement.height],
                if admitted { [width, height] } else { [16; 2] }
            );
            let offset = ((placement.y * system.atlas().side() + placement.x) * 4) as usize;
            assert_eq!(
                &system.atlas().pixels()[offset..offset + 4],
                if admitted {
                    &[91, 73, 57, 255]
                } else {
                    &[7; 4]
                }
            );
        }
        let restored = prepare_particles(&super::super::tests::view(&[]), Some(&base));
        assert_eq!(restored.atlas().placement(path).unwrap().width, 16);
    }

    #[test]
    fn actor_controller_particles_survive_pack_preparation() {
        let Some(view) =
            crate::runtime::network::local_pack::local_pack_view("CINNABAR_SERVER_PACK")
        else {
            eprintln!(
                "skipping actor_controller_particles_survive_pack_preparation: missing CINNABAR_SERVER_PACK NPC fixture"
            );
            return;
        };
        let system = prepare_particles(&view, None);
        assert!(system.has_effect("hivehub:game_wars"));
        let effects: Vec<_> = system
            .actor_bindings
            .effects_for(
                "hivehub:game_wars",
                "controller.animation.hive.hub.game.idle.particle",
                "default",
            )
            .map(|effect| (effect.identifier, effect.bound))
            .collect();
        assert_eq!(effects, [("hivehub:game_wars", true)]);
        let effects: Vec<_> = system
            .actor_bindings
            .effects_for(
                "hivehub:game_sky",
                "controller.animation.hive.hub.game.idle.particle",
                "default",
            )
            .map(|effect| (effect.identifier, effect.bound))
            .collect();
        assert_eq!(effects, [("hivehub:game_sky", true)]);
    }

    #[test]
    fn review_particle_effect_budget_includes_retained_textures() {
        assert!(!effects_fit(12, 24, 32));
        assert!(effects_fit(8, 24, 32));
    }

    #[test]
    fn review_shared_particle_texture_is_decoded_once() {
        let bytes: Arc<[u8]> = Arc::from(br#"{"particle_effect":{"description":{"basic_render_parameters":{"texture":"textures/shared"}}}}"#.as_slice());
        let effects = ["a", "b"]
            .into_iter()
            .map(|identifier| {
                (
                    identifier.into(),
                    ParticleEffectFile {
                        identifier: identifier.into(),
                        bytes: Arc::clone(&bytes),
                    },
                )
            })
            .collect();
        let mut calls = 0;
        prepare_textures(&effects, &mut BTreeMap::new(), 0, 0, |_| {
            calls += 1;
            Some(DecodedTexture {
                width: 1,
                height: 1,
                rgba8: vec![0; 4].into(),
            })
        });
        assert_eq!(calls, 1);
    }
}
