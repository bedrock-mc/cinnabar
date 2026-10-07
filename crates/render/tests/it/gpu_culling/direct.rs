//! Read-back occlusion for direct draws: the kernel matches a CPU Hi-Z reference, replayed
//! camera paths never skip a sub-chunk that shows pixels, and still views skip hidden terrain.

use std::{collections::VecDeque, time::Instant};

use super::*;
use kernels::OcclusionStorage;
use model::reference_occluded;
use occlusion::{OcclusionBasis, OcclusionHistory, VerdictTag};

/// The occlusion kernel over one record table.
struct Occluder<'a> {
    gpu: &'a Gpu,
    kernels: CullKernels,
    storage: OcclusionStorage,
    slots: u32,
}

impl<'a> Occluder<'a> {
    fn new(gpu: &'a Gpu, records: &[CullRecord]) -> Self {
        let capacity = (records.len() as u32).next_power_of_two().max(256);
        let occluder = Self {
            gpu,
            kernels: CullKernels::new(&gpu.device),
            storage: OcclusionStorage::new(&gpu.device, capacity),
            slots: records.len() as u32,
        };
        occluder.set_records(records);
        occluder
    }

    fn set_records(&self, records: &[CullRecord]) {
        assert_eq!(records.len() as u32, self.slots);
        self.gpu
            .queue
            .write_buffer(&self.storage.records, 0, bytemuck::cast_slice(records));
    }

    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &CullViewInput,
        pyramid: &HizPyramid,
    ) {
        let uniform = self.uniform(input);
        self.gpu
            .queue
            .write_buffer(&self.storage.uniform, 0, bytemuck::bytes_of(&uniform));
        let group = self
            .kernels
            .occlusion_bind_group(&self.gpu.device, &self.storage, pyramid);
        self.kernels.encode_occlusion(encoder, &group, self.slots);
    }

    fn uniform(&self, input: &CullViewInput) -> CullViewUniform {
        CullViewUniform::new(input, CullPhase::Late, self.slots, self.storage.capacity)
    }

    fn bits(&self) -> Vec<u32> {
        let bytes = read_buffer(
            self.gpu,
            &self.storage.occluded,
            kernels::occlusion_bytes(self.storage.capacity),
        );
        bytemuck::cast_slice(&bytes).to_vec()
    }
}

fn bit(words: &[u32], slot: usize) -> bool {
    words
        .get(slot / 32)
        .is_some_and(|word| word >> (slot % 32) & 1 != 0)
}

fn sized_input(camera: &Camera, hiz_mips: u32, size: [u32; 2]) -> CullViewInput {
    CullViewInput {
        viewport: [0.0, 0.0, size[0] as f32, size[1] as f32],
        depth_size: size,
        ..view_input(camera, hiz_mips)
    }
}

fn pyramid_levels(gpu: &Gpu, pyramid: &HizPyramid) -> Vec<(Vec<f32>, [u32; 2])> {
    (0..pyramid.mip_count())
        .map(|level| {
            (
                floats(&read_texture(gpu, &pyramid.texture, level)),
                pyramid.size(level),
            )
        })
        .collect()
}

