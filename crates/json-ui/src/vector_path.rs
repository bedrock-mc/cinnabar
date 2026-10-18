//! Finite authored contours and canonical even-odd cancellation.

use std::collections::BTreeMap;

use serde_json::Value;

/// Renderer name shared by path placement and painting.
pub const RENDERER: &str = "cinnabar_vector_path";
/// Maximum number of authored contours in one path.
pub const MAX_CONTOURS: usize = 64;
/// Maximum total number of authored points in one path.
pub const MAX_POINTS: usize = 2048;

#[derive(Clone, Eq, Hash, PartialEq)]
/// Validated, canonical path coordinates stored as finite float bits.
pub struct Path {
    /// Authored origin and positive extent.
    pub view_box: [u32; 4],
    /// Canonical fill contours or ordered stroke polylines.
    pub contours: Vec<Vec<[u32; 2]>>,
    /// Positive authored stroke width; absent for an even-odd fill.
    pub stroke_width: Option<u32>,
    /// Whether each contour joins its last point to its first.
    pub closed: bool,
}

/// Conservative authored bounds kept separately from layout and input rectangles.
#[derive(Clone, Copy)]
pub struct Bounds {
    view_box: [f32; 4],
    min: [f32; 2],
    max: [f32; 2],
    stroke: f32,
}

