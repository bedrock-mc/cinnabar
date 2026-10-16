//! Retained vertex buffers upload bounded disjoint changes through reusable staging.

use super::*;
use crate::upload_staging::{BufferUploadStaging, write_batch};

/// Maximum copy ranges per list, bounding preparation and staging metadata.
const MAX_UPLOAD_RANGES: usize = 16;
/// Whole vertices compared together before refining each dirty page's endpoints.
const COMPARE_VERTICES: usize = COMPARE_BLOCK_BYTES / VERTEX_BYTES as usize;

/// Upload dependencies shared by all lists prepared in one frame.
pub(super) struct VertexUpload<'a> {
    pub(super) device: &'a RenderDevice,
    pub(super) queue: &'a RenderQueue,
    pub(super) staging: Option<&'a BufferUploadStaging>,
}

/// One storage buffer of vertices with its live count and bind group.
pub(super) struct VertexList {
    pub(super) buffer: Option<Buffer>,
    capacity_vertices: u64,
    pub(super) count: u32,
    pub(super) bind_group: Option<BindGroup>,
    pub(super) runs: Vec<render_model::block_entity::draw_ranges::DrawRun>,
    dirty: Vec<Range<usize>>,
}

impl VertexList {
    pub(super) const fn new() -> Self {
        Self {
            buffer: None,
            capacity_vertices: 0,
            count: 0,
            bind_group: None,
            runs: Vec::new(),
            dirty: Vec::new(),
        }
    }

