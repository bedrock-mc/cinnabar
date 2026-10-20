//! Original fixture packs through planning and, with a GPU, through a whole frame.

use super::loader::{build, runnable_programs, translate_config};
use super::passes::{FrameIo, ViewState, record_frame};
use super::pipelines::GpuContext;
use super::pipelines::class_sampler;
use super::plan::plan_frame;
use super::plugin::{BlockIdRevisions, JAVA_DIMENSIONS};
use super::uniforms::{FrameClock, FrameUniforms, ViewCamera};
use bevy::math::{Quat, Vec3};
use render_scene::shaderpack::ShaderPackFrameInputs;
use shaderpack::{PackSource, ProgramGroup, SamplerClass, ShaderPack};
use std::sync::Arc;

const PASS_VERTEX: &str = "#version 120
varying vec2 texcoord;
void main() {
    gl_Position = ftransform();
    texcoord = gl_MultiTexCoord0.xy;
}
";

/// Inverts `colortex0` through texture coordinates.
const INVERT: &str = "#version 120
uniform sampler2D colortex0;
varying vec2 texcoord;
void main() {
    gl_FragData[0] = vec4(1.0 - texture2D(colortex0, texcoord).rgb, 1.0);
}
";

/// Copies `colortex0` to the screen through window coordinates.
const FINAL: &str = "#version 120
uniform sampler2D colortex0;
uniform float viewWidth;
uniform float viewHeight;
void main() {
    gl_FragColor = texture2D(colortex0, gl_FragCoord.xy / vec2(viewWidth, viewHeight));
}
";

/// Writes `colortex3` and leaves `colortex0` alone.
const FILL: &str = "#version 120
void main() {
/* DRAWBUFFERS:3 */
    gl_FragData[0] = vec4(0.25);
}
";

fn pack(files: &[(&str, &str)]) -> ShaderPack {
    ShaderPack::new(PackSource::from_files(files.iter().copied()))
}

#[test]
fn enabled_full_screen_programs_run_in_frame_order() {
    let pack = pack(&[
        (
            "shaders.properties",
            "program.composite1.enabled = WANT_BLUR\nflip.composite2.colortex3 = false\n",
        ),
        ("composite.vsh", PASS_VERTEX),
        ("composite.fsh", INVERT),
        ("composite1.vsh", PASS_VERTEX),
        ("composite1.fsh", INVERT),
        ("composite2.vsh", PASS_VERTEX),
        ("composite2.fsh", FILL),
        ("deferred.vsh", PASS_VERTEX),
        ("deferred.fsh", INVERT),
        ("final.vsh", PASS_VERTEX),
        ("final.fsh", FINAL),
        ("gbuffers_terrain.vsh", PASS_VERTEX),
        ("gbuffers_terrain.fsh", FILL),
    ]);
    let translation = pack.translate(&translate_config(&pack, JAVA_DIMENSIONS[0]));
    let (runnable, plans) = runnable_programs(&translation).expect("fixture programs translate");
    let names: Vec<&str> = runnable.iter().map(|p| p.program.name.as_str()).collect();
    assert_eq!(names, ["deferred", "composite", "composite2", "final"]);
    let frame = plan_frame(&plans, &translation.properties.flips);
    let writes: Vec<_> = frame.post.iter().map(|s| s.writes.clone()).collect();
    assert_eq!(
        writes,
        [
            vec![Some((0, true))],
            vec![Some((0, false))],
            vec![Some((3, true))],
            vec![None],
        ]
    );
    assert!(
        frame.swap_back.is_empty(),
        "composite2's flip is suppressed"
    );
    assert_eq!(plans[3].group, ProgramGroup::Final);
}

#[test]
fn a_failed_required_program_fails_the_load() {
    let pack = pack(&[
        ("composite.vsh", PASS_VERTEX),
        (
            "composite.fsh",
            "#version 120\nvoid main() { gl_FragData[0] = undefined_name; }\n",
        ),
    ]);
    let translation = pack.translate(&translate_config(&pack, JAVA_DIMENSIONS[0]));
    let error = runnable_programs(&translation).expect_err("composite fails");
    let lines: Vec<&str> = error.lines().collect();
    assert_eq!(lines.len(), 1, "one line per failed program: {error}");
    assert!(
        lines[0].starts_with("composite: composite.fsh:") && lines[0].contains("undefined_name"),
        "names the program and its first diagnostic: {error}"
    );
}

