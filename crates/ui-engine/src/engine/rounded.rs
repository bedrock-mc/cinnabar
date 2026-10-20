//! Cached antialiased geometry for host-owned rounded JSON-UI chrome.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use ui::{UiMesh, UiVisual};

use super::Canvas;

const MAX_CACHED_SHAPES: usize = 512;
mod geometry;
mod shadow;
use geometry::mesh;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key {
    width: u32,
    height: u32,
    radius: u32,
    fringe: u32,
    page: u16,
    color: [u8; 4],
    /// Color at the far edge of an optional linear gradient.
    end_color: [u8; 4],
    horizontal: bool,
    /// Width of the optional inside border; zero draws no border.
    border: u32,
    border_color: [u8; 4],
}

/// Least-recently-used built meshes, so unchanged chrome reuses its geometry every frame.
pub(super) struct MeshCache<K> {
    entries: HashMap<K, (u64, Arc<UiMesh>)>,
    clock: u64,
}

impl<K> Default for MeshCache<K> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
        }
    }
}

/// Rounded shapes keyed by their full paint.
type ShapeCache = MeshCache<Key>;

impl<K: Copy + Eq + std::hash::Hash> MeshCache<K> {
    /// Returns the mesh for `key`, building it on a miss. Evicts only the least recently used
    /// entry, preserving hot geometry during transitions.
    pub(super) fn get_or_build(
        &mut self,
        key: K,
        build: impl FnOnce(K) -> Option<UiMesh>,
    ) -> Option<Arc<UiMesh>> {
        self.clock = self.clock.saturating_add(1);
        if let Some((used, mesh)) = self.entries.get_mut(&key) {
            *used = self.clock;
            return Some(Arc::clone(mesh));
        }
        let mesh = Arc::new(build(key)?);
        if self.entries.len() == MAX_CACHED_SHAPES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(key, (self.clock, Arc::clone(&mesh)));
        Some(mesh)
    }
}

impl ShapeCache {
    /// The rounded shape for `key`, tessellated on a miss.
    fn get_or_create(&mut self, key: Key) -> Option<Arc<UiMesh>> {
        self.get_or_build(key, mesh)
    }
}

/// One rounded shape in logical pixels with a solid fill and optional inside border.
#[derive(Clone, Copy)]
pub(super) struct RoundedPaint {
    pub(super) size: [f32; 2],
    pub(super) radius: f32,
    pub(super) fill: [u8; 4],
    pub(super) border: f32,
    pub(super) border_color: [u8; 4],
}

thread_local! { static SHAPES: RefCell<ShapeCache> = RefCell::default(); }

impl Canvas<'_> {
    /// Draws an antialiased rounded rectangle. `color2` with `gradient_direction`
    /// (`vertical` by default, or `horizontal`) fades the fill linearly from `color`;
    /// `border_width` and `border_color` add an inside border that keeps translucent
    /// fills from blending with it.
    pub(super) fn rounded_rectangle(
        &self,
        data: &std::collections::BTreeMap<String, serde_json::Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<UiVisual> {
        let width = dest[2] - dest[0];
        let height = dest[3] - dest[1];
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let length = |name: &str, default: f64| {
            data.get(name)
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or(default) as f32
                * self.px
                * self.length_scale
        };
        // A bound `#name` wins over the authored `name`.
        let color = |bound: &str, name: &str| {
            data.get(bound)
                .or_else(|| data.get(name))
                .and_then(json_ui::color_value)
        };
        let radius = length("radius", f64::from(json_ui::rounded::DEFAULT_RADIUS))
            .clamp(0.0, width.min(height) * 0.5);
        let fill = color("#color", "color").unwrap_or([255; 4]);
        let border_color = color("#border_color", "border_color");
        let border = if border_color.is_some() {
            length("border_width", 1.0).clamp(0.0, width.min(height) * 0.5)
        } else {
            0.0
        };
        let key = Key {
            width: width.to_bits(),
            height: height.to_bits(),
            radius: radius.to_bits(),
            fringe: geometry::coverage_width(self.metrics.dpi_scale.get()).to_bits(),
            page: self.solid_page,
            color: alpha(fill),
            end_color: alpha(color("#color2", "color2").unwrap_or(fill)),
            horizontal: data
                .get("gradient_direction")
                .and_then(serde_json::Value::as_str)
                == Some("horizontal"),
            border: border.to_bits(),
            border_color: alpha(border_color.unwrap_or([0; 4])),
        };
        SHAPES.with(|shapes| {
            let mut shapes = shapes.borrow_mut();
            shapes.get_or_create(key).map(UiVisual::Mesh)
        })
    }

    /// The cached antialiased geometry for `paint`, with radius and border clamped to its size.
    pub(super) fn rounded_shape(&self, paint: RoundedPaint) -> Option<Arc<UiMesh>> {
        let [width, height] = paint.size;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let half = width.min(height) * 0.5;
        let key = Key {
            width: width.to_bits(),
            height: height.to_bits(),
            radius: paint.radius.clamp(0.0, half).to_bits(),
            fringe: geometry::coverage_width(self.metrics.dpi_scale.get()).to_bits(),
            page: self.solid_page,
            color: paint.fill,
            end_color: paint.fill,
            horizontal: false,
            border: paint.border.clamp(0.0, half).to_bits(),
            border_color: paint.border_color,
        };
        SHAPES.with(|shapes| shapes.borrow_mut().get_or_create(key))
    }
}

#[cfg(test)]
mod tests;
