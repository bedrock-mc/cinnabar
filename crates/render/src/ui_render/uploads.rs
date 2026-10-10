//! Upload only changed UI buffer spans while retaining the last resident bytes.

use std::{ops::Range, sync::Arc};

use render_model::{UiRenderInput, UiRenderVertex};

#[derive(Default)]
pub(super) struct BufferUploads {
    vertices: Arc<[UiRenderVertex]>,
    indices: Arc<[u32]>,
}

pub(super) struct UploadPlan {
    pub(super) vertices: Range<usize>,
    pub(super) indices: Range<usize>,
}

impl BufferUploads {
    /// Plan exact byte changes, forcing complete uploads for newly allocated buffers.
    pub(super) fn plan(
        &mut self,
        input: &UiRenderInput,
        fresh_vertices: bool,
        fresh_indices: bool,
    ) -> UploadPlan {
        let plan = UploadPlan {
            vertices: changed_range(&self.vertices, &input.vertices, fresh_vertices),
            indices: changed_range(&self.indices, &input.indices, fresh_indices),
        };
        self.vertices = Arc::clone(&input.vertices);
        self.indices = Arc::clone(&input.indices);
        plan
    }
}

/// Return the first through last changed element; every byte outside it is identical.
pub(super) fn changed_range<T: bytemuck::Pod>(old: &[T], new: &[T], fresh: bool) -> Range<usize> {
    if fresh || old.len() != new.len() {
        return 0..new.len();
    }
    let size = std::mem::size_of::<T>();
    let old: &[u8] = bytemuck::cast_slice(old);
    let new: &[u8] = bytemuck::cast_slice(new);
    if old.as_ptr() == new.as_ptr() {
        return 0..0;
    }
    // Compare blocks with the slice equality fast path, then locate the changed byte.
    const BLOCK: usize = 1024;
    let first = old
        .chunks(BLOCK)
        .zip(new.chunks(BLOCK))
        .enumerate()
        .find(|(_, (a, b))| a != b)
        .map(|(block, (a, b))| block * BLOCK + a.iter().zip(b).position(|(a, b)| a != b).unwrap());
    let Some(first) = first else { return 0..0 };
    let equal_tail = old
        .rchunks(BLOCK)
        .zip(new.rchunks(BLOCK))
        .enumerate()
        .find(|(_, (a, b))| a != b)
        .map(|(block, (a, b))| {
            block * BLOCK
                + a.iter()
                    .rev()
                    .zip(b.iter().rev())
                    .position(|(a, b)| a != b)
                    .unwrap()
        })
        .unwrap();
    first / size..(new.len() - equal_tail).div_ceil(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_reconstruct_exact_bytes_across_edits_resize_and_buffer_replacement() {
        let mut resident = vec![0_u32; 8];
        for next in [
            vec![0; 8],
            vec![0, 1, 0, 0, 0, 2, 0, 0],
            vec![3; 12],
            vec![3; 4],
            vec![],
            vec![5; 8],
        ] {
            let span = changed_range(&resident, &next, false);
            resident.resize(next.len(), 0);
            resident[span.clone()].copy_from_slice(&next[span]);
            assert_eq!(resident, next);
            assert_eq!(changed_range(&resident, &next, false), 0..0);
            assert_eq!(changed_range(&resident, &next, true), 0..next.len());
        }
    }

    #[test]
    fn signed_zero_and_every_vertex_field_are_compared_by_bytes() {
        let vertex = UiRenderVertex {
            position: [0.0; 2],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color: [0; 4],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        };
        let old = [vertex; 3];
        for field in 0..9 {
            let mut new = old;
            match field {
                0 => new[1].position[0] = -0.0,
                1 => new[1].uv[1] = 0.5,
                2 => new[1].color[3] = 1,
                3 => new[1].style_flags = 1,
                4 => new[1].clip_z = 0.5,
                5 => new[1].clip_w = 2.0,
                6 => new[1].alpha_cutoff = 0.1,
                7 => new[1].model_light = 0.718_629,
                _ => new[1].overlay_color[3] = 0.7,
            }
            assert_eq!(changed_range(&old, &new, false), 1..2);
        }
    }

    #[test]
    fn spans_are_exact_across_comparison_block_boundaries() {
        for len in [1, 255, 256, 257, 600] {
            let old = vec![0_u32; len];
            for first in [0, len / 2, len - 1] {
                for last in [first, len - 1] {
                    let mut new = old.clone();
                    new[first] = 1;
                    new[last] = u32::MAX;
                    assert_eq!(changed_range(&old, &new, false), first..last + 1);
                }
            }
            assert_eq!(changed_range(&old, &old.clone(), false), 0..0);
        }
    }

    #[test]
    #[ignore = "benchmark"]
    fn frame_cost_bench_ui_upload_spans_moving_nametags() {
        let vertex = UiRenderVertex {
            position: [0.0; 2],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color: [255; 4],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        };
        let old = vec![vertex; 12_000];
        let mut new = old.clone();
        for v in &mut new[10_800..] {
            v.position[0] = 1.0;
        }
        let indices: Vec<u32> = (0..18_000).collect();
        const FRAMES: u32 = 2_000;
        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            std::hint::black_box((new.clone(), indices.clone()));
        }
        let full = started.elapsed() / FRAMES;
        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            let vertices = changed_range(&old, &new, false);
            let changed_indices = changed_range(&indices, &indices, false);
            std::hint::black_box((new[vertices].to_vec(), indices[changed_indices].to_vec()));
        }
        let spans = started.elapsed() / FRAMES;
        let vertex_span = changed_range(&old, &new, false);
        assert_eq!(vertex_span, 10_800..12_000);
        assert!(changed_range(&indices, &indices, false).is_empty());
        eprintln!(
            "FRAME_COST ui_upload_spans: full_copy={:.3}ms span_scan_copy={:.3}ms bytes={}->{}",
            full.as_secs_f64() * 1e3,
            spans.as_secs_f64() * 1e3,
            old.len() * std::mem::size_of::<UiRenderVertex>()
                + indices.len() * std::mem::size_of::<u32>(),
            vertex_span.len() * std::mem::size_of::<UiRenderVertex>(),
        );
    }
}
