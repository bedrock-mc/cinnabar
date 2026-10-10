//! Physical attachment bounds for render-pass scissors and viewports.
use bevy::camera::Viewport;
use render_model::UiScissor;

/// Reads a full-size attachment in physical pixels, independent of the UI layout.
pub(crate) fn extent(view: &wgpu::TextureView) -> [u32; 2] {
    [view.texture().width(), view.texture().height()]
}

/// Intersects a physical scissor with its attachment; empty draws have no rectangle.
pub(crate) fn scissor(rect: UiScissor, extent: [u32; 2]) -> Option<UiScissor> {
    let right = rect.x.saturating_add(rect.width).min(extent[0]);
    let bottom = rect.y.saturating_add(rect.height).min(extent[1]);
    (rect.x < right && rect.y < bottom)
        .then(|| UiScissor::new(rect.x, rect.y, right - rect.x, bottom - rect.y))
}

/// Preserves the depth range while clipping a camera viewport to physical attachment pixels.
pub(crate) fn viewport(viewport: &Viewport, extent: [u32; 2]) -> Option<Viewport> {
    let rect = scissor(
        UiScissor::new(
            viewport.physical_position.x,
            viewport.physical_position.y,
            viewport.physical_size.x,
            viewport.physical_size.y,
        ),
        extent,
    )?;
    Some(Viewport {
        physical_position: bevy::math::UVec2::new(rect.x, rect.y),
        physical_size: bevy::math::UVec2::new(rect.width, rect.height),
        depth: viewport.depth.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rectangles_intersect_small_and_empty_physical_targets() {
        let layout = UiScissor::new(0, 0, 352, 184);
        for size in [[254, 124], [1, 1], [0, 0], [0, 124], [254, 0]] {
            let expected = (!size.contains(&0)).then(|| UiScissor::new(0, 0, size[0], size[1]));
            assert_eq!(scissor(layout, size), expected);
            let view = Viewport {
                physical_size: bevy::math::UVec2::new(352, 184),
                ..Default::default()
            };
            assert_eq!(
                viewport(&view, size).map(|v| [v.physical_size.x, v.physical_size.y]),
                expected.map(|r| [r.width, r.height])
            );
        }
        assert_eq!(scissor(UiScissor::new(254, 0, 3, 5), [254, 124]), None);
        assert_eq!(
            scissor(UiScissor::new(250, 120, u32::MAX, u32::MAX), [254, 124]),
            Some(UiScissor::new(250, 120, 4, 4))
        );
        assert_eq!(scissor(UiScissor::new(5, 5, 0, 0), [254, 124]), None);
    }

    #[test]
    fn physical_viewports_keep_position_and_depth_at_different_dpi_scales() {
        let view = Viewport {
            physical_position: bevy::math::UVec2::new(20, 10),
            physical_size: bevy::math::UVec2::new(704, 368),
            depth: 0.2..0.8,
        };
        let clipped = viewport(&view, [508, 248]).unwrap();
        assert_eq!(clipped.physical_position, view.physical_position);
        assert_eq!(clipped.physical_size, bevy::math::UVec2::new(488, 238));
        assert_eq!(clipped.depth, view.depth);
    }
}