/// Switching packs shows Vanilla, which removes the block ids, between them; the geometry
/// adapter still remembers the revision its terrain attributes came from, so the next pack's
/// ids must arrive under another one or its terrain keeps the previous pack's `mc_Entity`.
#[test]
fn the_next_packs_block_ids_never_reuse_a_revision() {
    let mut revisions = BlockIdRevisions::default();
    let photon = revisions.publish(Arc::new(vec![-1, 8]));
    let bsl = revisions.publish(Arc::new(vec![-1, 10001]));
    assert_ne!(photon.revision, bsl.revision);
    assert_eq!((photon.block_entity_id)(1), 8);
    assert_eq!(
        [(bsl.block_entity_id)(1), (bsl.block_entity_id)(2)],
        [10001, -1]
    );
}

pub(super) fn gpu() -> Option<GpuContext> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).ok()?;
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("shader pack frame fixture"),
        required_features: adapter.features(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some(GpuContext {
        device,
        queue,
        adapter,
    })
}

/// Side of the fixture frame.
const SIZE: [u32; 2] = [4, 4];

/// Runs `files` over a scene whose top half is red and bottom half mid grey; returns the
/// output rows top-down, 256 bytes apart.
fn render(gpu: &GpuContext, files: &[(&str, &str)]) -> Vec<u8> {
    render_frames(gpu, files, 1).1
}

/// [`render`] over `frames` frames of one view; also returns the loaded pack.
fn render_frames(
    gpu: &GpuContext,
    files: &[(&str, &str)],
    frames: usize,
) -> (Arc<super::loader::PackGpu>, Vec<u8>) {
    render_frame_options(gpu, files, frames, [0, 0, SIZE[0], SIZE[1]], false, false)
}