    /// Keeps the live prefix current, writing only bounded dirty ranges unless storage grows.
    pub(super) fn upload(
        &mut self,
        previous: &[BlockEntityVertex],
        vertices: &[BlockEntityVertex],
        upload: &VertexUpload<'_>,
        label: &'static str,
    ) {
        self.count = u32::try_from(vertices.len()).unwrap_or(0);
        if vertices.is_empty() {
            render_model::block_entity::draw_ranges::rebuild_runs(&mut self.runs, vertices);
            return;
        }
        let needed = vertices.len() as u64;
        let reallocated = self.buffer.is_none() || self.capacity_vertices < needed;
        if reallocated {
            let capacity = needed.next_power_of_two().max(MIN_BUFFER_VERTICES);
            self.buffer = Some(upload.device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: capacity * VERTEX_BYTES,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity_vertices = capacity;
            self.bind_group = None;
            self.dirty.clear();
            self.dirty.push(0..vertices.len());
        } else if !changed_ranges(previous, vertices, &mut self.dirty) {
            return;
        }
        render_model::block_entity::draw_ranges::rebuild_runs(&mut self.runs, vertices);
        if let Some(buffer) = &self.buffer {
            for range in &self.dirty {
                write_batch(
                    upload.staging,
                    upload.device,
                    upload.queue,
                    &[(
                        buffer,
                        range.start as u64 * VERTEX_BYTES,
                        bytemuck::cast_slice(&vertices[range.clone()]),
                    )],
                );
            }
        }
    }
}

/// Vertices of `vertices` that must be written over a buffer holding `previous`, or `None`
/// when both lists are bytewise identical. Bytes, not float values, are compared, so signed
/// zeros and NaN payloads are never skipped.
pub(super) fn changed_span(
    previous: &[BlockEntityVertex],
    vertices: &[BlockEntityVertex],
) -> Option<Range<usize>> {
    let start = matching_prefix(previous, vertices);
    if previous.len() != vertices.len() {
        return Some(start..vertices.len());
    }
    if start == vertices.len() {
        return None;
    }
    let end = vertices.len() - matching_suffix(&previous[start..], &vertices[start..]);
    Some(start..end)
}

/// Number of leading vertices whose bytes are equal in both lists.
fn matching_prefix(left: &[BlockEntityVertex], right: &[BlockEntityVertex]) -> usize {
    let shared = left.len().min(right.len());
    let left: &[u8] = bytemuck::cast_slice(&left[..shared]);
    let right: &[u8] = bytemuck::cast_slice(&right[..shared]);
    let mut start = 0;
    while start < left.len() {
        let end = (start + COMPARE_BLOCK_BYTES).min(left.len());
        if left[start..end] != right[start..end] {
            let byte = left[start..end]
                .iter()
                .zip(&right[start..end])
                .position(|(a, b)| a != b)
                .map_or(end, |offset| start + offset);
            return byte / VERTEX_BYTES as usize;
        }
        start = end;
    }
    shared
}

/// Number of trailing vertices whose bytes are equal in two lists of the same length.
fn matching_suffix(left: &[BlockEntityVertex], right: &[BlockEntityVertex]) -> usize {
    debug_assert_eq!(left.len(), right.len());
    let left: &[u8] = bytemuck::cast_slice(left);
    let right: &[u8] = bytemuck::cast_slice(right);
    let mut end = left.len();
    while end > 0 {
        let start = end.saturating_sub(COMPARE_BLOCK_BYTES);
        if left[start..end] != right[start..end] {
            let byte = left[start..end]
                .iter()
                .zip(&right[start..end])
                .rposition(|(a, b)| a != b)
                .map_or(start, |offset| start + offset);
            return (left.len() - byte - 1) / VERTEX_BYTES as usize;
        }
        end = start;
    }
    left.len() / VERTEX_BYTES as usize
}

/// Finds disjoint dirty pages without allocating after warmup; byte equality preserves NaNs
/// and signed zeros. Length changes write the shifted tail. Excess ranges join the last one.
fn changed_ranges(
    previous: &[BlockEntityVertex],
    vertices: &[BlockEntityVertex],
    ranges: &mut Vec<Range<usize>>,
) -> bool {
    ranges.clear();
    let Some(span) = changed_span(previous, vertices) else {
        return false;
    };
    if previous.len() != vertices.len() {
        if !span.is_empty() {
            ranges.push(span);
        }
        return true;
    }
    let mut first = span.start;
    while first < span.end {
        let end = (first + COMPARE_VERTICES).min(span.end);
        let left = &previous[first..end];
        let right = &vertices[first..end];
        let skip = matching_prefix(left, right);
        if skip < right.len() {
            let start = first + skip;
            let end = end - matching_suffix(&left[skip..], &right[skip..]);
            let full = ranges.len() == MAX_UPLOAD_RANGES;
            if let Some(last) = ranges.last_mut()
                && (last.end == start || full)
            {
                last.end = end;
            } else {
                ranges.push(start..end);
            }
        }
        first = (first + COMPARE_VERTICES).min(span.end);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_updates_reconstruct_all_bytes_without_copying_the_unchanged_gap() {
        let original = vec![BlockEntityVertex::default(); COMPARE_VERTICES * 12];
        let mut next = original.clone();
        next[7].position[0] = -0.0;
        next[COMPARE_VERTICES * 10 + 3].position[1] = f32::from_bits(0x7fc0_0001);
        let mut ranges = Vec::new();
        assert!(changed_ranges(&original, &next, &mut ranges));
        assert_eq!(ranges.len(), 2);
        assert!(ranges.iter().map(Range::len).sum::<usize>() < next.len() / 4);
        let mut applied = original;
        for range in ranges {
            applied[range.clone()].copy_from_slice(&next[range]);
        }
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&applied),
            bytemuck::cast_slice::<_, u8>(&next)
        );
    }

    #[test]
    fn dirty_range_ceiling_and_length_changes_preserve_the_live_prefix() {
        let original = vec![BlockEntityVertex::default(); COMPARE_VERTICES * MAX_UPLOAD_RANGES * 4];
        let mut next = original.clone();
        for index in (0..next.len()).step_by(COMPARE_VERTICES * 2) {
            next[index].uv[0] = 1.0;
        }
        let mut ranges = Vec::new();
        assert!(changed_ranges(&original, &next, &mut ranges));
        assert!(ranges.len() <= MAX_UPLOAD_RANGES);
        let mut applied = original.clone();
        for range in &ranges {
            applied[range.clone()].copy_from_slice(&next[range.clone()]);
        }
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&applied),
            bytemuck::cast_slice::<_, u8>(&next)
        );
        assert!(!changed_ranges(&next, &next, &mut ranges));
        assert!(ranges.is_empty());
        assert!(changed_ranges(&original, &original[..5], &mut ranges));
        assert!(ranges.is_empty());
        next.push(BlockEntityVertex::default());
        assert!(changed_ranges(&original, &next, &mut ranges));
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..next.len());
    }
}
