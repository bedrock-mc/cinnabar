use assets::{FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
use render_model::FontAtlasFrame;
use render_model::{
    UiRenderBatch, UiRenderInput, UiRenderVertex, UiScissor, UiTextureCatalog, UiTexturePage,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Makes a sparse source page with a glyph far from the demand atlas's first slot.
fn input() -> UiRenderInput {
    let side = 2048usize;
    let mut pixels = vec![0; side * side];
    for y in 100..118 {
        for x in 1100..1118 {
            pixels[y * side + x] = ((x * 7 + y * 13) % 256) as u8;
        }
    }
    let page = FontTexturePage {
        source_path: "font/sparse.png".into(),
        source_bytes: 1,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: side as u32,
        height: side as u32,
        pixels: FontPixels::Coverage(pixels.into()),
    };
    let glyph = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [1100, 100, 1118, 118],
        bearing: [0, 0],
        advance_64: 18 * 64,
    };
    let bytes = encode_font_catalog([2; 32], &[glyph], &[page]).unwrap();
    let font = Arc::new(RuntimeFontCatalog::decode(&bytes, [2; 32]).unwrap());
    let vertices =
        [[1100., 100.], [1118., 100.], [1118., 118.], [1100., 118.]].map(|uv| UiRenderVertex {
            position: [(uv[0] - 1100.) * 6.0 + 20., (uv[1] - 100.) * 6.0 + 20.],
            uv,
            clip_z: 0.,
            clip_w: 1.,
            color: [170, 210, 255, 255],
            style_flags: 0,
            alpha_cutoff: -1.,
            model_light: 1.,
            overlay_color: [0.; 4],
        });
    UiRenderInput {
        revision: 1,
        viewport_size: [256; 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: [0, 1, 2, 0, 2, 3].into(),
        batches: [UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 256, 256),
            0,
            6,
            0,
        )]
        .into(),
        textures: Arc::new(
            UiTextureCatalog::new(vec![UiTexturePage::font(font, 0).unwrap()], 1).unwrap(),
        ),
    }
}

#[test]
fn demand_pages_cannot_alias_fully_uploaded_pages_with_identical_source_pixels() {
    let input = input();
    let font = &input.textures.pages()[0];
    let full = UiTexturePage::coverage(font.dimensions(), font.pixels().into()).unwrap();
    assert_ne!(font.identity(), full.identity());
}

#[test]
fn first_frame_uploads_only_requested_texels_and_warm_frames_allocate_and_upload_nothing() {
    let input = input();
    let mut fonts = FontAtlasFrame::default();
    let mut written = 0;
    fonts
        .prepare(&input, |_, _, extent, pixels| {
            assert_eq!(extent, [20; 2]);
            written += pixels.len();
        })
        .unwrap();
    assert_eq!(written, 400);
    assert_eq!(
        input.textures.plan().bytes(),
        render_model::FONT_ATLAS_SIDE.pow(2) as usize
    );
    assert_ne!(fonts.vertices[0].atlas_offset, [0.; 2]);
    assert_eq!(fonts.vertices[0].source.uv, input.vertices[0].uv);
    fonts.commit_vertices();
    fonts
        .prepare(&input, |_, _, _, _| panic!("warm glyph uploaded twice"))
        .unwrap();
    fonts.commit_vertices();
    let before = crate::alloc_count::thread_allocations();
    for _ in 0..10 {
        fonts
            .prepare(&input, |_, _, _, _| panic!("warm glyph uploaded twice"))
            .unwrap();
        fonts.commit_vertices();
    }
    assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
    fonts.clear();
    let mut replacement_writes = 0;
    fonts
        .prepare(&input, |_, _, _, _| replacement_writes += 1)
        .unwrap();
    assert_eq!(
        replacement_writes, 1,
        "a new GPU allocation cannot inherit old residency"
    );
}