/// Renders a viewport and optionally records a second camera before submitting the first.
fn render_frame_options(
    gpu: &GpuContext,
    files: &[(&str, &str)],
    frames: usize,
    viewport: [u32; 4],
    second_view: bool,
    geometry_fixture: bool,
) -> (Arc<super::loader::PackGpu>, Vec<u8>) {
    let pack = pack(files);
    let pack = Arc::new(
        build(
            &pack,
            "fixture",
            JAVA_DIMENSIONS[0],
            &Default::default(),
            None,
            gpu,
            &Default::default(),
            &Default::default(),
        )
        .expect("fixture loads"),
    );
    let size = SIZE;
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let texture = |format, usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let format = render_scene::SCENE_COLOR_FORMAT;
    let scene = texture(
        format,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    // Top half red, bottom half mid grey, rows top-down as wgpu stores them.
    let pixels: Vec<u8> = (0..size[1])
        .flat_map(|row| {
            let colour = if row < 2 {
                [255, 0, 0, 255]
            } else {
                [128, 128, 128, 255]
            };
            std::iter::repeat_n(colour, size[0] as usize).flatten()
        })
        .collect();
    gpu.queue.write_texture(
        scene.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * size[0]),
            rows_per_image: None,
        },
        extent,
    );
    let depth = texture(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let output = texture(
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut state =
        ViewState::new(&gpu.device, Arc::clone(&pack), [viewport[2], viewport[3]]).unwrap();
    let game = ShaderPackFrameInputs::default();
    let frame = FrameUniforms::new(
        ViewCamera {
            position: Vec3::new(0.0, 64.0, 0.0),
            rotation: Quat::IDENTITY,
            focal: [1.0, 1.0],
            screen: [viewport[2], viewport[3]],
        },
        &game,
        &pack.settings,
        FrameClock {
            delta_seconds: 0.016,
        },
        &mut state.history,
    );
    let context = super::passes::DrawContext {
        chunk_offset: Vec3::ZERO,
        atlas_size: [0; 2],
        particle_atlas_size: [0; 2],
        hand_projection: bevy::math::Mat4::IDENTITY,
        crack_atlas_size: [0; 2],
    };
    state.write_uniforms(&gpu.queue, &frame, 0.016, &context);
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth.create_view(&Default::default()),
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(if second_view || geometry_fixture {
                    0.5
                } else {
                    0.0
                }),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    drop(clear);
    let render_device = bevy::render::renderer::RenderDevice::from(gpu.device.clone());
    let terrain = EmptyTerrain(pack.terrain_layout);
    let io = FrameIo {
        viewport,
        scene_colour: &scene.create_view(&Default::default()),
        scene_depth: &depth.create_view(&Default::default()),
        scene_near: 0.1,
        inverse_focal: [1.0, 1.0],
        render_distance: game.render_distance,
        fog_color: super::uniforms::sky_fog(&game, Quat::IDENTITY, Vec3::new(0.0, 64.0, 0.0)).fog,
        destination: &output.create_view(&Default::default()),
        destination_format: format,
        geometry: geometry_fixture.then_some(super::frame_geometry::FrameGeometry {
            device: &render_device,
            terrain: &terrain,
            atlas: None,
            effect_textures: None,
            actors: super::actors::ActorSources::default(),
            effects: super::effects::EffectInputs {
                camera: super::effects::EffectCamera {
                    position: Vec3::new(0., 64., 0.),
                    model_view: bevy::math::Mat4::IDENTITY,
                    focal: [1.; 2],
                    screen: [viewport[2], viewport[3]],
                },
                frame: game,
                atmosphere: None,
                atmosphere_view: None,
                atmosphere_textures: None,
                clouds_visible: false,
                particles: None,
                precipitation: None,
                lightning: None,
                selection: None,
                block_entities: None,
                seconds: 0.,
            },
            sky: super::sky::SkyInputs {
                sky_color: [0.; 3],
                fog_color: [0.; 3],
                render_distance: game.render_distance,
                celestial_angle: 0.,
                camera_y: 64.,
                sunrise: false,
            },
            overlay_radius: f32::MAX,
        }),
        timestamps: None,
    };
    for _ in 0..frames {
        record_frame(&gpu.device, &gpu.queue, &mut encoder, &mut state, &io);
    }
    if second_view {
        let mut other =
            ViewState::new(&gpu.device, Arc::clone(&pack), [viewport[2], viewport[3]]).unwrap();
        other.write_uniforms(&gpu.queue, &frame, 0.016, &context);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let other_io = FrameIo {
            scene_near: 10.0,
            geometry: None,
            ..io
        };
        record_frame(&gpu.device, &gpu.queue, &mut encoder, &mut other, &other_io);
        gpu.queue.submit([encoder.finish()]);
    }
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        extent,
    );
    gpu.queue.submit([encoder.finish()]);
    if let Some(error) = bevy::tasks::block_on(scope.pop()) {
        panic!("validation error: {error}");
    }
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap().to_vec();
    (pack, bytes)
}

fn assert_pixels(bytes: &[u8], expected: impl Fn(usize) -> [u8; 3]) {
    for row in 0..SIZE[1] as usize {
        for column in 0..SIZE[0] as usize {
            let at = row * 256 + column * 4;
            let pixel = &bytes[at..at + 4];
            let expected = expected(row);
            for c in 0..3 {
                assert!(
                    pixel[c].abs_diff(expected[c]) <= 1,
                    "row {row} column {column}: {pixel:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn composite_and_final_transform_the_scene_and_present_it_upright() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |row| {
        if row < 2 {
            [0, 255, 255]
        } else {
            [127, 127, 127]
        }
    });
}

/// Adds a quarter to `colortex5`, which keeps its contents across frames.
const ACCUMULATE: &str = "#version 120
/* DRAWBUFFERS:5 */
const bool colortex5Clear = false;
uniform sampler2D colortex5;
varying vec2 texcoord;
void main() {
    gl_FragData[0] = texture2D(colortex5, texcoord) + vec4(0.25);
}
";

/// Presents `colortex5`.
const SHOW_5: &str = "#version 120
uniform sampler2D colortex5;
varying vec2 texcoord;
void main() {
    gl_FragColor = texture2D(colortex5, texcoord);
}
";

#[test]
fn a_buffer_kept_across_frames_exchanges_its_sides_instead_of_copying_back() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (pack, bytes) = render_frames(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", ACCUMULATE),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", SHOW_5),
        ],
        3,
    );
    assert_eq!(pack.frame.exchange, [5]);
    assert!(pack.frame.swap_back.is_empty());
    assert_pixels(&bytes, |_| [191, 191, 191]);
}

