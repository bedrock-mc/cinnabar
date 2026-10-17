//! Bounded, cached coverage meshes for caller-authored vector silhouettes and strokes.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    collections::HashMap,
    mem::{size_of, size_of_val},
    sync::Arc,
};

use serde_json::Value;
use ui::{UiMesh, UiVisual};

use super::Canvas;

mod coverage;
mod raster;
#[cfg(test)]
use json_ui::vector_path::{MAX_CONTOURS, MAX_POINTS};
use json_ui::vector_path::{Path, parse};
use raster::mesh;

const MAX_GRID_CELLS: usize = 256;
const MAX_COVERAGE_WORK: usize = 8_000_000;
const MAX_MESH_INDICES: usize = 32_768;
const MAX_CACHED_PATHS: usize = 128;
const MAX_CACHED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Eq, Hash, PartialEq)]
struct Key {
    path: Path,
    size: [u32; 2],
    dpi: u32,
    page: u16,
    color: [u8; 4],
}

struct Entry {
    used: u64,
    bytes: usize,
    mesh: Option<Arc<UiMesh>>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    clock: u64,
}

impl Cache {
    /// Retains meshes and rejections while bounding entry count and authored geometry bytes.
    fn get_or_create(&mut self, key: Key) -> Option<Arc<UiMesh>> {
        self.clock = self.clock.saturating_add(1);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = self.clock;
            return entry.mesh.clone();
        }
        let mesh = mesh(&key).map(Arc::new);
        let bytes = mesh.as_ref().map_or(0, |mesh| {
            size_of_val(mesh.vertices()) + size_of_val(mesh.indices())
        }) + key
            .path
            .contours
            .iter()
            .map(|path| path.len() * size_of::<[u32; 2]>())
            .sum::<usize>();
        if bytes > MAX_CACHED_BYTES {
            return mesh;
        }
        while self.entries.len() >= MAX_CACHED_PATHS || self.bytes + bytes > MAX_CACHED_BYTES {
            let oldest = self.entries.iter().min_by_key(|(_, entry)| entry.used)?.0;
            let oldest = Key {
                path: oldest.path.clone(),
                size: oldest.size,
                dpi: oldest.dpi,
                page: oldest.page,
                color: oldest.color,
            };
            self.bytes -= self.entries.remove(&oldest)?.bytes;
        }
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                used: self.clock,
                bytes,
                mesh: mesh.clone(),
            },
        );
        mesh
    }
}

thread_local! { static CACHE: RefCell<Cache> = RefCell::default(); }

/// Rejects only paths whose authored coverage, including outside-box art and transforms, misses the clip.
fn visible(
    key: &Key,
    dest: [f32; 4],
    clip: [f32; 4],
    transform: Option<json_ui::transform::Affine>,
) -> bool {
    let rect = |[x, y, right, bottom]: [f32; 4]| {
        json_ui::Rect::new(
            f64::from(x),
            f64::from(y),
            f64::from(right - x),
            f64::from(bottom - y),
        )
    };
    let Some(bounds) = json_ui::vector_path::Bounds::new(&key.path) else {
        return false;
    };
    let pixel = 1. / f64::from(f32::from_bits(key.dpi));
    let bounds = bounds.at(rect(dest), pixel);
    let bounds = transform.map_or(bounds, |transform| transform.bounds(bounds));
    let clip = clip.map(f64::from);
    bounds.x - pixel < clip[2]
        && bounds.y - pixel < clip[3]
        && bounds.x + bounds.w + pixel > clip[0]
        && bounds.y + bounds.h + pixel > clip[1]
}

impl Canvas<'_> {
    /// Draws finite authored contours with even-odd fills or a union of rounded strokes.
    pub(super) fn vector_path(
        &self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<UiVisual> {
        let size = [dest[2] - dest[0], dest[3] - dest[1]];
        if size.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            return None;
        }
        let color = data
            .get("#color")
            .or_else(|| data.get("color"))
            .and_then(json_ui::color_value)
            .unwrap_or([255; 4]);
        let color = alpha(color);
        if color[3] == 0 {
            return None;
        }
        let key = Key {
            path: parse(data)?,
            size: size.map(f32::to_bits),
            dpi: self.metrics.dpi_scale.get().to_bits(),
            page: self.solid_page,
            color,
        };
        if self
            .clip()
            .is_some_and(|clip| !visible(&key, dest, clip, self.residual))
        {
            return None;
        }
        CACHE.with(|cache| cache.borrow_mut().get_or_create(key).map(UiVisual::Mesh))
    }
}

#[cfg(test)]
mod tests;