#[test]
fn actual_gpu_pixels_match_the_full_rgba_page_before_the_first_glyph_draw() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("demand font atlas") else {
        return;
    };
    let input = input();
    let source = &input.textures.pages()[0];
    let rgba: Vec<_> = source
        .pixels()
        .iter()
        .flat_map(|alpha| [255, 255, 255, *alpha])
        .collect();
    let mut fonts = FontAtlasFrame::default();
    let side = source.font_atlas_side().unwrap();
    let mut atlas = vec![0; (side * side) as usize];
    fonts
        .prepare(&input, |_, origin, size, pixels| {
            for y in 0..size[1] {
                let target = ((origin[1] + y) * side + origin[0]) as usize;
                let start = (y * size[0]) as usize;
                atlas[target..target + size[0] as usize]
                    .copy_from_slice(&pixels[start..start + size[0] as usize]);
            }
        })
        .unwrap();
    for flags in [
        0,
        8,
        u32::from(assets::FONT_STYLE_COVERAGE_GAMMA),
        u32::from(assets::FONT_STYLE_COVERAGE_GAMMA | assets::FONT_STYLE_SDF),
    ] {
        let before = raster(
            &gpu,
            &input.vertices,
            None,
            &input.indices,
            &rgba,
            source.dimensions(),
            false,
            flags,
        );
        let after = raster(
            &gpu,
            &input.vertices,
            Some(&fonts.vertices),
            &input.indices,
            &atlas,
            [side; 2],
            true,
            flags,
        );
        assert!(
            before == after,
            "source and resident coverage differ for flags {flags}: first channel {:?}, maximum delta {}",
            before.iter().zip(&after).position(|(a, b)| a != b),
            before
                .iter()
                .zip(&after)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0),
        );
    }
}

/// Draws the production fragment shader with fixture vertices and returns native GPU pixels.
fn raster(
    gpu: &crate::gpu_snapshot::Gpu,
    vertices: &[UiRenderVertex],
    resident: Option<&[render_model::FontAtlasVertex]>,
    indices: &[u32],
    pixels: &[u8],
    size: [u32; 2],
    coverage: bool,
    flags: u32,
) -> Vec<u8> {
    use wgpu::*;
    let texture = gpu.device.create_texture(&TextureDescriptor {
        label: Some("font residency pixel witness"),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: if coverage {
            TextureFormat::R8Unorm
        } else {
            TextureFormat::Rgba8Unorm
        },
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * if coverage { 1 } else { 4 }),
            rows_per_image: Some(size[1]),
        },
        texture.size(),
    );
    let view = texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..Default::default()
    });
    let viewport = gpu.buffer(&[256., 256., 0., 0.], BufferUsages::UNIFORM);
    let format = gpu.words(&[u32::from(coverage), 0, 0, 0], BufferUsages::UNIFORM);
    let nearest = gpu.device.create_sampler(&SamplerDescriptor::default());
    let linear = gpu.device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    let bindings = [
        BindGroupEntry {
            binding: 0,
            resource: viewport.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 1,
            resource: BindingResource::TextureView(&view),
        },
        BindGroupEntry {
            binding: 2,
            resource: BindingResource::Sampler(&nearest),
        },
        BindGroupEntry {
            binding: 3,
            resource: BindingResource::Sampler(&linear),
        },
        BindGroupEntry {
            binding: 4,
            resource: format.as_entire_binding(),
        },
    ];
    let values = |position: bool| {
        indices
            .iter()
            .map(|&index| {
                let vertex = vertices[index as usize];
                let v = if position { vertex.position } else { vertex.uv };
                format!("vec2<f32>({:?}, {:?})", v[0], v[1])
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let offsets = indices
        .iter()
        .map(|&index| {
            let v = resident.map_or([0.; 2], |vertices| vertices[index as usize].atlas_offset);
            format!("vec2<f32>({:?}, {:?})", v[0], v[1])
        })
        .collect::<Vec<_>>()
        .join(",");
    let shader = format!(
        "{}\n@vertex fn fixture(@builtin(vertex_index) i: u32) -> UiVertexOutput {{ let pos = array<vec2<f32>, 6>({}); let uv = array<vec2<f32>, 6>({}); var out: UiVertexOutput; out.clip_position = vec4<f32>(pos[i].x / 128.0 - 1.0, 1.0 - pos[i].y / 128.0, 0.0, 1.0); out.uv = uv[i]; let offsets = array<vec2<f32>, 6>({}); out.atlas_offset = offsets[i]; out.color = vec4<f32>(0.667, 0.824, 1.0, 1.0); out.texture_page = 0u; out.style_flags = {}u; out.alpha_cutoff = -1.0; out.model_light = 1.0; out.overlay_color = vec4<f32>(0.0); return out; }}",
        super::shader::source(include_str!("../ui.wgsl")),
        values(true),
        values(false),
        offsets,
        flags
    );
    gpu.render(
        &shader,
        "fixture",
        &[crate::gpu_snapshot::Draw {
            fragment: "ui_fragment",
            vertices: 0..6,
            bindings: &bindings,
            blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_depth: false,
        }],
    )
}

#[test]
fn font_upload_burst_measurement() {
    if std::env::var_os("CINNABAR_FONT_UPLOAD_BENCH").is_none() {
        eprintln!(
            "skipping font_upload_burst_measurement: missing CINNABAR_FONT_UPLOAD_BENCH fixture switch"
        );
        return;
    }
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("font upload bursts") else {
        return;
    };
    let mut input = input();
    let template = input.vertices[0];
    let side = input.textures.pages()[0].font_atlas_side().unwrap();
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("font upload burst measurement"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut atlas = FontAtlasFrame::default();
    let mut prepare_times = Vec::new();
    let mut completed_times = Vec::new();
    let mut bytes_written = 0;
    for frame in 0..50 {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for glyph in frame * 128..(frame + 1) * 128 {
            let x = (glyph % 100 * 20 + 2) as f32;
            let y = (glyph / 100 * 20 + 2) as f32;
            let base = vertices.len() as u32;
            vertices.extend(
                [[x, y], [x + 18., y], [x + 18., y + 18.], [x, y + 18.]]
                    .map(|uv| UiRenderVertex { uv, ..template }),
            );
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        input.vertices = vertices.into();
        input.indices = indices.into();
        input.batches = [UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 256, 256),
            0,
            input.indices.len() as u32,
            0,
        )]
        .into();
        input.validate().unwrap();
        let started = std::time::Instant::now();
        atlas
            .prepare(&input, |_, origin, size, pixels| {
                bytes_written += pixels.len();
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: origin[0],
                            y: origin[1],
                            z: 0,
                        },
                        aspect: Default::default(),
                    },
                    pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0]),
                        rows_per_image: Some(size[1]),
                    },
                    wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                );
            })
            .unwrap();
        atlas.commit_vertices();
        prepare_times.push(started.elapsed().as_secs_f64() * 1000.);
        gpu.queue.submit([]);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        completed_times.push(started.elapsed().as_secs_f64() * 1000.);
        atlas
            .prepare(&input, |_, _, _, _| panic!("warm glyph uploaded twice"))
            .unwrap();
        atlas.commit_vertices();
    }
    prepare_times.sort_by(f64::total_cmp);
    completed_times.sort_by(f64::total_cmp);
    eprintln!(
        "FONT_UPLOAD_BURSTS frames={} new_glyphs_per_frame=128 bytes={} prepare_p50_ms={:.4} prepare_max_ms={:.4} completed_p50_ms={:.4} completed_max_ms={:.4}",
        prepare_times.len(),
        bytes_written,
        prepare_times[25],
        prepare_times[49],
        completed_times[25],
        completed_times[49]
    );
}

