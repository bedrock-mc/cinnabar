//! Prepared even-odd scanline spans and exact pixel-box area coverage.

use std::{collections::BTreeMap, mem::size_of};

use super::super::{MAX_CACHED_BYTES, MAX_COVERAGE_WORK};
use super::Segment;

const MAX_BREAKS: usize = 2 + 2 * 2;
const MAX_SPAN_WORK: usize = 2 + (MAX_BREAKS - 1) * 2;

struct Band {
    min_y: f64,
    max_y: f64,
    spans: Vec<[usize; 2]>,
}

pub(super) struct Fill {
    bands: Vec<Band>,
    sample_work: usize,
}

/// Computes a non-horizontal boundary's X at a scanline using its endpoint as the origin.
fn edge_x(segment: &Segment, y: f64) -> f64 {
    f64::from(segment.a[0])
        + (y - f64::from(segment.a[1])) / (f64::from(segment.b[1]) - f64::from(segment.a[1]))
            * (f64::from(segment.b[0]) - f64::from(segment.a[0]))
}

/// Finds the Y where finite edges cross and can change their scanline order.
fn intersection_y(a: &Segment, b: &Segment) -> Option<f64> {
    let delta_a =
        std::array::from_fn::<_, 2, _>(|axis| f64::from(a.b[axis]) - f64::from(a.a[axis]));
    let delta_b =
        std::array::from_fn::<_, 2, _>(|axis| f64::from(b.b[axis]) - f64::from(b.a[axis]));
    let offset = std::array::from_fn::<_, 2, _>(|axis| f64::from(b.a[axis]) - f64::from(a.a[axis]));
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let denominator = cross(delta_a, delta_b);
    if denominator == 0.0 {
        return None;
    }
    let t = cross(offset, delta_b) / denominator;
    let u = cross(offset, delta_a) / denominator;
    (t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0).then(|| f64::from(a.a[1]) + t * delta_a[1])
}

/// Integrates one span after splitting at its intersections with the pixel's vertical sides.
fn span_area(edges: [&Segment; 2], min: [f64; 2], max: [f64; 2]) -> f64 {
    let mut breaks = [0.0; MAX_BREAKS];
    breaks[..2].copy_from_slice(&[min[1], max[1]]);
    let mut count = 2;
    for edge in edges {
        let delta_x = f64::from(edge.b[0]) - f64::from(edge.a[0]);
        if delta_x == 0.0 {
            continue;
        }
        for x in [min[0], max[0]] {
            let y = f64::from(edge.a[1])
                + (x - f64::from(edge.a[0])) / delta_x
                    * (f64::from(edge.b[1]) - f64::from(edge.a[1]));
            if y > min[1] && y < max[1] {
                breaks[count] = y;
                count += 1;
            }
        }
    }
    breaks[..count].sort_unstable_by(f64::total_cmp);
    breaks[..count]
        .windows(2)
        .map(|range| {
            let y = range[0] + (range[1] - range[0]) * 0.5;
            let width =
                (edge_x(edges[1], y).min(max[0]) - edge_x(edges[0], y).max(min[0])).max(0.0);
            width * (range[1] - range[0])
        })
        .sum()
}

impl Fill {
    /// Prepares stable scanline bands, bounding preprocessing work and retained span bytes.
    pub(super) fn new(segments: &[Segment], pixel: f32) -> Option<Self> {
        let mut heights: BTreeMap<_, _> = segments
            .iter()
            .flat_map(|edge| [f64::from(edge.a[1]), f64::from(edge.b[1])])
            .map(|y| (y.to_bits(), y))
            .collect();
        let mut work = 0;
        for (index, edge) in segments.iter().enumerate() {
            for other in &segments[index + 1..] {
                work += 1;
                if let Some(y) = intersection_y(edge, other) {
                    heights.insert(y.to_bits(), y);
                }
                if work + heights.len() * segments.len() > MAX_COVERAGE_WORK {
                    return None;
                }
            }
        }
        let mut breaks: Vec<_> = heights.into_values().collect();
        breaks.sort_unstable_by(f64::total_cmp);
        breaks.dedup();
        let mut bands = Vec::with_capacity(breaks.len().saturating_sub(1));
        let mut crossings = Vec::with_capacity(segments.len());
        let mut bytes = breaks.len() * size_of::<Band>();
        for range in breaks.windows(2) {
            work += segments.len();
            if work > MAX_COVERAGE_WORK {
                return None;
            }
            let y = range[0] + (range[1] - range[0]) * 0.5;
            crossings.clear();
            for (index, edge) in segments.iter().enumerate() {
                if (f64::from(edge.a[1]) > y) != (f64::from(edge.b[1]) > y) {
                    crossings.push((edge_x(edge, y), index));
                }
            }
            crossings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            if !crossings.len().is_multiple_of(2) {
                return None;
            }
            bytes += crossings.len() / 2 * size_of::<[usize; 2]>();
            if bytes > MAX_CACHED_BYTES {
                return None;
            }
            bands.push(Band {
                min_y: range[0],
                max_y: range[1],
                spans: crossings
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| [pair[0].1, pair[1].1])
                    .collect(),
            });
        }
        let (mut end, mut spans, mut peak) = (0, 0, 0);
        for start in 0..bands.len() {
            while end < bands.len() && bands[end].min_y < bands[start].max_y + f64::from(pixel) {
                spans += bands[end].spans.len();
                end += 1;
            }
            peak = peak.max(spans);
            spans -= bands[start].spans.len();
        }
        Some(Self {
            bands,
            sample_work: peak * MAX_SPAN_WORK,
        })
    }

    /// Bounds boundary evaluation work for every span a one-pixel vertical window can touch.
    pub(super) fn sample_work(&self) -> usize {
        self.sample_work
    }

    /// Adds disjoint even-odd span areas inside the physical pixel centered at the sample.
    pub(super) fn at(&self, segments: &[Segment], point: [f32; 2], pixel: f32) -> f32 {
        let half = f64::from(pixel) * 0.5;
        let min = point.map(|value| f64::from(value) - half);
        let max = point.map(|value| f64::from(value) + half);
        let first = self.bands.partition_point(|band| band.max_y <= min[1]);
        let mut area = 0.0;
        for band in self.bands[first..]
            .iter()
            .take_while(|band| band.min_y < max[1])
        {
            for span in &band.spans {
                area += span_area(
                    span.map(|index| &segments[index]),
                    [min[0], min[1].max(band.min_y)],
                    [max[0], max[1].min(band.max_y)],
                );
            }
        }
        (area / (f64::from(pixel) * f64::from(pixel))).clamp(0.0, 1.0) as f32
    }
}