/// Wherever rounding cannot flip the decision, the kernel's bit equals the CPU reference,
/// and no box that shows a pixel, or reaches past the screen, is ever called occluded.
#[test]
fn occlusion_bits_match_the_cpu_hi_z_reference() {
    let Some(gpu) = Gpu::for_fixture("terrain occlusion bits") else {
        return;
    };
    let mut random = Lcg(23);
    let mut records = Vec::new();
    let mut boxes = vec![[-40.0, 60.0, -24.0, 0.0, 56.0, 84.0, -23.0, 0.0]];
    for x in -4..=4 {
        for y in 3..=5 {
            for z in -6..=-2 {
                let slot = records.len() as u32;
                let low = [random.next(10), random.next(12), random.next(10)];
                let high = low.map(|value| value as i32 + 2 + random.next(5) as i32);
                let origin = [x * 16, y * 16, z * 16];
                records.push(
                    CullRecord::new(&CullRecordSource {
                        origin,
                        base_vertex: slot as i32 * 4,
                        bounds: [low.map(|value| value as i32), high],
                        cube: slot * 8..slot * 8 + 6,
                        solid_ends: [1, 2, 3, 4, 5, 6],
                        ..Default::default()
                    })
                    .unwrap(),
                );
                let world = |local: [i32; 3]| {
                    std::array::from_fn::<f32, 3, _>(|axis| (origin[axis] + local[axis]) as f32)
                };
                let (lo, hi) = (world(low.map(|value| value as i32)), world(high));
                boxes.push([
                    lo[0],
                    lo[1],
                    lo[2],
                    (slot + 1) as f32,
                    hi[0],
                    hi[1],
                    hi[2],
                    0.0,
                ]);
            }
        }
    }
    records[17] = CullRecord::default();
    let occluder = Occluder::new(&gpu, &records);
    let cameras = [
        camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0)),
        camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(-30.0, 66.0, -40.0)),
        camera(Vec3::new(-20.5, 75.0, -4.0), Vec3::new(20.0, 68.0, -60.0)),
    ];
    let (mut decided, mut total, mut occluded, mut edge_kept) = (0, 0, 0, 0);
    for camera in &cameras {
        let target = Target::new(&gpu, wgpu::TextureFormat::R32Uint, 1);
        hiz::render_scene(
            &gpu,
            &target,
            ("box_vertex", "id_fragment"),
            &camera.clip_from_world.to_cols_array(),
            &boxes,
        );
        let ids = read_texture(&gpu, &target.color, 0);
        let shown = bytemuck::cast_slice::<u8, u32>(&ids)
            .iter()
            .filter(|&&id| id != 0)
            .map(|&id| id as usize - 1)
            .collect::<BTreeSet<_>>();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let pyramid = target.pyramid(&gpu, &occluder.kernels, &mut encoder);
        let input = view_input(camera, pyramid.mip_count());
        occluder.encode(&mut encoder, &input, &pyramid);
        gpu.queue.submit([encoder.finish()]);
        let bits = occluder.bits();
        let levels = pyramid_levels(&gpu, &pyramid);
        let uniform = occluder.uniform(&input);
        for (slot, record) in records.iter().enumerate() {
            total += 1;
            let visible_lean = reference_occluded(record, &uniform, &levels, true, 1.0);
            let occluded_lean = reference_occluded(record, &uniform, &levels, true, -1.0);
            if visible_lean == occluded_lean {
                decided += 1;
                assert_eq!(bit(&bits, slot), visible_lean, "slot {slot}");
            }
            assert!(
                !(bit(&bits, slot) && shown.contains(&slot)),
                "slot {slot} shows pixels but was called occluded"
            );
            occluded += usize::from(bit(&bits, slot));
            // Hidden on screen, but reaching past it: the same-frame late cull would drop it.
            edge_kept += usize::from(
                !bit(&bits, slot) && reference_occluded(record, &uniform, &levels, false, -1.0),
            );
        }
        assert!(
            bits.iter()
                .skip(records.len().div_ceil(32))
                .all(|&word| word == 0)
        );
    }
    assert!(decided * 100 >= total * 98, "{decided} of {total} decided");
    assert!(occluded > 0, "the wall must hide some sub-chunks");
    assert!(edge_kept > 0, "some boxes reach past the screen");
}

