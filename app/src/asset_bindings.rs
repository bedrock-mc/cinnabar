//! Installs decoded carrier data into renderer and pack-runtime state.
use assets::{
    RuntimeActorCatalog,
    carriers::startup::{LoadedAssets, LoadedAtmosphereAssets},
};
use sha2::{Digest, Sha256};

const ATMOSPHERE_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/atmosphere.wesl");
const CLOUD_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/cloud.wesl");

/// The neutral actor artwork pages for `catalog`; also installs the default player skin.
pub(crate) fn actor_artwork(
    catalog: &RuntimeActorCatalog,
    entities: &assets::RuntimeEntityAssets,
) -> render_model::ActorArtworkPages {
    if let Some(skin) = default_player_skin(catalog, entities) {
        render_model::install_default_player_skin(skin);
    }
    let artwork = render_model::ActorArtworkPages::new(catalog);
    eprintln!(
        "loaded neutral unlit actor artwork: bindings={}, textures={}, page budget rejections={}, rest pose fallbacks={} (pose_expression_unverified); lighting/tint/overlay parity incomplete",
        catalog.bindings().len(),
        catalog.textures().len(),
        artwork.rejected_bindings(),
        catalog
            .bindings()
            .iter()
            .filter(|binding| binding.pose_mode == assets::ActorPoseMode::RestPose)
            .count()
    );
    artwork
}

/// The player entity's default texture from the carrier, when present.
fn default_player_skin(
    catalog: &RuntimeActorCatalog,
    entities: &assets::RuntimeEntityAssets,
) -> Option<std::sync::Arc<[u8]>> {
    catalog
        .textures()
        .iter()
        .find(|texture| {
            (texture.width, texture.height) == (64, 64)
                && entities
                    .sources()
                    .get(texture.source as usize)
                    .is_some_and(|source| {
                        source.path.as_ref() == render_model::DEFAULT_PLAYER_SKIN_PATH
                    })
        })
        .map(|texture| std::sync::Arc::clone(&texture.rgba8))
}

/// Transfers decoded world sidecars to the pack consumers that own their runtime state.
pub(crate) fn install_world_sidecars(assets: &mut LoadedAssets) {
    if let Some(keys) = assets.material_keys.take() {
        pack_runtime::pack_textures::set_base_terrain_catalog(keys.aliases());
        crate::runtime::network::set_base_material_keys(keys);
    }
    if let Some(refs) = assets.vanilla_entity_refs.take() {
        pack_runtime::entity_pack::set_vanilla_refs(refs);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtmosphereEvidence {
    pub envelope_sha256: String,
    pub shader_source_sha256: String,
    pub cloud_shader_source_sha256: String,
}

impl AtmosphereEvidence {
    /// Combines the carrier identity with the shaders compiled into this app.
    #[must_use]
    pub fn evidence(assets: &LoadedAtmosphereAssets) -> AtmosphereEvidence {
        AtmosphereEvidence {
            envelope_sha256: format_sha256(assets.identity()),
            shader_source_sha256: atmosphere_shader_source_sha256(),
            cloud_shader_source_sha256: cloud_shader_source_sha256(),
        }
    }

    /// Formats the stable carrier and shader identities for startup diagnostics.
    #[must_use]
    pub fn startup_summary(assets: &LoadedAtmosphereAssets) -> String {
        let evidence = Self::evidence(assets);
        format!(
            "ATMOSPHERE_EVIDENCE envelope_sha256={} shader_source_sha256={} cloud_shader_source_sha256={}",
            evidence.envelope_sha256,
            evidence.shader_source_sha256,
            evidence.cloud_shader_source_sha256
        )
    }
}

/// Hashes the exact atmosphere shader source compiled into the renderer.
#[must_use]
pub fn atmosphere_shader_source_sha256() -> String {
    format_sha256(Sha256::digest(ATMOSPHERE_SHADER_SOURCE).into())
}

/// Hashes the exact cloud shader source compiled into the renderer.
#[must_use]
pub fn cloud_shader_source_sha256() -> String {
    format_sha256(Sha256::digest(CLOUD_SHADER_SOURCE).into())
}

/// Formats a SHA-256 identity for startup diagnostics.
fn format_sha256(identity: [u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}
