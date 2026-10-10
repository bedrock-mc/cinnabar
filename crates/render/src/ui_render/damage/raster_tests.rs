//! Exact pixel comparisons for damage replay through the production UI shader.

#[path = "raster_fixture.rs"]
mod fixture;

use super::*;
use fixture::{Raster, SIDE, buffer, target};
use render_model::{UiRenderBatch, UiTextureCatalog, UiTexturePage};

impl Raster {
    /// Clears the whole layer or its damage rectangle, then replays every overlapping batch in order.
    fn draw(&self, input: &UiRenderInput, output: &wgpu::Texture, damage: Option<UiScissor>) {
        let resident: Vec<_> = input
            .vertices
            .iter()
            .map(|&source| render_model::FontAtlasVertex {
                source,
                atlas_offset: [0.; 2],
            })
            .collect();
        let vertices = buffer(
            &self.gpu,
            bytemuck::cast_slice(&resident),
            wgpu::BufferUsages::VERTEX,
        );
        let indices = buffer(
            &self.gpu,
            bytemuck::cast_slice(&input.indices),
            wgpu::BufferUsages::INDEX,
        );
        let color = output.create_view(&Default::default());
        let depth = target(&self.gpu, true).create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut start = 0;
        let mut lifetime = super::super::model_depth::ModelDepthLifetime::default();
        while let Some(first) = input.batches.get(start) {
            let mode = (
                first.isolated_depth_scope,
                first.depth_test,
                first.depth_write,
            );
            let length = input.batches[start..]
                .iter()
                .take_while(|batch| {
                    (
                        batch.isolated_depth_scope,
                        batch.depth_test,
                        batch.depth_write,
                    ) == mode
                })
                .count();
            lifetime.enter(mode.0);
            let needs_depth = mode.1 != 0 || mode.2 != 0;
            let initial = start == 0;
            let batches = &input.batches[start..start + length];
            start += length;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("UI damage parity draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if initial && damage.is_none() {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: needs_depth.then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: if lifetime.cleared() {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(0.0)
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if initial
                && let Some(rect) = damage.and_then(|rect| {
                    crate::render_bounds::scissor(rect, [output.width(), output.height()])
                })
            {
                pass.set_pipeline(&self.clear);
                pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
                pass.draw(0..3, 0..1);
            }
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_bind_group(0, &self.binding, &[]);
            pass.set_pipeline(
                &self.materials[usize::from(mode.1 != 0) * 2 + usize::from(mode.2 != 0)],
            );
            for batch in batches {
                let Some(rect) = super::super::layer::clipped_scissor(batch.scissor, damage)
                    .and_then(|rect| {
                        crate::render_bounds::scissor(rect, [output.width(), output.height()])
                    })
                else {
                    continue;
                };
                pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
                pass.draw_indexed(
                    batch.first_index..batch.first_index + batch.index_count,
                    0,
                    batch.texture_page..batch.texture_page + 1,
                );
            }
            if needs_depth {
                lifetime.encoded();
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
    }

    /// Reads all RGBA bytes without image conversion or comparison tolerances.
    fn pixels(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let (width, height) = (texture.width(), texture.height());
        let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        readback
            .slice(..)
            .get_mapped_range()
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect()
    }

    /// Writes optional native offscreen frames without opening a window.
    fn snapshot(&self, texture: &wgpu::Texture) {
        let Some(directory) = std::env::var_os("CINNABAR_UI_RASTER_SNAPSHOT_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let image =
            image::RgbaImage::from_raw(texture.width(), texture.height(), self.pixels(texture))
                .unwrap();
        image
            .save(directory.join(format!(
                "native-ui-{}x{}.png",
                texture.width(),
                texture.height()
            )))
            .unwrap();
    }
}

/// Adds a textured quad; depth-enabled quads share one isolated model lifetime.
fn quad(
    input: &mut UiRenderInput,
    bounds: [f32; 4],
    color: [u8; 4],
    page: u32,
    depth: Option<f32>,
) {
    let base = input.vertices.len() as u32;
    let first = input.indices.len() as u32;
    let mut vertices = input.vertices.to_vec();
    for ([x, y], uv) in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
        [bounds[0], bounds[3]],
    ]
    .into_iter()
    .zip([[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]])
    {
        vertices.push(UiRenderVertex {
            position: [x, y],
            clip_z: depth.unwrap_or_default(),
            clip_w: 1.0,
            uv,
            color,
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        });
    }
    input.vertices = vertices.into();
    let mut indices = input.indices.to_vec();
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    input.indices = indices.into();
    let mut batches = input.batches.to_vec();
    let scissor = if depth.is_some() {
        UiScissor::new(16, 14, 30, 30)
    } else {
        UiScissor::new(0, 0, SIDE, SIDE)
    };
    batches.push(
        UiRenderBatch::new(page, scissor, first, 6, UI_BLEND_ALPHA)
            .with_depth_test(depth.is_some())
            .with_depth_write(depth.is_some())
            .with_isolated_depth_scope(depth.map(|_| 1)),
    );
    input.batches = batches.into();
}

/// Combines transparent texture pixels, stable overlays and overlapping clipped model quads.
fn scene() -> UiRenderInput {
    let mut input = UiRenderInput {
        revision: 1,
        viewport_size: [SIDE; 2],
        safe_area: [0; 4],
        vertices: Arc::from([]),
        indices: Arc::from([]),
        batches: Arc::from([]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![
                    UiTexturePage::owned([2, 2], Arc::from([255; 16])).unwrap(),
                    UiTexturePage::owned(
                        [2, 2],
                        Arc::from([
                            250, 20, 80, 255, 30, 200, 120, 128, 70, 90, 255, 0, 180, 140, 10, 210,
                        ]),
                    )
                    .unwrap(),
                ],
                2,
            )
            .unwrap(),
        ),
    };
    quad(
        &mut input,
        [0.0, 0.0, SIDE as f32, SIDE as f32],
        [35, 55, 75, 255],
        0,
        None,
    );
    quad(
        &mut input,
        [8.5, 12.25, 50.5, 49.5],
        [190, 60, 150, 120],
        1,
        None,
    );
    quad(
        &mut input,
        [18.0, 16.0, 44.0, 44.0],
        [100, 170, 200, 255],
        0,
        Some(0.4),
    );
    quad(
        &mut input,
        [20.25, 18.75, 38.25, 38.75],
        [180, 210, 140, 205],
        1,
        Some(0.7),
    );
    quad(
        &mut input,
        [12.0, 25.0, 52.0, 31.0],
        [180, 90, 20, 105],
        1,
        None,
    );
    for vertex in &mut Arc::make_mut(&mut input.vertices)[16..20] {
        vertex.style_flags = u32::from(assets::FONT_STYLE_SDF | assets::FONT_STYLE_COVERAGE_GAMMA);
    }
    input
}

#[test]
fn retained_damage_replay_matches_full_ui_raster_byte_for_byte() {
    let initial = scene();
    let Some(raster) = Raster::new(&initial) else {
        return;
    };
    for change in 0..7 {
        let mut previous = initial.clone();
        if change == 4 {
            for batch in &mut Arc::make_mut(&mut previous.batches)[..2] {
                batch.scissor = UiScissor::new(0, 0, SIDE, 8);
            }
        }
        if change == 5 {
            Arc::make_mut(&mut previous.batches)[3].depth_write = 0;
        }
        if change == 6 {
            Arc::make_mut(&mut previous.batches)[2].depth_test = 0;
        }
        let mut current = previous.clone();
        current.revision += 1;
        for vertex in &mut Arc::make_mut(&mut current.vertices)[12..16] {
            match change {
                0 | 4 => {
                    vertex.position[0] += 12.625;
                    vertex.position[1] -= 8.375;
                }
                1 | 5 | 6 => vertex.clip_z = 0.2,
                2 => {
                    vertex.color[3] = 90;
                    vertex.uv[0] += 0.5;
                }
                3 => vertex.position[0] -= 25.25,
                _ => unreachable!(),
            }
        }
        let UiDamage::Rect(damage) = plan(&previous, &current) else {
            panic!("expected bounded damage");
        };
        assert!(damage.width < SIDE && damage.height < SIDE);
        let full = target(&raster.gpu, false);
        let retained = target(&raster.gpu, false);
        raster.draw(&previous, &retained, None);
        let old_pixels = raster.pixels(&retained);
        raster.draw(&current, &full, None);
        raster.draw(&current, &retained, Some(damage));
        let expected = raster.pixels(&full);
        let actual = raster.pixels(&retained);
        assert_ne!(
            expected, old_pixels,
            "fixture change {change} must change pixels"
        );
        assert_eq!(
            actual, expected,
            "partial replay must match full redraw for change {change}"
        );
    }
}

#[test]
fn analytic_radial_gradient_interpolates_premultiplied_stops_and_extent() {
    let mut input = scene();
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    quad(
        &mut input,
        [0.0, 0.0, SIDE as f32, SIDE as f32],
        [0, 0, 0, 102],
        0,
        None,
    );
    for vertex in Arc::make_mut(&mut input.vertices) {
        vertex.uv = vertex.position.map(|value| value * 2.0 / SIDE as f32 - 1.0);
        vertex.style_flags = render_model::UI_STYLE_RADIAL_GRADIENT;
        vertex.overlay_color = [45.0 / 255.0, 4.0 / 255.0, 4.0 / 255.0, 0.8];
    }
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    let output = target(&raster.gpu, false);
    for extent in [1.0, 3.0] {
        let mut frame = input.clone();
        for vertex in Arc::make_mut(&mut frame.vertices) {
            vertex.uv = vertex.uv.map(|value| value / extent);
        }
        raster.draw(&frame, &output, None);
        let pixels = raster.pixels(&output);
        for [x, y] in [[32, 32], [0, 32], [0, 0], [16, 32]] {
            let uv = [x, y].map(|value| ((value as f32 + 0.5) * 2.0 / SIDE as f32 - 1.0) / extent);
            let radius = uv[0].hypot(uv[1]).min(1.0);
            let expected = [
                36.0 * radius,
                3.2 * radius,
                3.2 * radius,
                102.0 + 102.0 * radius,
            ];
            let offset = ((y * SIDE + x) * 4) as usize;
            for (actual, expected) in pixels[offset..offset + 4].iter().zip(expected) {
                assert!(
                    (f32::from(*actual) - expected).abs() <= 1.0,
                    "pixel ({x},{y}), extent {extent}: {:?}",
                    &pixels[offset..offset + 4]
                );
            }
        }
    }
}

#[test]
fn oversized_layout_on_small_targets_emits_valid_scissors() {
    let mut input = scene();
    input.viewport_size = [352, 184];
    for batch in Arc::make_mut(&mut input.batches) {
        batch.scissor = UiScissor::new(0, 0, 352, 184);
        batch.depth_test = 0;
        batch.depth_write = 0;
        batch.isolated_depth_scope = None;
    }
    let Some(raster) = Raster::new(&input) else {
        return;
    };
    for [width, height] in [[352, 184], [254, 124], [1, 1]] {
        let output = raster.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("small UI surface"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::super::composite::UI_LAYER_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        raster
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        raster.draw(&input, &output, None);
        raster
            .gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let error = bevy::tasks::block_on(raster.gpu.device.pop_error_scope());
        assert!(error.is_none(), "{width}x{height}: {error:?}");
        if width > 1 {
            assert!(
                raster
                    .pixels(&output)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] != 0),
                "valid small-target drawing must produce pixels"
            );
        }
        raster.snapshot(&output);
    }
}