/// Reads `colortex0` exactly on its top edge, where a repeating sampler would blend in the
/// bottom row.
const TOP_EDGE: &str = "#version 120
uniform sampler2D colortex0;
void main() {
    gl_FragData[0] = texture2D(colortex0, vec2(0.5, 1.0));
}
";

#[test]
fn render_targets_clamp_at_their_edges() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", TOP_EDGE),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |_| [255, 0, 0]);
}

/// Multiplies a 32-bit float `colortex0` by `depthtex0` (1.0 at the cleared far plane).
const FLOAT_AND_DEPTH: &str = "#version 120
/*
const int colortex0Format = RGBA32F;
*/
uniform sampler2D colortex0;
uniform sampler2D depthtex0;
varying vec2 texcoord;
void main() {
    gl_FragData[0] = vec4(texture2D(colortex0, texcoord).rgb * texture2D(depthtex0, texcoord).r, 1.0);
}
";

#[test]
fn float32_targets_and_depth_bind_with_their_samplers() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", FLOAT_AND_DEPTH),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |row| {
        if row < 2 {
            [255, 0, 0]
        } else {
            [128, 128, 128]
        }
    });
}

/// Reads `colortex0` through a 3D sampler, where the pack's 3D custom texture binds.
const VOLUME: &str = "#version 120
uniform sampler3D colortex0;
void main() {
/* DRAWBUFFERS:3 */
    gl_FragData[0] = texture3D(colortex0, vec3(0.5));
}
";

#[test]
fn a_3d_custom_texture_leaves_2d_reads_of_its_name_on_the_buffer() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    // As Photon does: `colortex0` names both the scene buffer (sampler2D) and a 3D noise
    // volume (sampler3D) in composite programs.
    let bytes = render(
        &gpu,
        &[
            (
                "shaders.properties",
                "texture.composite.colortex0 = volume.dat TEXTURE_3D R8 2 2 2 RED UNSIGNED_BYTE\n",
            ),
            ("volume.dat", "ABCDEFGH"),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", VOLUME),
            ("composite1.vsh", PASS_VERTEX),
            ("composite1.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |row| {
        if row < 2 {
            [0, 255, 255]
        } else {
            [127, 127, 127]
        }
    });
}

/// Inverts `colortex0` in place: samples it and writes `colorimg0`, the same texture.
const INVERT_IN_PLACE: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
uniform sampler2D colortex0;
layout(rgba8) uniform image2D colorimg0;
void main() {
    ivec2 p = ivec2(gl_GlobalInvocationID.xy);
    imageStore(colorimg0, p, vec4(1.0 - texelFetch(colortex0, p, 0).rgb, 1.0));
}
";

#[test]
fn a_buffer_sampled_and_written_as_an_image_reads_a_snapshot() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            ("composite.csh", INVERT_IN_PLACE),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |row| {
        if row < 2 {
            [0, 255, 255]
        } else {
            [127, 127, 127]
        }
    });
}

/// Solas's floodfill: each frame samples one custom image and writes the other, binding
/// both images and both samplers either way. Here frame 0 inverts `pongSampler` (cleared
/// to zero) into `ping`.
const PING_PONG: &str = "#version 430
layout(local_size_x = 1, local_size_y = 1) in;
const ivec3 workGroups = ivec3(1, 1, 1);
uniform int frameCounter;
writeonly uniform image2D ping, pong;
uniform sampler2D pingSampler, pongSampler;
void main() {
    if ((frameCounter & 1) == 0) {
        imageStore(ping, ivec2(0), vec4(1.0 - texelFetch(pongSampler, ivec2(0), 0).rgb, 1.0));
    } else {
        imageStore(pong, ivec2(0), vec4(1.0 - texelFetch(pingSampler, ivec2(0), 0).rgb, 1.0));
    }
}
";