#[test]
fn row_uploads_preserve_existing_glyphs_when_a_taller_glyph_is_added() {
    let mut input = input();
    let side = input.textures.pages()[0].font_atlas_side().unwrap();
    let mut pixels = vec![0; (side * side) as usize];
    let mut fonts = FontAtlasFrame::default();
    fonts
        .prepare(&input, |_, origin, size, bytes| {
            copy_upload(&mut pixels, side, origin, size, bytes)
        })
        .unwrap();
    fonts.commit_vertices();
    let mut vertices = input.vertices.to_vec();
    let template = vertices[0];
    vertices.extend(
        [[300., 400.], [330., 400.], [330., 430.], [300., 430.]]
            .map(|uv| UiRenderVertex { uv, ..template }),
    );
    input.vertices = vertices.into();
    input.indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7].into();
    input.batches = [UiRenderBatch::new(
        0,
        UiScissor::new(0, 0, 256, 256),
        0,
        12,
        0,
    )]
    .into();
    input.validate().unwrap();
    fonts
        .prepare(&input, |_, origin, size, bytes| {
            copy_upload(&mut pixels, side, origin, size, bytes)
        })
        .unwrap();
    let source = &input.textures.pages()[0];
    for quad in fonts.vertices.chunks_exact(4) {
        let [dx, dy] = quad[0].atlas_offset.map(|value| value as i32);
        for y in quad[0].source.uv[1] as i32..quad[2].source.uv[1] as i32 {
            for x in quad[0].source.uv[0] as i32..quad[2].source.uv[0] as i32 {
                assert_eq!(
                    pixels[((y + dy) as u32 * side + (x + dx) as u32) as usize],
                    source.pixels()[(y as u32 * source.dimensions()[0] + x as u32) as usize]
                );
            }
        }
    }
}

/// Applies one coverage upload to a CPU image of the retained GPU atlas.
fn copy_upload(target: &mut [u8], side: u32, origin: [u32; 2], size: [u32; 2], pixels: &[u8]) {
    for row in 0..size[1] {
        let start = ((origin[1] + row) * side + origin[0]) as usize;
        let from = (row * size[0]) as usize;
        target[start..start + size[0] as usize]
            .copy_from_slice(&pixels[from..from + size[0] as usize]);
    }
}
