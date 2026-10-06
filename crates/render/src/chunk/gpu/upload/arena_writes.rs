use std::num::NonZeroU64;

use crate::chunk::*;

/// Arena writes collected while GPU preparation admits uploads.
#[derive(Default)]
pub(in crate::chunk) struct ArenaWrites {
    pub(in crate::chunk) quads: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) model: Vec<(u32, Vec<[u32; 4]>)>,
    pub(in crate::chunk) model_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) model_draw: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) transparent_model_draw: Vec<(u32, Vec<[u32; 2]>)>,
    pub(in crate::chunk) liquid: Vec<(u32, Vec<[u32; 4]>)>,
    pub(in crate::chunk) liquid_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) cube_lighting: Vec<(u32, Vec<[u16; 4]>)>,
    pub(in crate::chunk) biome: Vec<(u32, Vec<u32>)>,
    pub(in crate::chunk) origins: Vec<(u32, GpuChunkOrigin)>,
}

impl ArenaWrites {
    /// Every admitted upload writes an origin record.
    pub(in crate::chunk) fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    /// Stages the collected writes into the arena's current buffers. The next
    /// queue submit runs them first, so they must be issued before a
    /// migration slice copies the regions they touch.
    pub(in crate::chunk) fn issue(&mut self, arena: &ChunkGpuArena, render_queue: &RenderQueue) {
        let quads = std::mem::take(&mut self.quads);
        let origins = std::mem::take(&mut self.origins);
        let model = std::mem::take(&mut self.model);
        let model_lighting = std::mem::take(&mut self.model_lighting);
        let model_draw = std::mem::take(&mut self.model_draw);
        let transparent = std::mem::take(&mut self.transparent_model_draw);
        let liquid = std::mem::take(&mut self.liquid);
        let liquid_lighting = std::mem::take(&mut self.liquid_lighting);
        let cube_lighting = std::mem::take(&mut self.cube_lighting);
        let biome = std::mem::take(&mut self.biome);

        let mut staged = Vec::new();
        stage(&mut staged, PACKED_QUAD_BYTES, &quads);
        write_merged(render_queue, &arena.quad_buffer, &mut staged);
        staged.extend(origins.iter().map(|(index, origin)| {
            (
                u64::from(*index) * CHUNK_ORIGIN_BYTES,
                bytemuck::bytes_of(origin),
            )
        }));
        write_merged(render_queue, &arena.origin_buffer, &mut staged);
        let word = GEOMETRY_STREAM_WORD_BYTES;
        stage(&mut staged, word, &model);
        stage(&mut staged, word, &model_lighting);
        stage(&mut staged, word, &model_draw);
        stage(&mut staged, word, &transparent);
        stage(&mut staged, word, &liquid);
        stage(&mut staged, word, &liquid_lighting);
        stage(&mut staged, word, &cube_lighting);
        write_merged(render_queue, &arena.geometry_stream_buffer, &mut staged);
        stage(&mut staged, BIOME_WORD_BYTES, &biome);
        write_merged(render_queue, &arena.biome_buffer, &mut staged);
    }
}

/// Appends each non-empty record run as its byte offset and bytes, in issue order.
fn stage<'a, T: bytemuck::Pod>(
    staged: &mut Vec<(u64, &'a [u8])>,
    item_bytes: u64,
    writes: &'a [(u32, Vec<T>)],
) {
    staged.extend(
        writes
            .iter()
            .filter(|(_, records)| !records.is_empty())
            .map(|(offset, records)| {
                (
                    u64::from(*offset) * item_bytes,
                    bytemuck::cast_slice(records),
                )
            }),
    );
}

/// Issues `staged` into `buffer`, one staged write per run of abutting ranges, since each
/// `write_buffer` call allocates its own staging memory. Overlapping ranges keep their issue
/// order unmerged, so the buffer ends identical either way. Drains `staged`.
fn write_merged(render_queue: &RenderQueue, buffer: &Buffer, staged: &mut Vec<(u64, &[u8])>) {
    let mut sorted = staged.clone();
    sorted.sort_by_key(|(offset, _)| *offset);
    let overlapping = sorted
        .windows(2)
        .any(|pair| pair[0].0 + pair[0].1.len() as u64 > pair[1].0);
    if overlapping {
        for (offset, bytes) in staged.drain(..) {
            render_queue.write_buffer(buffer, offset, bytes);
        }
        return;
    }
    staged.clear();
    for run in abutting_runs(&sorted) {
        let run = &sorted[run];
        let offset = run[0].0;
        if let [(_, bytes)] = run {
            render_queue.write_buffer(buffer, offset, bytes);
            continue;
        }
        let size = run.iter().map(|(_, bytes)| bytes.len() as u64).sum();
        if let Some(size) = NonZeroU64::new(size)
            && let Some(mut view) = render_queue.write_buffer_with(buffer, offset, size)
        {
            let mut at = 0;
            for (_, bytes) in run {
                view[at..at + bytes.len()].copy_from_slice(bytes);
                at += bytes.len();
            }
        }
    }
}

/// Index ranges of `sorted` whose byte ranges each start where the previous one ends.
fn abutting_runs(sorted: &[(u64, &[u8])]) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < sorted.len() {
        let mut end = start + 1;
        let mut next = sorted[start].0 + sorted[start].1.len() as u64;
        while end < sorted.len() && sorted[end].0 == next {
            next += sorted[end].1.len() as u64;
            end += 1;
        }
        runs.push(start..end);
        start = end;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Merged runs write exactly the bytes the individual writes would, in any issue order.
    #[test]
    fn abutting_runs_reproduce_individual_writes() {
        let records: Vec<Vec<u8>> = (0..12_u8)
            .map(|seed| vec![seed; 4 + 4 * (seed as usize % 3)])
            .collect();
        let mut offset = 0;
        let mut writes = Vec::new();
        for (index, bytes) in records.iter().enumerate() {
            writes.push((offset, bytes.as_slice()));
            // Leave a gap after every third record so runs split.
            offset += bytes.len() as u64 + if index % 3 == 2 { 8 } else { 0 };
        }
        writes.reverse();
        let apply = |buffer: &mut Vec<u8>, at: u64, bytes: &[u8]| {
            buffer[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
        };
        let mut expected = vec![0; offset as usize];
        for (at, bytes) in &writes {
            apply(&mut expected, *at, bytes);
        }
        let mut sorted = writes.clone();
        sorted.sort_by_key(|(at, _)| *at);
        let runs = abutting_runs(&sorted);
        assert_eq!(runs.len(), 4);
        let mut merged = vec![0; offset as usize];
        for run in runs {
            let joined: Vec<u8> = sorted[run.clone()]
                .iter()
                .flat_map(|(_, bytes)| bytes.iter().copied())
                .collect();
            apply(&mut merged, sorted[run.start].0, &joined);
        }
        assert_eq!(merged, expected);
    }
}