/// Writes `ping`'s texel to every pixel.
const SHOW_PING: &str = "#version 430
uniform sampler2D pingSampler;
void main() {
/* DRAWBUFFERS:0 */
    gl_FragData[0] = vec4(texelFetch(pingSampler, ivec2(0), 0).rgb, 1.0);
}
";

#[test]
fn images_sampled_and_written_in_one_dispatch_read_snapshots() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.ping = pingSampler RGBA RGBA8 UNSIGNED_BYTE true false 1 1\n\
                 image.pong = pongSampler RGBA RGBA8 UNSIGNED_BYTE true false 1 1\n",
            ),
            ("composite.csh", PING_PONG),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_PING),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |_| [255, 255, 255]);
}

/// Counts its 16 invocations into an `r32ui` custom image with an image atomic.
const COUNT_ATOMICALLY: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(0), 1u);
}
";

/// Writes the counter's first texel, over 255, to red.
const SHOW_COUNT: &str = "#version 430
uniform usampler2D counterSampler;
void main() {
/* DRAWBUFFERS:0 */
    uint count = texelFetch(counterSampler, ivec2(0), 0).r;
    gl_FragData[0] = vec4(float(count) / 255.0, 0.0, 0.0, 1.0);
}
";

#[test]
fn image_atomics_count_into_a_custom_image() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        eprintln!("missing fixture: a GPU with TEXTURE_ATOMIC for image atomics");
        return;
    }
    let bytes = render(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n",
            ),
            ("composite.csh", COUNT_ATOMICALLY),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_COUNT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |_| [16, 0, 0]);
}

/// A pack's switched-off pass, as Noble's `deferred` is without a LOD mod: no output, and a
/// target nothing else writes.
const SWITCHED_OFF: &str = "#version 430
/* RENDERTARGETS: 10 */
void main() {}
";

/// Writes no colour buffer, only magenta into a custom image.
const STORE_ONLY: &str = "#version 430
writeonly uniform image2D marker;
void main() {
    imageStore(marker, ivec2(0), vec4(1.0, 0.0, 1.0, 1.0));
}
";

/// Writes the marker's texel to every pixel.
const SHOW_MARKER: &str = "#version 430
uniform sampler2D markerSampler;
void main() {
/* DRAWBUFFERS:0 */
    gl_FragData[0] = vec4(texelFetch(markerSampler, ivec2(0), 0).rgb, 1.0);
}
";

#[test]
fn programs_without_colour_outputs_load_and_keep_their_stores() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.marker = markerSampler RGBA RGBA8 UNSIGNED_BYTE true false 1 1\n",
            ),
            ("deferred.vsh", PASS_VERTEX),
            ("deferred.fsh", SWITCHED_OFF),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", STORE_ONLY),
            ("composite1.vsh", PASS_VERTEX),
            ("composite1.fsh", SHOW_MARKER),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |_| [255, 0, 255]);
}

#[test]
fn a_rejected_required_actor_pipeline_keeps_native_world_rendering() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    // Integer fragment output cannot bind the default normalized color target.
    let pack = pack(&[
        ("gbuffers_basic.vsh", PASS_VERTEX),
        ("gbuffers_basic.fsh", FILL),
        ("gbuffers_terrain.vsh", PASS_VERTEX),
        ("gbuffers_terrain.fsh", FILL),
        ("gbuffers_entities.vsh", PASS_VERTEX),
        (
            "gbuffers_entities.fsh",
            "#version 430\nlayout(location=0) out uvec4 colour; void main() { colour = uvec4(1); }",
        ),
    ]);
    let pack = build(
        &pack,
        "fixture",
        JAVA_DIMENSIONS[0],
        &Default::default(),
        None,
        &gpu,
        &Default::default(),
        &Default::default(),
    )
    .expect("pipeline rejection retains the loaded pack and native scene");
    let entities = pack
        .programs
        .iter()
        .position(|p| p.name == "gbuffers_entities")
        .expect("gbuffers_entities translated");
    assert!(
        pack.draws.is_empty(),
        "a rejected required draw retains the native world path"
    );
    let kept: Vec<_> = pack
        .draws
        .iter()
        .filter(|d| d.program == entities)
        .map(|d| d.kind)
        .collect();
    assert!(kept.is_empty(), "rejected entities draws kept: {kept:?}");
}

