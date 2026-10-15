//! Mod-surface glass panels and the world backdrop.
//!
//! Each emits backdrop geometry ([`ui::UiBlendMode::Backdrop`]) that the renderer fills from one
//! shared blurred copy of the scene, then ordinary rounded chrome above it. Only hosts that pass
//! a [`GlassRenderer`] draw [`GLASS_RENDERER`] nodes, so server and resource-pack screens never
//! do.

use std::{cell::RefCell, sync::Arc};

use render_api::UiBackdropPaint;
use serde_json::Value;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiVisual,
    mod_panel::backdrop::{BLUR, BRIGHTNESS, BackdropProperty, GLASS_RENDERER, SATURATION},
};

use super::{
    Canvas, rect,
    rounded::{MeshCache, RoundedPaint},
    screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame},
};

/// Default glass corner radius in GUI pixels, matching `cinnabar_rounded_rectangle`.
const DEFAULT_RADIUS: f64 = 6.0;
/// Default top highlight width in GUI pixels, like CSS `inset 0 1px 0`.
const DEFAULT_HIGHLIGHT_WIDTH: f64 = 1.0;

/// A backdrop in GUI pixels, as authored by a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backdrop {
    /// Gaussian standard deviation in GUI pixels.
    pub blur: f64,
    pub saturation: f64,
    pub brightness: f64,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            blur: BLUR.identity,
            saturation: SATURATION.identity,
            brightness: BRIGHTNESS.identity,
        }
    }
}

impl Backdrop {
    /// Reads each property through `value`, clamping it into its bound.
    pub fn read(value: impl Fn(BackdropProperty) -> Option<f64>) -> Self {
        Self {
            blur: BLUR.clamp(value(BLUR)),
            saturation: SATURATION.clamp(value(SATURATION)),
            brightness: BRIGHTNESS.clamp(value(BRIGHTNESS)),
        }
    }

    /// Whether this backdrop leaves the scene unchanged and so draws nothing.
    pub fn is_identity(self) -> bool {
        self == Self::default()
    }

    /// The renderer's paint for `gui_physical` physical pixels per GUI pixel.
    fn paint(self, gui_physical: f32) -> UiBackdropPaint {
        UiBackdropPaint {
            blur: self.blur as f32 * gui_physical,
            saturation: self.saturation as f32,
            brightness: self.brightness as f32,
        }
        .bounded()
    }
}

/// Draws `cinnabar_glass` nodes for a mod surface, seen through the surface's world backdrop.
#[derive(Default)]
pub struct GlassRenderer {
    world: Backdrop,
}

impl GlassRenderer {
    /// Glass drawn above `world`, which [`paint_world_backdrop`] paints first.
    pub fn new(world: Backdrop) -> Self {
        Self { world }
    }
}

impl CustomRenderer for GlassRenderer {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != GLASS_RENDERER {
            return Ok(false);
        }
        canvas.glass(node, self.world)?;
        Ok(true)
    }
}

/// Paints `backdrop` over the whole content area beneath everything painted after it.
/// Draws nothing for an identity backdrop.
pub fn paint_world_backdrop(
    frame: &mut PaintFrame<'_>,
    backdrop: Backdrop,
) -> Result<(), PaintError> {
    let [width, height] = frame.content;
    if backdrop.is_identity() || width <= 0.0 || height <= 0.0 {
        return Ok(());
    }
    let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let paint = backdrop.paint(px * frame.metrics.dpi_scale.get());
    let Some(mesh) = WORLD.with(|cache| {
        cache
            .borrow_mut()
            .get_or_build((frame.solid_page, paint_bits(paint)), |(page, _)| {
                world_quad(page, paint)
            })
    }) else {
        return Ok(());
    };
    let group = ui::UiNodeId::new(*frame.next);
    let id = ui::UiNodeId::new(frame.next.saturating_add(1));
    *frame.next = frame.next.saturating_add(2);
    let bounds = rect(0.0, 0.0, width, height)?;
    frame
        .nodes
        .push(UiNode::new(group, None, bounds).with_clip_children(true));
    frame
        .nodes
        .push(UiNode::new(id, Some(group), bounds).with_visual(UiVisual::Mesh(mesh)));
    Ok(())
}

type BackdropKey = (u16, [u32; 3]);
/// A glass outline's logical size, radius and display density, its opacity and its paint.
type GlassKey = ([u32; 4], u8, [u32; 3]);

thread_local! {
    /// Backdrop geometry derived from rounded shapes, keyed by outline and paint.
    static GLASS: RefCell<MeshCache<GlassKey>> = RefCell::default();
    /// Whole-content world backdrop quads, keyed by page and paint.
    static WORLD: RefCell<MeshCache<BackdropKey>> = RefCell::default();
}

/// Bit patterns identifying a bounded paint.
fn paint_bits(paint: UiBackdropPaint) -> [u32; 3] {
    [paint.blur, paint.saturation, paint.brightness].map(f32::to_bits)
}

/// One fully covered quad over its node.
fn world_quad(page: u16, paint: UiBackdropPaint) -> Option<UiMesh> {
    let vertices: Vec<_> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        .map(|position| backdrop_vertex(position, 1.0, 255, paint))
        .into();
    UiMesh::new(
        vertices.into(),
        Arc::from([0, 1, 2, 0, 2, 3]),
        Arc::from([backdrop_batch(page, 6)]),
    )
    .ok()
}

