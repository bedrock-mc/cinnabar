//! Constant-size geometry for repeated source regions.

use super::{UiError, UiVisual};
use render_api::ui_repeat::{UI_STYLE_REPEAT_X, UI_STYLE_REPEAT_Y};

impl UiVisual {
    /// Builds a repeat primitive from finite tile coordinates and source texel edges.
    pub fn repeated_sprite(
        texture_page: u16,
        source: [f32; 4],
        repeat: json_ui::SpriteRepeat,
        color: [u8; 4],
        filter: json_ui::SpriteFilter,
        page_clip: bool,
    ) -> Result<Self, UiError> {
        let style = (u16::from(filter.bilinear) * u16::from(super::UI_STYLE_BILINEAR))
            | (u16::from(filter.grayscale) * u16::from(super::UI_STYLE_GRAYSCALE))
            | (u16::from(repeat.axes[0]) * UI_STYLE_REPEAT_X)
            | (u16::from(repeat.axes[1]) * UI_STYLE_REPEAT_Y)
            | (u16::from(page_clip) * render_api::ui_repeat::UI_STYLE_REPEAT_PAGE);
        let bounds = [
            source[0],
            source[1],
            source[2] - source[0],
            source[3] - source[1],
        ];
        let coords = repeat.uv;
        let tile_uv = [coords.u0, coords.v0, coords.u1, coords.v1];
        if !bounds
            .iter()
            .chain(tile_uv.iter())
            .all(|value| value.is_finite())
        {
            return Err(UiError::InvalidSpriteCoordinates);
        }
        Ok(Self::RepeatedSprite {
            texture_page,
            tile_uv,
            source: bounds,
            color,
            style,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_sprite_uses_four_vertices_for_billions_of_tiles() {
        let repeat = json_ui::SpriteRepeat {
            uv: json_ui::UvRect {
                u0: 0.0,
                v0: 0.0,
                u1: 128_000.0,
                v1: 64_000.0,
            },
            axes: [true; 2],
        };
        let visual = UiVisual::repeated_sprite(
            3,
            [10.0, 20.0, 12.0, 22.0],
            repeat,
            [255; 4],
            Default::default(),
            false,
        )
        .unwrap();
        let rect = crate::UiRect::new(
            crate::UiPoint::new(0.0, 0.0).unwrap(),
            crate::UiPoint::new(256.0, 128.0).unwrap(),
        )
        .unwrap();
        let node = crate::UiNode::new(crate::UiNodeId::new(1), None, rect).with_visual(visual);
        let list = crate::UiTree::new(vec![node])
            .unwrap()
            .build_draw_list()
            .unwrap();
        assert_eq!(list.vertices.len(), 4);
        assert_eq!(list.indices.len(), 6);
        assert_eq!(list.batches.len(), 1);
        assert_eq!(list.vertices[2].uv, [128_000.0, 64_000.0]);
        assert!(
            list.vertices
                .iter()
                .all(|vertex| vertex.overlay_color == [10.0, 20.0, 2.0, 2.0])
        );
    }
}
