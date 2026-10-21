//! Fills as image controls, and the PNG textures they and `@image` exports ship.

use std::io::Cursor;

use super::{Body, Ctx, SOLID_KEY, color, image, label, names, set_alpha, visible_fills};
use crate::figma::Node;

impl Ctx<'_> {
    /// Image controls for `node`'s visible fills, bottom first, with their Figma paint type.
    pub(super) fn fills(&mut self, node: &Node, path: &str) -> Vec<(String, Body)> {
        let mut out = Vec::new();
        // Figma's plugin API lists fills bottom-first, the last on top (FigmaToCode's
        // `retrieveTopFill` relies on the same), so they emit in order.
        for paint in visible_fills(node) {
            let mut body = match (paint.kind.as_str(), paint.color, paint.image.as_deref()) {
                ("SOLID", Some(rgb), _) => {
                    let mut body = image(&self.solid());
                    body.insert("color".into(), color(rgb));
                    body.insert("keep_ratio".into(), false.into());
                    body
                }
                ("IMAGE", _, Some(key)) => {
                    let Some(texture) = self.texture(node, key, path) else {
                        continue;
                    };
                    if paint.rotation.abs() > 0.01 {
                        self.warn(node, path, "rotated image fills are not supported");
                    }
                    let mut body = image(&texture);
                    match paint.scale_mode.as_str() {
                        "FILL" | "" => {
                            body.insert("fill".into(), true.into());
                        }
                        "FIT" => {
                            body.insert("keep_ratio".into(), true.into());
                        }
                        mode => {
                            let message = format!(
                                "{} image scaling is not supported; the image stretches",
                                label(mode)
                            );
                            self.warn(node, path, message);
                            body.insert("keep_ratio".into(), false.into());
                        }
                    }
                    if paint.has_filters {
                        self.warn(node, path, "image adjustments are not supported");
                    }
                    body
                }
                (kind, _, _) => {
                    self.warn(
                        node,
                        path,
                        format!("{} fills are not supported", label(kind)),
                    );
                    continue;
                }
            };
            set_alpha(&mut body, paint.opacity);
            out.push((paint.kind.clone(), body));
        }
        out
    }

    /// The pack texture for image `key`, exported as PNG on first use.
    pub(super) fn texture(&mut self, node: &Node, key: &str, path: &str) -> Option<String> {
        if let Some(texture) = self.textures.get(key) {
            return Some(texture.clone());
        }
        let Some(bytes) = self.images.get(key) else {
            self.warn(node, path, "the image's data is missing");
            return None;
        };
        let png = match to_png(bytes) {
            Ok(png) => png,
            Err(error) => {
                self.warn(node, path, format!("the image could not be read: {error}"));
                return None;
            }
        };
        let stem = names::identifier(&names::parse(&node.name).words, "image");
        Some(self.add_texture(key, &stem, png))
    }

    /// The white texture solid fills tint.
    pub(super) fn solid(&mut self) -> String {
        if let Some(texture) = self.textures.get(SOLID_KEY) {
            return texture.clone();
        }
        let white = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
        let mut png = Cursor::new(Vec::new());
        white
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encoding a 1x1 PNG in memory cannot fail");
        self.add_texture(SOLID_KEY, "solid", png.into_inner())
    }

    fn add_texture(&mut self, key: &str, stem: &str, png: Vec<u8>) -> String {
        let texture = format!("textures/{}/{}", self.namespace, self.stems.unique(stem));
        self.files.insert(format!("{texture}.png"), png);
        self.textures.insert(key.to_owned(), texture.clone());
        texture
    }
}

/// PNG bytes as given; other formats decoded and re-encoded as PNG.
fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match image::guess_format(bytes).map_err(|error| error.to_string())? {
        image::ImageFormat::Png => Ok(bytes.to_vec()),
        _ => {
            let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
            let mut png = Cursor::new(Vec::new());
            decoded
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            Ok(png.into_inner())
        }
    }
}