/// A backdrop vertex with `coverage` of its shape's area and `opacity` in its tint alpha.
fn backdrop_vertex(
    position: [f32; 2],
    coverage: f32,
    opacity: u8,
    paint: UiBackdropPaint,
) -> UiMeshVertex {
    UiMeshVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [0.0, coverage],
        color: [255, 255, 255, opacity],
        model_light: 0.0,
        overlay_color: paint.overlay(),
        style_flags: 0,
        alpha_test: false,
    }
}

/// The single backdrop material of a shape with `indices` indices.
fn backdrop_batch(page: u16, indices: u32) -> UiMeshBatch {
    UiMeshBatch {
        texture_page: page,
        index_range: 0..indices,
        blend: UiBlendMode::Backdrop,
        depth_test: false,
        depth_write: false,
        alpha_cutoff: None,
    }
}

/// `shape`'s topology and area coverage as backdrop geometry.
fn backdrop_of(shape: &UiMesh, opacity: u8, paint: UiBackdropPaint) -> Option<UiMesh> {
    let page = shape.batches().first()?.texture_page;
    let vertices: Vec<_> = shape
        .vertices()
        .iter()
        .map(|vertex| backdrop_vertex(vertex.position, vertex.uv[1], opacity, paint))
        .collect();
    UiMesh::new(
        vertices.into(),
        Arc::from(shape.indices()),
        Arc::from([backdrop_batch(page, shape.indices().len() as u32)]),
    )
    .ok()
}

impl Canvas<'_> {
    /// Draws a glass panel: the processed scene inside its rounded outline, then its `color`
    /// tint and optional `border_color` border, then an optional `highlight_color` line along
    /// the inside of its top edge. Backdrop values bound as `#blur`, `#saturation` and
    /// `#brightness` win over authored ones and compose with `world`.
    fn glass(&mut self, node: &CustomDraw<'_>, world: Backdrop) -> Result<(), PaintError> {
        let (data, dest) = (node.data, node.dest);
        let size = [dest[2] - dest[0], dest[3] - dest[1]];
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Ok(());
        }
        let number = |bound: &str, name: &str| {
            data.get(bound)
                .or_else(|| data.get(name))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
        };
        let color = |name: &str| {
            data.get(&format!("#{name}"))
                .or_else(|| data.get(name))
                .and_then(json_ui::color_value)
        };
        let px = self.px;
        let length = |name: &str, default: f64| {
            number(&format!("#{name}"), name)
                .unwrap_or(default)
                .max(0.0) as f32
                * px
        };
        let radius = length("radius", DEFAULT_RADIUS);
        let gui_physical = px * self.metrics.dpi_scale.get();
        let glass = Backdrop::read(|property| number(property.bound, property.name));
        let paint = glass.paint(gui_physical).over(world.paint(gui_physical));
        let opacity = node.alpha([255; 4])[3];
        let shape = RoundedPaint {
            size,
            radius,
            fill: [255; 4],
            border: 0.0,
            border_color: [0; 4],
        };
        if opacity > 0
            && let Some(base) = self.rounded_shape(shape)
        {
            let outline =
                [size[0], size[1], radius, self.metrics.dpi_scale.get()].map(f32::to_bits);
            let key = (outline, opacity, paint_bits(paint));
            let mesh = GLASS.with(|cache| {
                cache
                    .borrow_mut()
                    .get_or_build(key, |_| backdrop_of(&base, opacity, paint))
            });
            if let Some(mesh) = mesh {
                self.push(UiVisual::Mesh(mesh), dest)?;
            }
        }
        let tint = node.alpha(color("color").unwrap_or([0; 4]));
        let border_color = color("border_color").map(|color| node.alpha(color));
        if tint[3] > 0 || border_color.is_some_and(|color| color[3] > 0) {
            let chrome = RoundedPaint {
                fill: tint,
                border: border_color.map_or(0.0, |_| length("border_width", 1.0)),
                border_color: border_color.unwrap_or([0; 4]),
                ..shape
            };
            if let Some(mesh) = self.rounded_shape(chrome) {
                self.push(UiVisual::Mesh(mesh), dest)?;
            }
        }
        let highlight = node.alpha(color("highlight_color").unwrap_or([0; 4]));
        if highlight[3] > 0 {
            self.highlight(
                shape,
                highlight,
                length("highlight_width", DEFAULT_HIGHLIGHT_WIDTH),
                dest,
            )?;
        }
        Ok(())
    }

    /// Strokes `shape`'s outline in `color`, showing only the band across its top edge and
    /// corners, like a CSS `inset 0 1px 0` highlight.
    fn highlight(
        &mut self,
        shape: RoundedPaint,
        color: [u8; 4],
        width: f32,
        dest: [f32; 4],
    ) -> Result<(), PaintError> {
        let Some(current) = self.clip() else {
            return Ok(());
        };
        let band = dest[1] + (shape.radius * 0.5).max(width);
        let clip = [
            current[0].max(dest[0]),
            current[1].max(dest[1]),
            current[2].min(dest[2]),
            current[3].min(band),
        ];
        if clip[0] >= clip[2] || clip[1] >= clip[3] || width <= 0.0 {
            return Ok(());
        }
        let Some(mesh) = self.rounded_shape(RoundedPaint {
            fill: [0; 4],
            border: width,
            border_color: color,
            ..shape
        }) else {
            return Ok(());
        };
        // Later nodes open their own clip group.
        self.group(clip)?;
        self.push(UiVisual::Mesh(mesh), dest)
    }
}

#[cfg(test)]
#[path = "glass_tests.rs"]
mod tests;
