//! Resolve font rectangles before the first draw, retaining bounded atlas slots between frames.

use crate::{
    FONT_ATLAS_GUTTER, FontAtlas, FontRect, UiRenderInput, UiRenderRejectReason, UiRenderVertex,
};

#[derive(Default)]
struct Page {
    identity: [u8; 32],
    dimensions: [u32; 2],
    format: crate::UiTextureFormat,
    atlas: FontAtlas,
    requests: Vec<FontRect>,
}

/// Keeps source UV interpolation unchanged; the fragment sampler applies the flat atlas offset.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FontAtlasVertex {
    pub source: UiRenderVertex,
    pub atlas_offset: [f32; 2],
}

#[derive(Default)]
pub struct FontAtlasFrame {
    pages: Vec<Page>,
    pub vertices: Vec<FontAtlasVertex>,
    pub previous_vertices: Vec<FontAtlasVertex>,
    scratch: Vec<u8>,
}

impl FontAtlasFrame {
    /// Drops placements when the GPU textures have been replaced.
    pub fn clear(&mut self) {
        for page in &mut self.pages {
            page.atlas.clear();
        }
    }

    /// Prepares an admitted publication, issuing writes only after every glyph has a slot.
    pub fn prepare(
        &mut self,
        input: &UiRenderInput,
        write: impl FnMut(usize, [u32; 2], [u32; 2], &[u8]),
    ) -> Result<(), UiRenderRejectReason> {
        let result = self.prepare_inner(input, write);
        if result.is_err() {
            self.clear();
        }
        result
    }

    /// Resolves placements and sampling offsets before copying any texels.
    fn prepare_inner(
        &mut self,
        input: &UiRenderInput,
        mut write: impl FnMut(usize, [u32; 2], [u32; 2], &[u8]),
    ) -> Result<(), UiRenderRejectReason> {
        self.pages
            .resize_with(input.textures.pages().len(), Page::default);
        for (page, source) in self.pages.iter_mut().zip(input.textures.pages()) {
            page.requests.clear();
            if page.identity != source.identity()
                || page.dimensions != source.dimensions()
                || page.format != source.format()
            {
                page.identity = source.identity();
                page.dimensions = source.dimensions();
                page.format = source.format();
                page.atlas = FontAtlas::new(source.font_atlas_side().unwrap_or(1));
            }
        }
        for batch in input.batches.iter() {
            let index = batch.texture_page as usize;
            let source = &input.textures.pages()[index];
            if source.font_atlas_side().is_none() {
                continue;
            }
            let page = &mut self.pages[index];
            for triangle in input.indices
                [batch.first_index as usize..(batch.first_index + batch.index_count) as usize]
                .chunks_exact(3)
            {
                let rect = triangle_rect(input, triangle, source.dimensions())?;
                page.requests.push(rect);
            }
        }
        for page in &mut self.pages {
            page.requests.sort_unstable();
            page.requests.dedup();
            if !page.atlas.prepare(&page.requests) {
                return Err(UiRenderRejectReason::InvalidTextureExtent);
            }
        }
        self.vertices.clear();
        self.vertices
            .extend(input.vertices.iter().map(|&source| FontAtlasVertex {
                source,
                atlas_offset: [f32::NAN; 2],
            }));
        for batch in input.batches.iter() {
            let index = batch.texture_page as usize;
            let source = &input.textures.pages()[index];
            if source.font_atlas_side().is_none() {
                continue;
            }
            let page = &self.pages[index];
            for triangle in input.indices
                [batch.first_index as usize..(batch.first_index + batch.index_count) as usize]
                .chunks_exact(3)
            {
                let rect = triangle_rect(input, triangle, source.dimensions())?;
                let origin = page
                    .atlas
                    .origin(rect)
                    .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
                for &vertex in triangle {
                    let offset = [
                        (i32::from(origin[0]) - i32::from(rect[0])) as f32,
                        (i32::from(origin[1]) - i32::from(rect[1])) as f32,
                    ];
                    let target = &mut self.vertices[vertex as usize].atlas_offset;
                    if !target[0].is_nan() && *target != offset {
                        return Err(UiRenderRejectReason::InvalidTextureExtent);
                    }
                    *target = offset;
                }
            }
        }
        for vertex in &mut self.vertices {
            if vertex.atlas_offset[0].is_nan() {
                vertex.atlas_offset = [0.; 2];
            }
        }
        for (index, page) in self.pages.iter().enumerate() {
            let source = &input.textures.pages()[index];
            let stride = source.format().bytes_per_texel();
            let uploads = page.atlas.uploads();
            let pad = u32::from(FONT_ATLAS_GUTTER);
            let mut start = 0;
            while start < uploads.len() {
                let first = page.atlas.origin(uploads[start]).unwrap().map(u32::from);
                let origin = [first[0] - pad, first[1] - pad];
                let mut end = start;
                let mut extent = [0; 2];
                while let Some(&rect) = uploads.get(end) {
                    let [x, y] = page.atlas.origin(rect).unwrap().map(u32::from);
                    if y != first[1] {
                        break;
                    }
                    extent[0] = x + u32::from(rect[2] - rect[0]) + pad - origin[0];
                    extent[1] = extent[1].max(u32::from(rect[3] - rect[1]) + 2 * pad);
                    end += 1;
                }
                // Fresh slots are contiguous along a shelf, so the strip cannot erase residents.
                let row_bytes = extent[0] as usize * stride;
                self.scratch.resize(row_bytes * extent[1] as usize, 0);
                self.scratch.fill(0);
                for &rect in &uploads[start..end] {
                    let [x, y] = page.atlas.origin(rect).unwrap().map(u32::from);
                    copy_glyph(
                        source,
                        rect,
                        [x - pad - origin[0], y - pad - origin[1]],
                        row_bytes,
                        &mut self.scratch,
                    );
                }
                write(index, origin, extent, &self.scratch);
                start = end;
            }
        }
        Ok(())
    }