/// One replayed frame's camera, and a terrain edit applied at its start.
struct Step {
    camera: Camera,
    edit: Option<Terrain>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Policy {
    /// The production rules: one basis, consecutive verdicts, newer records drawn.
    Production,
    /// Trusts the newest verdict regardless of camera or geometry changes.
    Naive,
}

#[derive(Default, Debug)]
struct ReplayCounts {
    candidates: usize,
    drawn: usize,
    skipped: usize,
    candidate_draws: usize,
    drawn_draws: usize,
    /// Frames, and the sub-chunks they wrongly hid.
    wrongly_hidden: Vec<(usize, Vec<usize>)>,
    pixel_mismatches: usize,
    shown: usize,
}

struct Pending {
    deliver: usize,
    tag: VerdictTag,
    words: Vec<u32>,
}

fn basis_view(camera: &Camera, world: u64, size: [u32; 2]) -> OcclusionBasis {
    let world_from_view = (camera.clip_from_view.inverse() * camera.clip_from_world).inverse();
    OcclusionBasis {
        eye: camera.eye.to_array(),
        view_rotation: bevy::math::Mat3::from_mat4(world_from_view).to_cols_array(),
        clip_from_view: camera.clip_from_view.to_cols_array(),
        viewport: [0, 0, size[0], size[1]],
        depth_size: size,
        world,
    }
}

fn front_to_back(terrain: &Terrain, slots: &[usize], eye: Vec3) -> Vec<usize> {
    let distance =
        |slot: usize| (Vec3::from_array(terrain.chunks[slot].0.map(|v| v as f32)) - eye).length();
    let mut sorted = slots.to_vec();
    sorted.sort_by(|&a, &b| distance(a).total_cmp(&distance(b)));
    sorted
}

/// Draws one stream of `slots` in order, returning the draw count.
fn draw_slots(
    raster: &Raster,
    terrain: &Terrain,
    pass: &mut wgpu::RenderPass<'_>,
    slots: &[usize],
    eye: Vec3,
    stream: CullStream,
) -> usize {
    let eye = eye.as_dvec3().to_array();
    let draws = slots
        .iter()
        .flat_map(|&slot| slot_draws(terrain, slot, eye, stream))
        .collect::<Vec<_>>();
    raster.draw(pass, stream, &draws);
    draws.len()
}

/// Solid runs in a clearing pass, then `between`, then cutout tails in a loading pass.
fn draw_split(
    raster: &Raster,
    terrain: &Terrain,
    target: &Target,
    encoder: &mut wgpu::CommandEncoder,
    (slots, eye): (&[usize], Vec3),
    between: impl FnOnce(&mut wgpu::CommandEncoder),
) -> usize {
    let solid = draw_slots(
        raster,
        terrain,
        &mut target.pass(encoder, true),
        slots,
        eye,
        CullStream::Solid,
    );
    between(encoder);
    solid
        + draw_slots(
            raster,
            terrain,
            &mut target.pass(encoder, false),
            slots,
            eye,
            CullStream::Cutout,
        )
}

/// Both streams of `slots` in one clearing pass, as today's direct path draws them.
fn draw_single(
    raster: &Raster,
    terrain: &Terrain,
    target: &Target,
    encoder: &mut wgpu::CommandEncoder,
    (slots, eye): (&[usize], Vec3),
) -> usize {
    let mut pass = target.pass(encoder, true);
    [CullStream::Solid, CullStream::Cutout]
        .into_iter()
        .map(|stream| draw_slots(raster, terrain, &mut pass, slots, eye, stream))
        .sum()
}

/// Slots with a fragment that survives the finished depth, from per-slot occlusion queries.
fn shown_slots(
    gpu: &Gpu,
    probe: &Raster,
    terrain: &Terrain,
    target: &Target,
    slots: &[usize],
    eye: Vec3,
) -> BTreeSet<usize> {
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Occlusion,
        count: slots.len().max(1) as u32,
    });
    let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: slots.len().max(1) as u64 * 8,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let color = target.color.create_view(&Default::default());
        let depth = target.depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: Some(&queries),
        });
        let eye = eye.as_dvec3().to_array();
        for (index, &slot) in slots.iter().enumerate() {
            pass.begin_occlusion_query(index as u32);
            for stream in [CullStream::Solid, CullStream::Cutout] {
                probe.draw(&mut pass, stream, &slot_draws(terrain, slot, eye, stream));
            }
            pass.end_occlusion_query();
        }
    }
    encoder.resolve_query_set(&queries, 0..slots.len().max(1) as u32, &resolve, 0);
    gpu.queue.submit([encoder.finish()]);
    let bytes = read_buffer(gpu, &resolve, slots.len().max(1) as u64 * 8);
    let samples = bytemuck::cast_slice::<u8, u64>(&bytes);
    slots
        .iter()
        .zip(samples)
        .filter(|(_, count)| **count != 0)
        .map(|(&slot, _)| slot)
        .collect()
}

