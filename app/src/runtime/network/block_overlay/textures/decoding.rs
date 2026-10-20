//! Source decoding with immutable carrier fallback and destination tint.

use super::*;
use pack_runtime::pack_textures::resolve_pack_texture_with_policy;

impl TextureCatalog<'_> {
    /// Reports an absent static pack source without decoding or borrowing carrier pixels.
    pub(in super::super) fn static_source_missing(
        &self,
        route: &super::super::TextureRoute,
    ) -> bool {
        let Some((_, _, choice)) = self.resolve_choice(route) else {
            return false;
        };
        let reason = diagnostics::failure_reason(self.view, &choice.path);
        if reason != FailureReason::TextureFileMissing {
            return false;
        }
        if self.data.terrain.contains_key(route.key()) {
            self.data
                .diagnostics
                .failure(reason, route.key(), Some(&choice.path));
        }
        true
    }

    /// Resolves the winning pack or carrier filename before looking up reusable frames.
    pub(in super::super) fn reusable_image_path(
        &self,
        route: &super::super::TextureRoute,
        source: Option<&FlipbookSource>,
        mut carrier: impl FnMut(&str, Option<[u8; 3]>) -> Option<String>,
    ) -> Option<(String, bool)> {
        let (_, _, choice) = self.resolve_choice(route)?;
        let path = source.map_or(choice.path.as_str(), |source| source.texture_path.as_ref());
        if let Some(path) = pack_runtime::pack_textures::indexed_image_candidate(self.view, path) {
            Some((path, true))
        } else if source.is_none() {
            pack_runtime::pack_textures::texture_image_paths(path)
                .into_iter()
                .find_map(|path| carrier(&path, choice.tint))
                .map(|path| (path, false))
        } else {
            None
        }
    }

    /// Resolves one route, using normalized image filenames for both pack and carrier sources.
    #[cfg(test)]
    pub(in super::super) fn decode_route(
        &self,
        route: &super::super::TextureRoute,
        carrier: impl FnMut(&str, Option<[u8; 3]>) -> Option<TextureImage>,
    ) -> Option<TextureImage> {
        self.decode_source(route, None, carrier)
    }

    /// Decodes static or independent animation pixels with the destination tint and source identity.
    pub(in super::super) fn decode_source(
        &self,
        route: &super::super::TextureRoute,
        source: Option<&FlipbookSource>,
        mut carrier: impl FnMut(&str, Option<[u8; 3]>) -> Option<TextureImage>,
    ) -> Option<TextureImage> {
        use pack_runtime::pack_textures::texture_image_paths;
        let key = route.key();
        let Some((_, _, choice)) = self.resolve_choice(route) else {
            self.data
                .diagnostics
                .failure(FailureReason::TerrainKeyMissing, key, None);
            return None;
        };
        let path = source.map_or(choice.path.as_str(), |source| source.texture_path.as_ref());
        #[cfg(test)]
        self.decode_attempts.set(self.decode_attempts.get() + 1);
        let mut image =
            resolve_pack_texture_with_policy(self.view, path, self.texture_policy, true)
                .map(|resolved| TextureImage {
                    texture: resolved.texture,
                    image_path: resolved.image_path,
                    baked_tint: None,
                    pack: true,
                })
                .or_else(|| {
                    if source.is_some() {
                        return None;
                    }
                    texture_image_paths(path)
                        .into_iter()
                        .find_map(|path| carrier(&path, choice.tint))
                });
        if let Some(image) = image.as_mut()
            && image.baked_tint.is_none()
            && let Some(tint) = choice.tint
        {
            pack_compile_input::apply_atlas_tint(&mut image.texture.rgba8, tint);
        }
        if image.is_none() {
            let reason = diagnostics::failure_reason(self.view, path);
            if self.data.terrain.contains_key(key) || reason != FailureReason::TextureFileMissing {
                self.data.diagnostics.failure(reason, key, Some(path));
            }
        }
        image
    }
}
