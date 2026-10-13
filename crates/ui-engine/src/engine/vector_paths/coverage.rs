//! Prepared fill-area coverage and pretransformed stroke distance bounds.

use super::Key;

mod fill;
use fill::Fill;

struct Segment {
    a: [f32; 2],
    b: [f32; 2],
    delta: [f32; 2],
    length_squared: f32,
    min: [f32; 2],
    max: [f32; 2],
}

impl Segment {
    /// Prepares finite segment arithmetic and its conservative distance bounds once.
    fn new(a: [f32; 2], b: [f32; 2]) -> Option<Self> {
        let delta = [b[0] - a[0], b[1] - a[1]];
        let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
        if !length_squared.is_finite() {
            return None;
        }
        Some(Self {
            a,
            b,
            delta,
            length_squared,
            min: [a[0].min(b[0]), a[1].min(b[1])],
            max: [a[0].max(b[0]), a[1].max(b[1])],
        })
    }

    /// Rejects edges whose bounding box cannot improve the nearest relevant squared distance.
    fn candidate(&self, point: [f32; 2], distance_squared: f32) -> bool {
        let delta = std::array::from_fn::<_, 2, _>(|axis| {
            (self.min[axis] - point[axis])
                .max(point[axis] - self.max[axis])
                .max(0.0)
        });
        delta[0] * delta[0] + delta[1] * delta[1] <= distance_squared
    }

    /// The squared distance to a closed segment, including coincident endpoints.
    fn distance_squared(&self, point: [f32; 2]) -> f32 {
        let t = if self.length_squared > 0.0 {
            (((point[0] - self.a[0]) * self.delta[0] + (point[1] - self.a[1]) * self.delta[1])
                / self.length_squared)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let delta = [
            point[0] - self.a[0] - t * self.delta[0],
            point[1] - self.a[1] - t * self.delta[1],
        ];
        delta[0] * delta[0] + delta[1] * delta[1]
    }

    /// Tests an authored point against the exact line, without merging nearby outlines.
    fn collinear(&self, point: [f32; 2]) -> bool {
        let delta = std::array::from_fn::<_, 2, _>(|axis| {
            f64::from(self.b[axis]) - f64::from(self.a[axis])
        });
        delta[0] * (f64::from(point[1]) - f64::from(self.a[1]))
            == delta[1] * (f64::from(point[0]) - f64::from(self.a[0]))
    }
}

/// Keeps only odd-covered intervals on each authored line, independent of edge subdivision.
fn cancel_fill_edges(segments: Vec<Segment>) -> Option<Vec<Segment>> {
    let mut lines: Vec<Vec<Segment>> = Vec::new();
    for segment in segments {
        if segment.a == segment.b {
            continue;
        }
        if let Some(line) = lines
            .iter_mut()
            .find(|line| line[0].collinear(segment.a) && line[0].collinear(segment.b))
        {
            line.push(segment);
        } else {
            lines.push(vec![segment]);
        }
    }
    let mut retained = Vec::new();
    for line in lines {
        let axis = usize::from(line[0].delta[1].abs() > line[0].delta[0].abs());
        let mut endpoints: Vec<_> = line.into_iter().flat_map(|edge| [edge.a, edge.b]).collect();
        endpoints.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
        for pair in endpoints.as_chunks::<2>().0 {
            if pair[0] != pair[1] {
                retained.push(Segment::new(pair[0], pair[1])?);
            }
        }
    }
    Some(retained)
}

pub(super) struct Coverage {
    segments: Vec<Segment>,
    bounds: Option<([f32; 2], [f32; 2])>,
    half_stroke: Option<f32>,
    fill: Option<Fill>,
    pub(super) pixel: f32,
}

impl Coverage {
    /// Converts authored coordinates once; geometry distances thereafter use logical pixels.
    pub(super) fn new(key: &Key) -> Option<Self> {
        let size = key.size.map(f32::from_bits);
        let [x, y, width, height] = key.path.view_box.map(f32::from_bits);
        let scale = [size[0] / width, size[1] / height];
        let pixel = 1.0 / f32::from_bits(key.dpi);
        let half_stroke = key
            .path
            .stroke_width
            .map(|width| f32::from_bits(width) * scale[0].min(scale[1]) * 0.5);
        if !pixel.is_finite() || pixel <= 0.0 || half_stroke.is_some_and(|width| !width.is_finite())
        {
            return None;
        }
        let mut segments = Vec::with_capacity(key.path.contours.iter().map(Vec::len).sum());
        for contour in &key.path.contours {
            let points: Vec<_> = contour
                .iter()
                .map(|point| point.map(f32::from_bits))
                .collect();
            let count = if key.path.closed || half_stroke.is_none() {
                points.len()
            } else {
                points.len() - 1
            };
            for index in 0..count {
                segments.push(Segment::new(
                    points[index],
                    points[(index + 1) % points.len()],
                )?);
            }
        }
        if half_stroke.is_none() {
            segments = cancel_fill_edges(segments)?;
        }
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        let transform = |point: [f32; 2]| [(point[0] - x) * scale[0], (point[1] - y) * scale[1]];
        let segments: Vec<_> = segments
            .into_iter()
            .map(|segment| {
                let a = transform(segment.a);
                let b = transform(segment.b);
                if a.into_iter().chain(b).any(|value| !value.is_finite()) {
                    return None;
                }
                for axis in 0..2 {
                    min[axis] = min[axis].min(a[axis]).min(b[axis]);
                    max[axis] = max[axis].max(a[axis]).max(b[axis]);
                }
                Segment::new(a, b)
            })
            .collect::<Option<_>>()?;
        let bounds = (!segments.is_empty()).then_some((min, max));
        let fill = if half_stroke.is_none() {
            Some(Fill::new(&segments, pixel)?)
        } else {
            None
        };
        Some(Self {
            segments,
            bounds,
            half_stroke,
            fill,
            pixel,
        })
    }

    /// Bounds segment work per sample for strokes or prepared fill spans.
    pub(super) fn sample_work(&self) -> usize {
        self.fill
            .as_ref()
            .map_or(self.segments.len(), Fill::sample_work)
    }

    /// Includes round caps, outside-view-box points, and half a physical pixel of coverage support.
    pub(super) fn raster_bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let (min, max) = self.bounds?;
        let padding = self.half_stroke.unwrap_or(0.0) + self.pixel * 0.5;
        Some((
            min.map(|value| value - padding),
            max.map(|value| value + padding),
        ))
    }

    /// Integrates each filled silhouette over one pixel, or softens the union of rounded strokes.
    pub(super) fn at(&self, point: [f32; 2]) -> f32 {
        if let Some(fill) = &self.fill {
            return fill.at(&self.segments, point, self.pixel);
        }
        let width = self.half_stroke.unwrap_or(0.0);
        let limit = width + self.pixel * 0.5;
        let mut distance_squared = limit * limit;
        for segment in &self.segments {
            if segment.candidate(point, distance_squared) {
                distance_squared = distance_squared.min(segment.distance_squared(point));
            }
        }
        (0.5 + (width - distance_squared.sqrt()) / self.pixel)
            .clamp(0.0, 1.0)
            .min((2.0 * width / self.pixel).min(1.0))
    }
}