/// BSL's hash argument `dot(p, vec2(12.9898, 4.1414))` at large `p`, scaled so the red
/// channel resolves the last bits of the dot product.
const HASH_DOT: &str = "#version 120
void main() {
    float d = dot(floor(gl_FragCoord.xy) + 3000.0, vec2(12.9898, 4.1414));
    gl_FragColor = vec4(fract(d * 64.0), 0.0, 0.0, 1.0);
}
";

/// `dot` rounds like the reference OpenGL driver, `fma(a.y, b.y, a.x * b.x)`, so hash noise
/// built on it matches Java Iris instead of the device's own `dot` rounding.
#[test]
fn dot_rounds_like_the_reference_opengl_driver() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let bytes = render(&gpu, &[("final.vsh", PASS_VERTEX), ("final.fsh", HASH_DOT)]);
    let red = |d: f32| ((f64::from(d) * 64.0).fract() * 255.0).round() as f32;
    let mut discriminating = 0;
    for row in 0..SIZE[1] as usize {
        for column in 0..SIZE[0] as usize {
            // Rows come back top-down; gl_FragCoord.y counts from the bottom.
            let x = column as f32 + 3000.0;
            let y = (SIZE[1] as usize - 1 - row) as f32 + 3000.0;
            let reference = y.mul_add(4.1414, x * 12.9898);
            let other = x.mul_add(12.9898, y * 4.1414);
            if (red(reference) - red(other)).abs() > 2.0 {
                discriminating += 1;
            }
            let got = f32::from(bytes[row * 256 + column * 4]);
            assert!(
                (got - red(reference)).abs() <= 1.0,
                "row {row} column {column}: red {got}, reference {}, other order {} ({:?})",
                red(reference),
                red(other),
                gpu.adapter.get_info().backend
            );
        }
    }
    assert!(discriminating > 0, "no pixel tells the two roundings apart");
}

#[test]
fn sampler_classes_map_to_wrap_and_filter_modes() {
    for (index, class) in SamplerClass::ALL.into_iter().enumerate() {
        assert_eq!(
            class as usize, index,
            "PackGpu::filtering is indexed by class"
        );
        let d = class_sampler(class);
        let wrap = if class.clamps() {
            wgpu::AddressMode::ClampToEdge
        } else {
            wgpu::AddressMode::Repeat
        };
        assert_eq!(
            [d.address_mode_u, d.address_mode_v, d.address_mode_w],
            [wrap; 3]
        );
        let filter = if class.linear() {
            wgpu::FilterMode::Linear
        } else {
            wgpu::FilterMode::Nearest
        };
        assert_eq!([d.mag_filter, d.min_filter], [filter; 2]);
    }
}

#[test]
fn fallback_presentation_uses_the_exchanged_composite_output() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (pack, bytes) = render_frames(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            (
                "composite.fsh",
                "#version 120\nvoid main() { gl_FragData[0] = vec4(0.25, 0.5, 0.75, 1.0); }",
            ),
        ],
        2,
    );
    assert!(pack.frame.exchange.contains(&0));
    assert_pixels(&bytes, |_| [64, 128, 191]);
}

#[test]
fn viewport_copies_crop_and_preserve_pixels_outside_the_view() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        1,
        [1, 1, 2, 2],
        false,
        false,
    );
    for y in 0..4 {
        for x in 0..4 {
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            let original = if y < 2 { [255u8, 0, 0] } else { [128u8; 3] };
            let expected = original.map(|c| if inside { 255 - c } else { c });
            let at = y * 256 + x * 4;
            for c in 0..3 {
                assert!(bytes[at + c].abs_diff(expected[c]) <= 1);
            }
        }
    }
}

