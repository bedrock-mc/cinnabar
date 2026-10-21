//! `minecraft:` resource references in `shaders.properties` custom textures
//! (`texture.<stage>.<sampler> = minecraft:textures/environment/clouds.png`), resolved from
//! the vanilla textures Cinnabar has loaded.
//!
//! # Derivation
//! Java's resource location `minecraft:textures/<path>.<ext>` names the Bedrock vanilla
//! pack's `textures/<path>` (Bedrock keeps the same `textures/environment/` names and
//! accepts `.png` or `.tga`, so the extension is not compared). The textures available are
//! those Cinnabar decodes for its own renderers, with resource-pack overrides applied: today
//! the atmosphere carrier's `textures/environment/sun.png`, `moon_phases.png` and
//! `clouds.png`. Other namespaces, `textures/atlas/*` (generated atlases, not files) and
//! textures Cinnabar does not load are unresolved and logged; Iris then binds its missing
//! texture, here the placeholder.

use render_scene::AtmosphereTextureAssets;
use std::sync::Arc;

/// One decoded vanilla texture: top row first, straight-alpha RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VanillaImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

/// The vanilla textures a pack's resource references can name, by Bedrock pack path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VanillaTextures {
    images: Vec<(String, VanillaImage)>,
}

/// `path` without its file extension.
fn stem(path: &str) -> &str {
    match path.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => stem,
        _ => path,
    }
}

/// The Bedrock pack path a Java resource location names, or why it has none.
pub(crate) fn bedrock_path(location: &str) -> Result<&str, String> {
    let (namespace, path) = location
        .split_once(':')
        .ok_or_else(|| format!("{location}: not a resource location"))?;
    if namespace != "minecraft" {
        return Err(format!("{location}: only minecraft: resources resolve"));
    }
    let path = path.trim_start_matches('/');
    if path.starts_with("textures/atlas/") {
        return Err(format!(
            "{location}: generated atlases are not texture files"
        ));
    }
    if !path.starts_with("textures/") {
        return Err(format!("{location}: not a texture"));
    }
    Ok(path)
}

impl VanillaTextures {
    /// The textures in `assets` (sun, moon phases, clouds).
    pub(crate) fn from_atmosphere(assets: &AtmosphereTextureAssets) -> Self {
        let images = assets
            .runtime()
            .map(|runtime| {
                runtime
                    .textures()
                    .iter()
                    .map(|t| {
                        (
                            t.source_path.to_string(),
                            VanillaImage {
                                width: t.width,
                                height: t.height,
                                rgba8: Arc::from(&t.rgba8[..]),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { images }
    }

    #[cfg(test)]
    pub(crate) fn with(mut self, path: &str, image: VanillaImage) -> Self {
        self.images.push((path.to_owned(), image));
        self
    }

    /// The image `location` names.
    pub(crate) fn resolve(&self, location: &str) -> Result<&VanillaImage, String> {
        let path = stem(bedrock_path(location)?);
        self.images
            .iter()
            .find(|(p, _)| stem(p) == path)
            .map(|(_, image)| image)
            .ok_or_else(|| format!("{location}: Cinnabar has not loaded `{path}`"))
    }
}