    /// Retains the uploaded vertices without allocating for a warmed publication.
    pub fn commit_vertices(&mut self) {
        std::mem::swap(&mut self.vertices, &mut self.previous_vertices);
    }
}

/// Covers all samples of one glyph triangle while retaining fractional UVs for remapping.
fn triangle_rect(
    input: &UiRenderInput,
    triangle: &[u32],
    [width, height]: [u32; 2],
) -> Result<FontRect, UiRenderRejectReason> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for &index in triangle {
        let uv = input.vertices[index as usize].uv;
        for axis in 0..2 {
            min[axis] = min[axis].min(uv[axis]);
            max[axis] = max[axis].max(uv[axis]);
        }
    }
    if min[0] < 0.0
        || min[1] < 0.0
        || max[0] > width as f32
        || max[1] > height as f32
        || min[0] >= max[0]
        || min[1] >= max[1]
    {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    Ok([
        min[0].floor() as u16,
        min[1].floor() as u16,
        max[0].ceil() as u16,
        max[1].ceil() as u16,
    ])
}

/// Copies one glyph and its original sampling neighbors into an admitted upload strip.
fn copy_glyph(
    source: &crate::UiTexturePage,
    rect: FontRect,
    offset: [u32; 2],
    row_bytes: usize,
    target: &mut [u8],
) {
    let [width, height] = source.dimensions();
    let stride = source.format().bytes_per_texel();
    let pixels = source.pixels();
    let pad = u32::from(FONT_ATLAS_GUTTER);
    let extent = [
        u32::from(rect[2] - rect[0]) + 2 * pad,
        u32::from(rect[3] - rect[1]) + 2 * pad,
    ];
    for row in 0..extent[1] {
        let sy = (i64::from(rect[1]) + i64::from(row) - i64::from(pad))
            .clamp(0, i64::from(height - 1)) as usize;
        for col in 0..extent[0] {
            let sx = (i64::from(rect[0]) + i64::from(col) - i64::from(pad))
                .clamp(0, i64::from(width - 1)) as usize;
            let from = (sy * width as usize + sx) * stride;
            let to = (offset[1] + row) as usize * row_bytes + (offset[0] + col) as usize * stride;
            target[to..to + stride].copy_from_slice(&pixels[from..from + stride]);
        }
    }
}