/// Replays `steps` as production would on a direct-draw device, comparing every frame with a
/// render of every frustum-visible sub-chunk. Verdicts arrive `lags` frames late, cycling.
fn replay(
    gpu: &Gpu,
    mut terrain: Terrain,
    steps: &[Step],
    policy: Policy,
    lags: &[usize],
) -> ReplayCounts {
    let size = [SNAPSHOT_SIDE; 2];
    let occluder = Occluder::new(gpu, &terrain.records);
    let mut raster = Raster::new(gpu, &terrain, &steps[0].camera);
    let mut probe = Raster::with_depth(
        gpu,
        &terrain,
        &steps[0].camera,
        false,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let mut history = OcclusionHistory::default();
    for slot in 0..terrain.records.len() as u32 {
        history.assign(slot, 0);
    }
    let mut pending = VecDeque::<Pending>::new();
    let mut newest: Option<Vec<u32>> = None;
    let mut previous_pose = None;
    let mut counts = ReplayCounts::default();
    for (frame, step) in steps.iter().enumerate() {
        let camera = &step.camera;
        if let Some(edited) = &step.edit {
            for (slot, (old, new)) in terrain.records.iter().zip(&edited.records).enumerate() {
                if old != new {
                    history.invalidate_world();
                    history.assign(slot as u32, frame as u64);
                }
            }
            terrain = Terrain {
                quads: edited.quads.clone(),
                origins: edited.origins.clone(),
                records: edited.records.clone(),
                chunks: edited.chunks.clone(),
            };
            occluder.set_records(&terrain.records);
            raster = Raster::new(gpu, &terrain, camera);
            probe = Raster::with_depth(
                gpu,
                &terrain,
                camera,
                false,
                wgpu::TextureFormat::Rgba8Unorm,
            );
        }
        raster.set_camera(gpu, camera);
        probe.set_camera(gpu, camera);
        while pending
            .front()
            .is_some_and(|verdict| verdict.deliver <= frame)
        {
            let verdict = pending.pop_front().unwrap();
            history.apply(&verdict.tag, &verdict.words);
            newest = Some(verdict.words);
        }
        let view = basis_view(camera, history.world(), size);
        let candidates = (0..terrain.records.len())
            .filter(|&slot| bevy_visible(&camera.frustum, terrain.chunks[slot].0))
            .collect::<Vec<_>>();
        let drawn = candidates
            .iter()
            .copied()
            .filter(|&slot| match policy {
                Policy::Production => !history.skips(slot as u32, &view),
                Policy::Naive => !newest.as_ref().is_some_and(|words| bit(words, slot)),
            })
            .collect::<Vec<_>>();
        let pose = (camera.clip_from_world, camera.clip_from_view);
        let verdict =
            policy == Policy::Naive || (previous_pose == Some(pose) && !history.settled(&view));
        previous_pose = Some(pose);

        // Production: solid pass, verdict from its depth, then the remaining opaque streams.
        let culled = Target::new(gpu, wgpu::TextureFormat::Rgba8Unorm, 1);
        let order = front_to_back(&terrain, &drawn, camera.eye);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let drawn_draws = draw_split(
            &raster,
            &terrain,
            &culled,
            &mut encoder,
            (&order, camera.eye),
            |encoder| {
                if verdict {
                    let pyramid = culled.pyramid(gpu, &occluder.kernels, encoder);
                    occluder.encode(
                        encoder,
                        &sized_input(camera, pyramid.mip_count(), size),
                        &pyramid,
                    );
                }
            },
        );
        gpu.queue.submit([encoder.finish()]);
        if verdict {
            let lag = lags[frame % lags.len()];
            let deliver = pending
                .back()
                .map_or(0, |last| last.deliver)
                .max(frame + lag);
            pending.push_back(Pending {
                deliver,
                tag: VerdictTag {
                    frame: frame as u64,
                    basis: view,
                    slots: occluder.slots,
                },
                words: occluder.bits(),
            });
        }

        // Ground truth: every frustum-visible sub-chunk in one pass.
        let truth = Target::new(gpu, wgpu::TextureFormat::Rgba8Unorm, 1);
        let all = front_to_back(&terrain, &candidates, camera.eye);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let candidate_draws =
            draw_single(&raster, &terrain, &truth, &mut encoder, (&all, camera.eye));
        gpu.queue.submit([encoder.finish()]);
        let shown = shown_slots(gpu, &probe, &terrain, &truth, &all, camera.eye);
        let hidden_wrongly = shown
            .iter()
            .copied()
            .filter(|slot| !drawn.contains(slot))
            .collect::<Vec<_>>();
        if !hidden_wrongly.is_empty() {
            counts.wrongly_hidden.push((frame, hidden_wrongly));
        }
        let expected = read_texture(gpu, &truth.color, 0);
        let actual = read_texture(gpu, &culled.color, 0);
        counts.pixel_mismatches += usize::from(expected != actual);
        counts.candidates += candidates.len();
        counts.drawn += drawn.len();
        counts.skipped += candidates.len() - drawn.len();
        counts.candidate_draws += candidate_draws;
        counts.drawn_draws += drawn_draws;
        counts.shown += shown.len();
    }
    counts
}

fn yawed(eye: Vec3, toward: Vec3, degrees: f32) -> Camera {
    let rotation = bevy::math::Quat::from_rotation_y(degrees.to_radians());
    camera(eye, eye + rotation * (toward - eye))
}

/// A pillar near the eye and a wall with a gap, both hiding a row of block-filled sub-chunks.
fn pillar_scene() -> (Terrain, Terrain) {
    let build = |hole: bool| {
        let mut terrain = Terrain::default();
        terrain.add(
            [0, 4, 0],
            cuboid([3, 0, 4], [10, 16, 4], 1).to_vec(),
            Vec::new(),
        );
        for x in [-3, -2, -1, 2, 3] {
            let slab = if hole && x == 2 {
                [
                    cuboid([0, 0, 8], [4, 16, 1], 0),
                    cuboid([10, 0, 8], [6, 16, 1], 0),
                ]
                .concat()
            } else {
                cuboid([0, 0, 8], [16, 16, 1], (x & 1) as u32).to_vec()
            };
            terrain.add([x, 4, -2], slab, Vec::new());
        }
        for x in -2..=3 {
            let blocks = [
                cuboid([2, 3, 2], [4, 5, 4], 0),
                cuboid([9, 6, 10], [3, 3, 3], 1),
                cuboid([5, 11, 6], [6, 2, 2], 0),
            ];
            terrain.add([x, 4, -4], blocks.concat(), Vec::new());
        }
        let floor = cuboid([1, 13, 1], [14, 2, 14], 1);
        let sheet = vec![PackedQuad::new([3, 15, 3], Face::PositiveY, 4, 4, 0)];
        terrain.add([0, 3, 1], floor.to_vec(), sheet);
        terrain
    };
    (build(false), build(true))
}

/// Dwell, flick, small turn, strafe past the pillar, dwell, a wall edit, then back away turning.
fn pillar_path(edited: Terrain) -> Vec<Step> {
    let start = Vec3::new(8.5, 72.5, 24.5);
    let toward = Vec3::new(8.5, 70.0, -60.0);
    let mut steps = Vec::new();
    let mut push = |camera, edit| steps.push(Step { camera, edit });
    for _ in 0..8 {
        push(camera(start, toward), None);
    }
    push(yawed(start, toward, 40.0), None);
    for _ in 0..4 {
        push(yawed(start, toward, 10.0), None);
    }
    for step in 1..=12 {
        let eye = start + Vec3::X * 0.75 * step as f32;
        push(camera(eye, eye + (toward - start)), None);
    }
    let end = start + Vec3::X * 9.0;
    for _ in 0..6 {
        push(camera(end, end + (toward - start)), None);
    }
    let mut edit = Some(edited);
    for _ in 0..6 {
        push(camera(end, end + (toward - start)), edit.take());
    }
    for step in 1..=6 {
        let eye = end + Vec3::Z * 0.5 * step as f32;
        push(yawed(eye, eye + (toward - start), -3.0 * step as f32), None);
    }
    steps
}

/// Fast turns, strafing disocclusion behind a pillar and a wall edit never lose a pixel; a
/// policy that trusts stale verdicts on the same path does.
#[test]
fn replayed_camera_paths_never_skip_a_sub_chunk_that_shows_pixels() {
    let Some(gpu) = Gpu::for_fixture("direct occlusion replay") else {
        return;
    };
    let lags = [1, 2, 1, 3];
    let (terrain, edited) = pillar_scene();
    let production = replay(
        &gpu,
        terrain,
        &pillar_path(edited),
        Policy::Production,
        &lags,
    );
    eprintln!("direct occlusion replay: {production:?}");
    assert!(
        production.wrongly_hidden.is_empty(),
        "{:?}",
        production.wrongly_hidden
    );
    assert_eq!(production.pixel_mismatches, 0);
    assert!(production.skipped > 0, "still frames skip hidden terrain");

    let (terrain, edited) = pillar_scene();
    let naive = replay(&gpu, terrain, &pillar_path(edited), Policy::Naive, &lags);
    assert!(
        !naive.wrongly_hidden.is_empty(),
        "the path must uncover terrain a stale verdict hid"
    );
}

/// Turning in place moves the near plane: a wall just beyond it at the old orientation is
/// clipped at the new one, uncovering terrain an old verdict called occluded.
#[test]
fn a_turn_that_clips_a_near_occluder_never_reuses_old_verdicts() {
    let Some(gpu) = Gpu::for_fixture("direct occlusion near-plane turn") else {
        return;
    };
    let mut terrain = Terrain::default();
    terrain.add(
        [0, 4, 0],
        cuboid([0, 0, 0], [16, 16, 1], 1).to_vec(),
        Vec::new(),
    );
    let blocks = [
        cuboid([2, 3, 2], [10, 10, 10], 0),
        cuboid([12, 1, 4], [3, 3, 3], 1),
    ];
    terrain.add([0, 4, -2], blocks.concat(), Vec::new());
    // The wall's face is at z = 1; the eye sits 0.0505 blocks in front of the 0.05 near plane.
    let eye = Vec3::new(8.5, 72.5, 1.0505);
    let toward = eye - Vec3::Z * 40.0;
    let steps = (0..10)
        .map(|frame| Step {
            camera: if frame < 6 {
                camera(eye, toward)
            } else {
                yawed(eye, toward, 0.2_f32.to_degrees())
            },
            edit: None,
        })
        .collect::<Vec<_>>();
    let counts = replay(&gpu, terrain, &steps, Policy::Production, &[1]);
    assert!(
        counts.skipped > 0,
        "the still frames skip the hidden sub-chunk"
    );
    assert!(
        counts.wrongly_hidden.is_empty(),
        "{:?}",
        counts.wrongly_hidden
    );
    assert_eq!(counts.pixel_mismatches, 0);
}

/// #129's walled scene seen from a still camera: hidden sub-chunks stop being submitted.
#[test]
fn a_still_camera_stops_submitting_sub_chunks_behind_the_wall() {
    let Some(gpu) = Gpu::for_fixture("direct occlusion counters") else {
        return;
    };
    let eye = Vec3::new(8.25, 72.5, 8.75);
    let steps = (0..8)
        .map(|_| Step {
            camera: camera(eye, Vec3::new(10.0, 70.0, -40.0)),
            edit: None,
        })
        .collect::<Vec<_>>();
    let first = replay(&gpu, terrain(), &steps[..1], Policy::Production, &[1]);
    let counts = replay(&gpu, terrain(), &steps, Policy::Production, &[1]);
    eprintln!("direct occlusion walled scene, 8 still frames: {counts:?}");
    assert!(counts.wrongly_hidden.is_empty() && counts.pixel_mismatches == 0);
    assert_eq!(first.skipped, 0, "nothing is skipped before a verdict");
    assert!(counts.skipped > 0 && counts.drawn_draws < counts.candidate_draws);
}

/// A town behind a hill: the hill hides most frustum-visible sub-chunks, each dense with blocks.
fn town_scene() -> Terrain {
    let mut random = Lcg(41);
    let mut terrain = Terrain::default();
    for x in -6..=6 {
        for y in 3..=6 {
            terrain.add(
                [x, y, 0],
                cuboid([0, 0, 8], [16, 16, 1], (x & 1) as u32).to_vec(),
                Vec::new(),
            );
        }
        for z in 1..=2 {
            let floor = cuboid([0, 14, 0], [16, 2, 16], 1);
            terrain.add([x, 3, z], floor.to_vec(), Vec::new());
        }
        for y in 4..=5 {
            for z in -8..=-1 {
                let blocks = (0..300)
                    .flat_map(|_| {
                        let size = [1 + random.next(3), 1 + random.next(3), 1 + random.next(3)];
                        let low = size.map(|side| random.next(16 - side) as u8);
                        cuboid(low, size.map(|side| side as u8), random.next(2))
                    })
                    .collect();
                let sheet = vec![PackedQuad::new([2, 15, 2], Face::PositiveY, 6, 6, 0)];
                terrain.add([x, y, z], blocks, sheet);
            }
        }
    }
    terrain
}

/// Frames per measured submission, so submit-to-idle overhead is amortised.
const REPEATS: usize = 2;

/// Frames per camera phase; the still phase needs two before its first verdict lands.
const PHASE_FRAMES: usize = 16;

/// CPU recording and GPU milliseconds per recording: encode-and-finish time, then the
/// submit-to-idle wall time, each over `REPEATS` recordings.
fn frame_ms(gpu: &Gpu, mut record: impl FnMut(&mut wgpu::CommandEncoder)) -> (f64, f64) {
    let started = Instant::now();
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    for _ in 0..REPEATS {
        record(&mut encoder);
    }
    let commands = encoder.finish();
    let cpu = started.elapsed().as_secs_f64() * 1e3 / REPEATS as f64;
    let started = Instant::now();
    gpu.queue.submit([commands]);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    (cpu, started.elapsed().as_secs_f64() * 1e3 / REPEATS as f64)
}

#[derive(Default)]
struct PhaseTotals {
    frames: usize,
    candidates: usize,
    drawn: usize,
    candidate_draws: usize,
    drawn_draws: usize,
    gpu_before: Vec<f64>,
    gpu_after: Vec<f64>,
    encode_before: Vec<f64>,
    encode_after: Vec<f64>,
    cpu_policy: Vec<f64>,
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
}

/// Before (every frustum-visible sub-chunk, one pass) and after (production occlusion) on a
/// still, turning and walking camera; asserts submitted work and prints GPU and CPU times.
#[test]
fn occlusion_cuts_submitted_terrain_only_while_the_view_holds_still() {
    let Some(gpu) = Gpu::for_fixture("direct occlusion measurement") else {
        return;
    };
    // Small enough for a software adapter; the aspect keeps the frustum's candidate set.
    let size = [320, 180];
    let aspect = size[0] as f32 / size[1] as f32;
    let terrain = town_scene();
    let start = Vec3::new(8.5, 72.5, 40.5);
    let toward = Vec3::new(8.5, 70.0, -60.0);
    let view_at = |eye: Vec3, yaw: f32| {
        let rotation = bevy::math::Quat::from_rotation_y(yaw.to_radians());
        camera_with_aspect(eye, eye + rotation * (toward - start), aspect)
    };
    let phases: [(&str, Vec<Camera>); 3] = [
        (
            "still",
            (0..PHASE_FRAMES).map(|_| view_at(start, 0.0)).collect(),
        ),
        (
            "turning",
            (0..PHASE_FRAMES)
                .map(|frame| view_at(start, 15.0 * ((frame + 1) as f32 * 0.1).sin()))
                .collect(),
        ),
        (
            "walking",
            (0..PHASE_FRAMES)
                .map(|frame| view_at(start + Vec3::X * 0.1 * (frame + 1) as f32, 0.0))
                .collect(),
        ),
    ];
    let occluder = Occluder::new(&gpu, &terrain.records);
    let raster = Raster::new(&gpu, &terrain, &phases[0].1[0]);
    let target = Target::sized(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1, size);
    let pyramid = HizPyramid::new(&gpu.device, size);
    let depth = target.depth.create_view(&Default::default());
    let bindings = occluder
        .kernels
        .pyramid_bindings(&gpu.device, &depth, false, &pyramid);
    let mut history = OcclusionHistory::default();
    for slot in 0..terrain.records.len() as u32 {
        history.assign(slot, 0);
    }
    let mut pending = VecDeque::<Pending>::new();
    let mut previous_pose = None;
    let mut frame = 0;
    let mut report = Vec::new();
    for (name, cameras) in &phases {
        let mut totals = PhaseTotals::default();
        for camera in cameras {
            frame += 1;
            raster.set_camera(&gpu, camera);
            let candidates = (0..terrain.records.len())
                .filter(|&slot| bevy_visible(&camera.frustum, terrain.chunks[slot].0))
                .collect::<Vec<_>>();
            let all = front_to_back(&terrain, &candidates, camera.eye);

            // Before: today's direct path.
            let mut candidate_draws = 0;
            let (encode, gpu_time) = frame_ms(&gpu, |encoder| {
                candidate_draws =
                    draw_single(&raster, &terrain, &target, encoder, (&all, camera.eye));
            });
            totals.encode_before.push(encode);
            totals.gpu_before.push(gpu_time);

            // After: verdicts one frame late, the terrain pass only while the pose holds.
            let started = Instant::now();
            while pending
                .front()
                .is_some_and(|verdict| verdict.deliver <= frame)
            {
                let verdict = pending.pop_front().unwrap();
                history.apply(&verdict.tag, &verdict.words);
            }
            let view = basis_view(camera, history.world(), size);
            let drawn = candidates
                .iter()
                .copied()
                .filter(|&slot| !history.skips(slot as u32, &view))
                .collect::<Vec<_>>();
            let order = front_to_back(&terrain, &drawn, camera.eye);
            totals
                .cpu_policy
                .push(started.elapsed().as_secs_f64() * 1e3);
            let pose = (camera.clip_from_world, camera.clip_from_view);
            let verdict = previous_pose == Some(pose) && !history.settled(&view);
            previous_pose = Some(pose);
            let input = sized_input(camera, pyramid.mip_count(), size);
            let mut drawn_draws = 0;
            let (encode, gpu_time) = frame_ms(&gpu, |encoder| {
                drawn_draws = if verdict {
                    draw_split(
                        &raster,
                        &terrain,
                        &target,
                        encoder,
                        (&order, camera.eye),
                        |encoder| {
                            occluder
                                .kernels
                                .encode_pyramid(encoder, &pyramid, &bindings);
                            occluder.encode(encoder, &input, &pyramid);
                        },
                    )
                } else {
                    draw_single(&raster, &terrain, &target, encoder, (&order, camera.eye))
                };
            });
            totals.encode_after.push(encode);
            totals.gpu_after.push(gpu_time);
            if verdict {
                pending.push_back(Pending {
                    deliver: frame + 1,
                    tag: VerdictTag {
                        frame: frame as u64,
                        basis: view,
                        slots: occluder.slots,
                    },
                    words: occluder.bits(),
                });
            }
            totals.frames += 1;
            totals.candidates += candidates.len();
            totals.drawn += drawn.len();
            totals.candidate_draws += candidate_draws;
            totals.drawn_draws += drawn_draws;
        }
        eprintln!(
            "direct occlusion {name} ({}x{}, {:?}): sub-chunks {:.1} -> {:.1}, draws {:.1} -> {:.1}, \
             gpu terrain median {:.3} -> {:.3} ms, draw encoding median {:.3} -> {:.3} ms, \
             cpu policy median {:.4} ms",
            size[0],
            size[1],
            gpu.backend,
            totals.candidates as f64 / totals.frames as f64,
            totals.drawn as f64 / totals.frames as f64,
            totals.candidate_draws as f64 / totals.frames as f64,
            totals.drawn_draws as f64 / totals.frames as f64,
            median(&totals.gpu_before),
            median(&totals.gpu_after),
            median(&totals.encode_before),
            median(&totals.encode_after),
            median(&totals.cpu_policy),
        );
        report.push((*name, totals));
    }
    let [still, turning, walking] = [&report[0].1, &report[1].1, &report[2].1];
    assert!(
        still.drawn * 2 < still.candidates,
        "the hill hides most of the town"
    );
    assert_eq!(
        turning.drawn, turning.candidates,
        "a turning view never skips"
    );
    assert_eq!(
        walking.drawn, walking.candidates,
        "a moving eye never skips"
    );
    assert_eq!(walking.drawn_draws, walking.candidate_draws);
}