#[test]
fn recorded_views_keep_their_own_depth_conversion_planes() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("final.vsh", PASS_VERTEX),
            (
                "final.fsh",
                "#version 120\nuniform sampler2D depthtex0; varying vec2 texcoord; void main() { gl_FragColor = vec4(vec3(texture2D(depthtex0, texcoord).r), 1.0); }",
            ),
        ],
        1,
        [0, 0, 4, 4],
        true,
        false,
    );
    assert_pixels(&bytes, |_| [191; 3]);
}

/// Supplies an empty terrain list while retaining the host's geometry and overlay passes.
struct EmptyTerrain(super::geometry::TerrainLayout);
impl super::frame_geometry::TerrainDraws for EmptyTerrain {
    fn draw<'w>(
        &'w self,
        _pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
        _category: super::geometry::GeometryCategory,
        _shadow: bool,
    ) -> u32 {
        0
    }
    fn layout(&self) -> super::geometry::TerrainLayout {
        self.0
    }
}

#[test]
fn native_overlay_refreshes_pre_translucent_depth() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            (
                "final.fsh",
                "#version 120\nuniform sampler2D depthtex1; varying vec2 texcoord; void main() { gl_FragColor = vec4(vec3(texture2D(depthtex1, texcoord).r), 1.0); }",
            ),
        ],
        1,
        [0, 0, 4, 4],
        false,
        true,
    );
    assert_pixels(&bytes, |_| [191; 3]);
}

#[test]
fn geometry_draws_refresh_custom_image_feedback_before_opening_the_pass() {
    use super::frame_geometry::open_kind;
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let source = pack(&[
        ("gbuffers_basic.vsh", PASS_VERTEX),
        ("gbuffers_basic.fsh", FILL),
        (
            "shaders.properties",
            "image.marker = markerSampler RGBA RGBA8 UNSIGNED_BYTE true false 1 1\n",
        ),
        ("gbuffers_terrain.vsh", PASS_VERTEX),
        (
            "gbuffers_terrain.fsh",
            "#version 430\nuniform image2D marker; uniform sampler2D markerSampler; void main() { vec4 c = texelFetch(markerSampler, ivec2(0), 0); imageStore(marker, ivec2(0), c); gl_FragData[0] = c; }",
        ),
    ]);
    let pack = Arc::new(
        build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            None,
            &gpu,
            &Default::default(),
        )
        .unwrap(),
    );
    let mut state = ViewState::new(&gpu.device, Arc::clone(&pack), SIZE).unwrap();
    let texture = &state.targets.images[0].0;
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[255, 0, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        texture.size(),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    drop(
        open_kind(
            &mut encoder,
            &mut state,
            super::draws::DrawKind::TerrainSolid,
            0,
            None,
        )
        .unwrap(),
    );
    let source = &state.targets.image_snapshots[0].as_ref().unwrap().view;
    let pipeline = &pack
        .host
        .present
        .iter()
        .find(|(f, _)| *f == render_scene::SCENE_COLOR_FORMAT)
        .unwrap()
        .1;
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        }],
    });
    // Read the snapshot through a compatible color attachment, without requiring COPY_SRC on it.
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_scene::SCENE_COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    {
        let view = output.create_view(&Default::default());
        let mut pass = super::passes::pass(
            &mut encoder,
            "snapshot probe",
            &[super::passes::load_attachment(&view)],
            None,
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        output.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert_eq!(
        &readback.slice(..).get_mapped_range().unwrap()[..4],
        &[255, 0, 0, 255]
    );
}

#[test]
fn geometry_overlay_crops_the_native_camera_viewport() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        1,
        [1, 0, 2, 2],
        false,
        true,
    );
    assert_pixels(&bytes, |row| if row < 2 { [255, 0, 0] } else { [128; 3] });
}
