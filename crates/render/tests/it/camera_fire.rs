use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use wgpu::util::DeviceExt;

/// Perspective ray scales the fixtures render with: tan(half FOV) times aspect, and tan(half FOV).
const PROJECTION: [f32; 2] = [1.25, 0.7];

/// Uploads two `side`-square RGBA frames as the fire flipbook and a point sampler.
fn fire_frames(gpu: &Gpu, side: u32, pixels: &[u8]) -> (wgpu::TextureView, wgpu::Sampler) {
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("original fire test pixels"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        pixels,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    (view, sampler)
}

/// Renders the production overlay shader with only the fire layer at native opacity.
fn render_fire(
    gpu: &Gpu,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    frame: f32,
) -> Vec<u8> {
    let mut words = vec![1.0, 0.0, 0.0, 0.0];
    words.extend([frame, frame, 0.0, 1.0]);
    words.extend([PROJECTION[0], PROJECTION[1], 0.0, 0.0]);
    words.extend([0.0; 4]);
    words.extend(bevy::math::Mat4::IDENTITY.to_cols_array());
    let layer_start = words.len();
    words.extend([
        1.0,
        1.0,
        1.0,
        0.9,
        render_scene::ScreenOverlayKind::Fire as u32 as f32,
        0.0,
        0.0,
        0.0,
    ]);
    words.resize(
        layer_start + render_scene::MAX_SCREEN_OVERLAY_LAYERS * 8,
        0.0,
    );
    let uniform = gpu.buffer(&words, wgpu::BufferUsages::UNIFORM);
    let mut bindings = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: uniform.as_entire_binding(),
    }];
    for binding in [1, 3, 5] {
        bindings.push(wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        });
        bindings.push(wgpu::BindGroupEntry {
            binding: binding + 1,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
    }
    gpu.render(
        include_str!("../../src/screen_overlay.wgsl"),
        "overlay_vertex",
        &[Draw {
            fragment: "overlay_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }],
    )
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let start = ((y * SNAPSHOT_SIDE + x) * 4) as usize;
    <[u8; 4]>::try_from(&pixels[start..start + 4]).unwrap()
}

/// Original synthetic pixels isolate native cube orientation and point sampling.
#[test]
fn camera_fire_native_cube_is_open_above_and_keeps_pixel_edges_below() {
    let Some(gpu) =
        Gpu::for_fixture("camera_fire_native_cube_is_open_above_and_keeps_pixel_edges_below")
    else {
        return;
    };
    let (view, sampler) = fire_frames(
        &gpu,
        4,
        &(0..2)
            .flat_map(|frame| {
                (0..4).flat_map(move |y| {
                    (0..4).flat_map(move |_| {
                        if y < 2 {
                            [0; 4]
                        } else if frame == 0 {
                            [200, 70, 10, 255]
                        } else {
                            [10, 70, 200, 255]
                        }
                    })
                })
            })
            .collect::<Vec<_>>(),
    );
    for (frame, rgb) in [(0.0, [200, 70, 10]), (1.0, [10, 70, 200])] {
        let pixels = render_fire(&gpu, &view, &sampler, frame);
        assert_eq!(pixel(&pixels, SNAPSHOT_SIDE / 2, 0), [0; 4]);
        assert_eq!(pixel(&pixels, 0, 0), [0; 4]);
        let bottom = pixel(&pixels, SNAPSHOT_SIDE / 2, SNAPSHOT_SIDE - 1);
        assert_eq!(bottom[..3], rgb);
        assert!((i16::from(bottom[3]) - 230).abs() <= 1);
        let covered = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .count();
        assert!(covered > 0 && covered < pixels.len() / 4);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 0 || pixel[..3] == rgb)
        );
    }
}

/// The colour of one texel in a 4x4 sprite where every texel differs.
fn texel_colour(column: usize, row: usize) -> [u8; 3] {
    [
        30 + column as u8 * 60,
        30 + row as u8 * 60,
        220 - (column + row) as u8 * 20,
    ]
}

/// Independent model-space description of the native fire cube: faces at +-1 without a
/// top, the model turned half a turn about Y to face the viewer, scaled by 0.7 and its
/// centre 0.2 below the eye. Returns the texel a screen pixel reads, or `None` where the
/// ray leaves through the open top or lands too near a texel or face edge to be exact.
fn native_fire_texel(x: u32, y: u32) -> Option<Option<(usize, usize)>> {
    let screen = |value: u32| (value as f32 + 0.5) / SNAPSHOT_SIDE as f32 * 2.0 - 1.0;
    let view = [
        screen(x) * PROJECTION[0],
        -screen(y) * PROJECTION[1],
        -1.0_f32,
    ];
    // The half turn maps view x and z to model -x and -z.
    let direction = [-view[0], view[1], -view[2]];
    let origin = [0.0, 0.2 / 0.7, 0.0];
    let exits = [0, 1, 2].map(|axis| (direction[axis].signum() - origin[axis]) / direction[axis]);
    let t = exits.iter().copied().fold(f32::INFINITY, f32::min);
    let axis = exits.iter().position(|&exit| exit == t).unwrap();
    let mut sorted = exits;
    sorted.sort_by(f32::total_cmp);
    if sorted[1] - sorted[0] < 0.02 {
        return None;
    }
    let p = [0, 1, 2].map(|i| origin[i] + direction[i] * t);
    let (u, v) = match axis {
        1 if direction[1] > 0.0 => return Some(None),
        1 => ((p[0] + 1.0) / 2.0, (1.0 - p[2]) / 2.0),
        0 => ((p[2] + 1.0) / 2.0, (1.0 - p[1]) / 2.0),
        _ => ((p[0] + 1.0) / 2.0, (1.0 - p[1]) / 2.0),
    };
    let near_edge = |value: f32| ((value * 4.0) - (value * 4.0).round()).abs() < 0.05;
    if near_edge(u) || near_edge(v) {
        return None;
    }
    Some(Some((
        ((u * 4.0) as usize).min(3),
        ((v * 4.0) as usize).min(3),
    )))
}

#[test]
fn camera_fire_cube_faces_the_view_below_the_eye_with_mirrored_sprites() {
    let Some(gpu) =
        Gpu::for_fixture("camera_fire_cube_faces_the_view_below_the_eye_with_mirrored_sprites")
    else {
        return;
    };
    let frame = (0..4)
        .flat_map(|row| {
            (0..4).flat_map(move |column| texel_colour(column, row).into_iter().chain([255]))
        })
        .collect::<Vec<_>>();
    let (view, sampler) = fire_frames(&gpu, 4, &frame.repeat(2));
    let pixels = render_fire(&gpu, &view, &sampler, 0.0);
    let mut checked = 0;
    for y in (0..SNAPSHOT_SIDE).step_by(3) {
        for x in (0..SNAPSHOT_SIDE).step_by(3) {
            let Some(expected) = native_fire_texel(x, y) else {
                continue;
            };
            let actual = pixel(&pixels, x, y);
            match expected {
                None => assert_eq!(actual[3], 0, "open top at ({x}, {y})"),
                Some((column, row)) => {
                    assert_eq!(
                        actual[..3],
                        texel_colour(column, row),
                        "texel ({column}, {row}) at ({x}, {y})"
                    );
                    assert!((i16::from(actual[3]) - 230).abs() <= 1);
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 4000, "{checked} pixels compared");
}
