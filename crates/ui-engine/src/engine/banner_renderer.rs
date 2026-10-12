//! Layered banner artwork for the loom and pack-authored banner controls.

use super::{Canvas, screen::PaintError};
use assets::banner::{color_rgb, pattern_by_ui_id};
use serde_json::Value;
use std::collections::BTreeMap;

type Data = BTreeMap<String, Value>;
/// The JSON-UI renderer name of banner controls.
pub(super) const RENDERER: &str = "banner_pattern_renderer";
/// Maximum layers accepted from a banner renderer binding.
const MAX_LAYERS: usize = assets::banner::MAX_BANNER_LAYERS + 1;

/// Reads bounded underscore-separated native pattern IDs and their corresponding dye colors.
fn layers(data: &Data) -> impl Iterator<Item = (&'static str, [u8; 4])> + '_ {
    let value = |key: &str| {
        data.get(key)
            .or_else(|| data.get("property_bag").and_then(|bag| bag.get(key)))
    };
    let patterns = value("#banner_patterns")
        .and_then(Value::as_str)
        .unwrap_or("");
    let colors: Vec<_> = value("#banner_colors")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('_')
        .take(MAX_LAYERS)
        .map(|color| color.parse::<u8>().ok().filter(|color| *color < 16))
        .collect();
    patterns
        .split('_')
        .take(MAX_LAYERS)
        .enumerate()
        .filter_map(move |(index, id)| {
            let pattern = pattern_by_ui_id(id.parse().ok()?)?;
            let color = colors
                .get(index)
                .copied()
                .flatten()
                .map_or([255; 3], |color| color_rgb(i64::from(color)));
            Some((pattern.path, [color[0], color[1], color[2], 255]))
        })
}

/// Collects the actual layer paths before atlas publication.
pub(super) fn texture_paths(data: &Data) -> Vec<&'static str> {
    layers(data).map(|(path, _)| path).collect()
}

impl Canvas<'_> {
    /// Draws the cloth front in binding order, with each layer's own dye and authored destination.
    pub(super) fn banner_pattern(
        &mut self,
        data: &Data,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Result<(), PaintError> {
        let uv = json_ui::UvRect {
            u0: 1.0 / 64.0,
            v0: 1.0 / 64.0,
            u1: 21.0 / 64.0,
            v1: 41.0 / 64.0,
        };
        for (path, color) in layers(data) {
            if let Some(visual) =
                self.sprite(path, uv, alpha(color), json_ui::SpriteFilter::default())
            {
                self.push(visual, dest)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_bindings_keep_layer_order_and_dye_indices() {
        let data = BTreeMap::from([
            ("#banner_patterns".into(), Value::from("0_34_1")),
            ("#banner_colors".into(), Value::from("15_1_4")),
        ]);
        let layers: Vec<_> = layers(&data).collect();
        assert_eq!(
            layers,
            vec![
                ("textures/entity/banner/banner_base", [240, 240, 240, 255]),
                (
                    "textures/entity/banner/banner_stripe_right",
                    [176, 46, 38, 255]
                ),
                ("textures/entity/banner/banner_border", [60, 68, 170, 255]),
            ]
        );
    }
    #[test]
    fn malformed_ids_do_not_shift_colors_or_create_unbounded_layers() {
        let data = BTreeMap::from([
            ("#banner_patterns".into(), Value::from("bad_1_999_34")),
            ("#banner_colors".into(), Value::from("0_15_1_4")),
        ]);
        assert_eq!(
            layers(&data).collect::<Vec<_>>(),
            vec![
                ("textures/entity/banner/banner_border", [240, 240, 240, 255]),
                (
                    "textures/entity/banner/banner_stripe_right",
                    [60, 68, 170, 255]
                ),
            ]
        );
    }
}