impl Bounds {
    /// Retains point extrema and the shared stroke policy without retaining all path points.
    pub fn new(path: &Path) -> Option<Self> {
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for point in path.contours.iter().flatten() {
            for axis in 0..2 {
                let value = f32::from_bits(point[axis]);
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        min.iter().all(|value| value.is_finite()).then_some(Self {
            view_box: path.view_box.map(f32::from_bits),
            min,
            max,
            stroke: path.stroke_width.map(f32::from_bits).unwrap_or(0.0),
        })
    }

    /// Includes round caps, joins and half a physical pixel of coverage in virtual coordinates.
    pub fn at(self, dest: crate::Rect, pixel: f64) -> crate::Rect {
        self.geometry(dest).expanded(self.padding(dest, pixel))
    }

    /// Keeps authored bounds and allows one-pixel position and size changes from snapping.
    pub(crate) fn subtree_at(self, dest: crate::Rect, pixel: f64) -> crate::Rect {
        let enlarged = crate::Rect::new(dest.x, dest.y, dest.w + pixel, dest.h + pixel);
        dest.union(self.geometry(dest))
            .union(self.geometry(enlarged))
            .expanded(self.padding(enlarged, pixel) + pixel)
    }

    /// Round strokes scale by the smaller destination axis, independently of contour coordinates.
    fn padding(self, dest: crate::Rect, pixel: f64) -> f64 {
        let [_, _, width, height] = self.view_box.map(f64::from);
        f64::from(self.stroke) * (dest.w / width).min(dest.h / height) * 0.5 + pixel * 0.5
    }

    /// Transforms point extrema without coverage padding or changes to the authored rectangle.
    fn geometry(self, dest: crate::Rect) -> crate::Rect {
        let [x, y, width, height] = self.view_box.map(f64::from);
        let scale = [dest.w / width, dest.h / height];
        let min = [
            dest.x + (f64::from(self.min[0]) - x) * scale[0],
            dest.y + (f64::from(self.min[1]) - y) * scale[1],
        ];
        let max = [
            dest.x + (f64::from(self.max[0]) - x) * scale[0],
            dest.y + (f64::from(self.max[1]) - y) * scale[1],
        ];
        crate::Rect::new(min[0], min[1], max[0] - min[0], max[1] - min[1])
    }
}

/// Measures current draw data directly, without a separate style or geometry-bounds cache.
pub(crate) fn bounds(data: &BTreeMap<String, Value>) -> Option<Bounds> {
    Bounds::new(&parse(data)?)
}

/// Rejects malformed or excessively large path data before allocating raster geometry.
pub fn parse(data: &BTreeMap<String, Value>) -> Option<Path> {
    let source_box = data.get("view_box")?.as_array()?;
    if source_box.len() != 4 {
        return None;
    }
    let number = |value: &Value| {
        let number = value.as_f64()? as f32;
        (number.is_finite() && number.abs() <= 1_000_000.0).then_some(if number == 0.0 {
            0.0
        } else {
            number
        })
    };
    let view_box = [
        number(&source_box[0])?,
        number(&source_box[1])?,
        number(&source_box[2])?,
        number(&source_box[3])?,
    ];
    if view_box[2] <= 0.0 || view_box[3] <= 0.0 {
        return None;
    }
    let stroke_width = match data.get("stroke_width") {
        Some(value) => Some(number(value)?),
        None => None,
    };
    if stroke_width.is_some_and(|value| value <= 0.0) {
        return None;
    }
    let closed = match data.get("closed") {
        Some(value) => value.as_bool()?,
        None => stroke_width.is_none(),
    };
    if stroke_width.is_none() && !closed {
        return None;
    }
    let source_paths = data.get("paths")?.as_array()?;
    if source_paths.is_empty() || source_paths.len() > MAX_CONTOURS {
        return None;
    }
    let mut contours = Vec::with_capacity(source_paths.len());
    let mut total = 0usize;
    for source in source_paths {
        let points = source.as_array()?;
        total = total.checked_add(points.len())?;
        if points.len() < if stroke_width.is_some() { 2 } else { 3 } || total > MAX_POINTS {
            return None;
        }
        let mut contour = Vec::with_capacity(points.len());
        for point in points {
            let point = point.as_array()?;
            if point.len() != 2 {
                return None;
            }
            contour.push([number(&point[0])?.to_bits(), number(&point[1])?.to_bits()]);
        }
        contours.push(contour);
    }
    if stroke_width.is_none() {
        contours = cancel_contours(contours);
    }
    Some(Path {
        view_box: view_box.map(f32::to_bits),
        contours,
        stroke_width: stroke_width.map(f32::to_bits),
        closed,
    })
}

/// Finds a lexicographically smallest cyclic start in linear time, including repeated points.
fn rotation(points: &[[u32; 2]]) -> usize {
    let length = points.len();
    let (mut first, mut second, mut offset) = (0, 1, 0);
    while first < length && second < length && offset < length {
        let a = points[(first + offset) % length];
        let b = points[(second + offset) % length];
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => offset += 1,
            std::cmp::Ordering::Greater => {
                first += offset + 1;
                if first <= second {
                    first = second + 1;
                }
                offset = 0;
            }
            std::cmp::Ordering::Less => {
                second += offset + 1;
                if second <= first {
                    second = first + 1;
                }
                offset = 0;
            }
        }
    }
    first.min(second)
}

/// Normalizes cyclic starts and winding without changing a contour's silhouette.
fn canonical(mut contour: Vec<[u32; 2]>) -> Vec<[u32; 2]> {
    let first = rotation(&contour);
    contour.rotate_left(first);
    let forward = contour.clone();
    contour.reverse();
    let first = rotation(&contour);
    contour.rotate_left(first);
    forward.min(contour)
}

/// Degenerate fills have no area and must not acquire a ghost stroke from distance coverage.
fn has_area(contour: &[[u32; 2]]) -> bool {
    let Some(first) = contour.first() else {
        return false;
    };
    let Some(second) = contour.iter().find(|point| *point != first) else {
        return false;
    };
    let a = first.map(f32::from_bits);
    let b = second.map(f32::from_bits);
    contour.iter().any(|point| {
        let p = point.map(f32::from_bits);
        (b[0] - a[0]) * (p[1] - a[1]) != (b[1] - a[1]) * (p[0] - a[0])
    })
}

/// Identical even-odd boundaries cancel in pairs, independent of start point or winding.
fn cancel_contours(contours: Vec<Vec<[u32; 2]>>) -> Vec<Vec<[u32; 2]>> {
    let mut retained = BTreeMap::<Vec<[u32; 2]>, ()>::new();
    for mut contour in contours {
        contour.dedup();
        if contour.first() == contour.last() {
            contour.pop();
        }
        if contour.len() < 3 || !has_area(&contour) {
            continue;
        }
        let contour = canonical(contour);
        if retained.remove(&contour).is_none() {
            retained.insert(contour, ());
        }
    }
    retained.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coverage bounds follow the view-box origin and the smaller stroke scale on stretched nodes.
    #[test]
    fn coverage_bounds_include_round_caps_and_physical_pixel_support() {
        let data = serde_json::from_value(serde_json::json!({
            "view_box": [4, 8, 16, 16], "stroke_width": 4,
            "paths": [[[4, 16], [20, 16]]],
        }))
        .unwrap();
        let bounds = Bounds::new(&parse(&data).unwrap()).unwrap();
        for width in [16.0, 32.0] {
            assert_eq!(
                bounds.at(crate::Rect::new(-width, 0.0, width, 16.0), 0.5),
                crate::Rect::new(-width - 2.25, 5.75, width + 4.5, 4.5),
            );
        }
    }

    /// Subtree bounds include the authored hit cell even when its small path paints elsewhere.
    #[test]
    fn subtree_bounds_retain_authored_area_and_snapping_expansion() {
        let data = serde_json::from_value(serde_json::json!({
            "view_box": [0, 0, 16, 16], "stroke_width": 4,
            "paths": [[[0, 2], [16, 2]]],
        }))
        .unwrap();
        let bounds = bounds(&data).unwrap();
        let authored = crate::Rect::new(0.0, -10.0, 16.0, 16.0);
        let subtree = bounds.subtree_at(authored, 1.0);
        assert_eq!(authored.intersect(subtree), authored);
        assert!(bounds.at(authored, 1.0).y + bounds.at(authored, 1.0).h < 0.0);

        let data = serde_json::from_value(serde_json::json!({
            "view_box": [0, 0, 16, 16], "stroke_width": 4,
            "paths": [[[0, 8], [16, 8]]],
        }))
        .unwrap();
        let bounds = super::bounds(&data).unwrap();
        let snapped = bounds.at(crate::Rect::new(-19.0, 0.0, 17.0, 17.0), 1.0);
        let subtree = bounds.subtree_at(crate::Rect::new(-19.1, 0.0, 16.1, 16.1), 1.0);
        assert!((snapped.x + snapped.w - 0.625).abs() < 0.000001);
        assert_eq!(snapped.intersect(subtree), snapped);
    }
}
