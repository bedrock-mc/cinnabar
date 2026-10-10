//! Persistent cull records, the per-pass cull uniform, and a CPU reference of the cull kernels.
#![allow(
    dead_code,
    reason = "the CPU reference and accessors serve the fixture tests"
)]

use std::ops::Range;

use bytemuck::{Pod, Zeroable};

/// Threads per cull workgroup; the kernels are generated with this size.
pub const CULL_WORKGROUP: u32 = 256;
/// Facing slots never form more than three separate solid runs.
pub const MAX_SOLID_RUNS: u32 = 3;
/// Words per `DrawIndexedIndirectArgs`.
pub const ARGS_WORDS: u32 = 5;
pub const STREAM_COUNT: usize = 4;
pub const PHASE_COUNT: usize = 2;
/// Hi-Z boxes grow by this many blocks so rasterised depth never lands outside them.
pub const HIZ_PADDING: f32 = 1.0 / 16.0;
/// Relative and absolute slack that keeps the GPU frustum test a superset of the CPU one.
pub const FRUSTUM_RELATIVE_SLACK: f32 = 1.0e-5;
pub const FRUSTUM_ABSOLUTE_SLACK: f32 = 1.0e-3;
const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
const BOUNDS_BIAS: i32 = 128;

/// Draw-offset vertex entries per culled command, one per quad corner.
pub const OFFSET_CORNERS: u32 = 4;
/// Bytes of one draw-offset entry: the draw's own base vertex plus corner, then its first instance.
pub const OFFSET_ENTRY_BYTES: u64 = 8;

/// Bytes of the draw-offset vertex buffer behind the args of `capacity` slots.
pub fn draw_offset_bytes(capacity: u32) -> u64 {
    args_words(capacity) / u64::from(ARGS_WORDS) * u64::from(OFFSET_CORNERS) * OFFSET_ENTRY_BYTES
}

/// The draw a culled command performs. Its `base_vertex` addresses its four entries in
/// `offsets`, which carry the base vertex and first instance it stands for.
pub fn resolve_culled_args(
    args: [u32; ARGS_WORDS as usize],
    offsets: &[[u32; 2]],
) -> [u32; ARGS_WORDS as usize] {
    let [base_vertex, first_instance] = offsets[args[3] as usize];
    [
        args[0],
        args[1],
        args[2],
        base_vertex,
        first_instance + args[4],
    ]
}

/// Compacted draw streams, in the order the opaque pass draws them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullStream {
    Solid,
    Cutout,
    Model,
    Liquid,
}

impl CullStream {
    pub const ALL: [Self; STREAM_COUNT] = [Self::Solid, Self::Cutout, Self::Model, Self::Liquid];

    pub const fn draws_per_record(self) -> u32 {
        match self {
            Self::Solid => MAX_SOLID_RUNS,
            _ => 1,
        }
    }
}

/// Early draws what was visible last frame; late tests the rest against this frame's Hi-Z.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullPhase {
    Early,
    Late,
}

impl CullPhase {
    pub const ALL: [Self; PHASE_COUNT] = [Self::Early, Self::Late];
}

/// Word offset of one phase's stream region in the shared args buffer of `capacity` slots.
pub fn args_region(capacity: u32, phase: CullPhase, stream: CullStream) -> u32 {
    let per_phase: u32 = CullStream::ALL.iter().map(|s| s.draws_per_record()).sum();
    let before: u32 = CullStream::ALL
        .iter()
        .take_while(|&&s| s != stream)
        .map(|s| s.draws_per_record())
        .sum();
    (phase as u32 * per_phase + before) * capacity * ARGS_WORDS
}

pub fn args_words(capacity: u32) -> u64 {
    let per_phase: u64 = CullStream::ALL
        .iter()
        .map(|s| u64::from(s.draws_per_record()))
        .sum();
    PHASE_COUNT as u64 * per_phase * u64::from(capacity) * u64::from(ARGS_WORDS)
}

/// Index of a phase's stream count in the indirect count buffer.
pub const fn count_index(phase: CullPhase, stream: CullStream) -> u32 {
    phase as u32 * STREAM_COUNT as u32 + stream as u32
}

pub const fn group_count(slots: u32) -> u32 {
    slots.div_ceil(CULL_WORKGROUP)
}

/// One resident sub-chunk's opaque draws, indexed by its arena metadata slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct CullRecord {
    pub origin: [i32; 3],
    pub base_vertex: i32,
    bounds_min: u32,
    bounds_max: u32,
    cube_start: u32,
    cube_end: u32,
    solid_ends: [u32; 3],
    model_start: u32,
    model_count: u32,
    liquid_start: u32,
    liquid_count: u32,
    live: u32,
}

/// Validated draw ranges for [`CullRecord::new`]; empty ranges draw nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CullRecordSource {
    pub origin: [i32; 3],
    pub base_vertex: i32,
    /// Local block bounds of every drawn stream, inclusive min and exclusive max.
    pub bounds: [[i32; 3]; 2],
    pub cube: Range<u32>,
    /// Cumulative local solid-run ends in `CubeQuadLayout::SOLID_FACE_ORDER`.
    pub solid_ends: [u32; 6],
    pub model: Range<u32>,
    pub liquid: Range<u32>,
}

impl CullRecord {
    /// `None` when the solid runs overrun the cube range or a bound leaves the packed range.
    pub fn new(source: &CullRecordSource) -> Option<Self> {
        let cube_len = source.cube.end.checked_sub(source.cube.start)?;
        let ends = source.solid_ends;
        if ends.windows(2).any(|pair| pair[0] > pair[1]) || ends[5] > cube_len || ends[5] > 0xffff {
            return None;
        }
        let pack = |bound: [i32; 3]| -> Option<u32> {
            bound
                .iter()
                .enumerate()
                .try_fold(0, |word, (axis, &value)| {
                    let biased = u8::try_from(value + BOUNDS_BIAS).ok()?;
                    Some(word | (u32::from(biased) << (8 * axis)))
                })
        };
        let pairs = std::array::from_fn(|index| ends[2 * index] | (ends[2 * index + 1] << 16));
        let live = !source.cube.is_empty() || !source.model.is_empty() || !source.liquid.is_empty();
        Some(Self {
            origin: source.origin,
            base_vertex: source.base_vertex,
            bounds_min: pack(source.bounds[0])?,
            bounds_max: pack(source.bounds[1])?,
            cube_start: source.cube.start,
            cube_end: source.cube.end,
            solid_ends: pairs,
            model_start: source.model.start,
            model_count: source.model.end.checked_sub(source.model.start)?,
            liquid_start: source.liquid.start,
            liquid_count: source.liquid.end.checked_sub(source.liquid.start)?,
            live: u32::from(live),
        })
    }

    pub const fn is_live(&self) -> bool {
        self.live != 0
    }

    fn solid_end(&self, slot: usize) -> u32 {
        (self.solid_ends[slot / 2] >> (16 * (slot % 2))) & 0xffff
    }

    /// Most draws the kernels can emit for this record in each stream, from any eye.
    pub fn max_draws(&self) -> [u32; STREAM_COUNT] {
        let solid_slots = (0..6)
            .filter(|&slot| {
                let start = if slot == 0 {
                    0
                } else {
                    self.solid_end(slot - 1)
                };
                start != self.solid_end(slot)
            })
            .count() as u32;
        [
            solid_slots.min(MAX_SOLID_RUNS),
            u32::from(!self.cutout().is_empty()),
            u32::from(self.model_count != 0),
            u32::from(self.liquid_count != 0),
        ]
    }

    /// Merged absolute solid runs whose face bit is in `facing`, as the CPU path draws them.
    pub fn solid_runs(&self, facing: u8) -> Vec<Range<u32>> {
        let mut runs: Vec<Range<u32>> = Vec::new();
        for slot in 0..6 {
            let face = solid_slot_face(slot);
            let start = if slot == 0 {
                0
            } else {
                self.solid_end(slot - 1)
            };
            let end = self.solid_end(slot);
            if facing & (1 << face) == 0 || start == end {
                continue;
            }
            let range = self.cube_start + start..self.cube_start + end;
            match runs.last_mut() {
                Some(run) if run.end == range.start => run.end = range.end,
                _ => runs.push(range),
            }
        }
        runs
    }

    pub fn cutout(&self) -> Range<u32> {
        if self.cube_start == self.cube_end {
            return self.cube_start..self.cube_start;
        }
        self.cube_start + self.solid_end(5)..self.cube_end
    }

    pub fn model(&self) -> Range<u32> {
        self.model_start..self.model_start + self.model_count
    }

    pub fn liquid(&self) -> Range<u32> {
        self.liquid_start..self.liquid_start + self.liquid_count
    }

    pub fn bounds(&self) -> [[i32; 3]; 2] {
        let unpack = |word: u32| {
            std::array::from_fn(|axis| ((word >> (8 * axis)) & 0xff) as i32 - BOUNDS_BIAS)
        };
        [unpack(self.bounds_min), unpack(self.bounds_max)]
    }
}

/// `Face as u8` drawn by a solid slot in `CubeQuadLayout::SOLID_FACE_ORDER`.
const fn solid_slot_face(slot: usize) -> u32 {
    if slot < 3 {
        2 * slot as u32
    } else {
        2 * (slot as u32 - 3) + 1
    }
}

/// Eye for facing selection, split so the GPU compares integers exactly as the CPU compares f64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CullCamera {
    pub floor: [i32; 3],
    pub fraction: [bool; 3],
    pub all_faces: bool,
}

impl CullCamera {
    /// `None`, a non-finite eye or one beyond `i32` keeps every face, as the CPU path does.
    pub fn new(eye: Option<[f64; 3]>) -> Self {
        let all = Self {
            floor: [0; 3],
            fraction: [false; 3],
            all_faces: true,
        };
        let Some(eye) = eye else {
            return all;
        };
        let range = f64::from(i32::MIN)..f64::from(i32::MAX);
        if !eye.iter().all(|value| range.contains(&value.floor())) {
            return all;
        }
        Self {
            floor: eye.map(|value| value.floor() as i32),
            fraction: eye.map(|value| value != value.floor()),
            all_faces: false,
        }
    }

    /// Bit `Face as u8` set for each face direction some quad can show to the eye.
    pub fn facing(&self, origin: [i32; 3]) -> u8 {
        if self.all_faces {
            return 0x3f;
        }
        let mut mask = 0;
        for (axis, &low) in origin.iter().enumerate() {
            let (eye, low) = (i64::from(self.floor[axis]), i64::from(low));
            if eye < low + i64::from(SIDE) {
                mask |= 1 << (2 * axis);
            }
            if eye > low || (eye == low && self.fraction[axis]) {
                mask |= 1 << (2 * axis + 1);
            }
        }
        mask
    }

    pub(crate) fn word(&self) -> i32 {
        let fraction = self
            .fraction
            .iter()
            .enumerate()
            .fold(0, |word, (axis, &set)| {
                word | (i32::from(set) << (axis + 1))
            });
        i32::from(self.all_faces) | fraction
    }
}

/// Per-view inputs shared by both phases.
#[derive(Clone, Copy, Debug)]
pub struct CullViewInput {
    /// Bevy's left, right, top, bottom and near half-spaces, unit normal and distance.
    pub planes: [[f32; 4]; 5],
    /// Column-major `clip_from_world`.
    pub clip_from_world: [[f64; 4]; 4],
    pub camera: CullCamera,
    /// Physical viewport origin and size inside the depth texture.
    pub viewport: [f32; 4],
    pub depth_size: [u32; 2],
    /// Pyramid mip count, or zero when the late phase cannot test Hi-Z.
    pub hiz_mips: u32,
    pub index_counts: [u32; STREAM_COUNT],
}

/// The uniform both cull kernels read; its WGSL twin is `CullView`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CullViewUniform {
    pub planes: [[f32; 4]; 5],
    pub clip_from_rel: [[f32; 4]; 4],
    pub camera: [i32; 4],
    pub viewport: [f32; 4],
    pub depth: [u32; 4],
    pub params: [u32; 4],
    pub regions: [u32; 4],
    pub index_counts: [u32; 4],
}

impl CullViewUniform {
    pub fn new(input: &CullViewInput, phase: CullPhase, slots: u32, capacity: u32) -> Self {
        let floor = input.camera.floor.map(f64::from);
        let m = input.clip_from_world;
        // Corners arrive relative to the integer eye, so large coordinates keep their precision.
        let translation = std::array::from_fn(|row| {
            m[0][row] * floor[0] + m[1][row] * floor[1] + m[2][row] * floor[2] + m[3][row]
        });
        let columns = [m[0], m[1], m[2], translation];
        Self {
            planes: input.planes,
            clip_from_rel: columns.map(|column| column.map(|value| value as f32)),
            camera: [
                input.camera.floor[0],
                input.camera.floor[1],
                input.camera.floor[2],
                input.camera.word(),
            ],
            viewport: input.viewport,
            depth: [
                input.depth_size[0],
                input.depth_size[1],
                input.hiz_mips,
                u32::from(input.hiz_mips != 0),
            ],
            params: [
                slots,
                group_count(slots),
                phase as u32,
                count_index(phase, CullStream::Solid),
            ],
            regions: CullStream::ALL.map(|stream| args_region(capacity, phase, stream)),
            index_counts: input.index_counts,
        }
    }
}

pub fn slot_enabled(bits: &[u32], slot: usize) -> bool {
    bits.get(slot / 32)
        .is_some_and(|word| word >> (slot % 32) & 1 != 0)
}

/// The kernels' frustum slack for one plane and box centre.
pub fn frustum_slack(plane: [f32; 4], center: [f32; 3]) -> f32 {
    let spread: f32 = (0..3).map(|axis| (plane[axis] * center[axis]).abs()).sum();
    FRUSTUM_RELATIVE_SLACK * (plane[3].abs() + spread) + FRUSTUM_ABSOLUTE_SLACK
}

/// Compacted draws for `visible` slots in slot order, as the emit kernel's commands perform them
/// once resolved through [`resolve_culled_args`].
pub fn reference_args(
    records: &[CullRecord],
    camera: CullCamera,
    index_counts: [u32; STREAM_COUNT],
    visible: impl Fn(usize) -> bool,
) -> [Vec<[u32; ARGS_WORDS as usize]>; STREAM_COUNT] {
    let mut args: [Vec<[u32; 5]>; STREAM_COUNT] = Default::default();
    for (slot, record) in records.iter().enumerate() {
        if !record.is_live() || !visible(slot) {
            continue;
        }
        let draw = |stream: CullStream, range: Range<u32>| {
            [
                index_counts[stream as usize],
                range.end - range.start,
                0,
                record.base_vertex as u32,
                range.start,
            ]
        };
        for run in record.solid_runs(camera.facing(record.origin)) {
            args[CullStream::Solid as usize].push(draw(CullStream::Solid, run));
        }
        for (stream, range) in [
            (CullStream::Cutout, record.cutout()),
            (CullStream::Model, record.model()),
            (CullStream::Liquid, record.liquid()),
        ] {
            if !range.is_empty() {
                args[stream as usize].push(draw(stream, range));
            }
        }
    }
    args
}

/// The occlusion kernel's bit for `record` against read-back pyramid `levels`; `strict` keeps
/// boxes reaching past the viewport. `lean` of +1 or -1 tilts every comparison toward visible or
/// occluded, so tests can set rounding ties aside.
pub fn reference_occluded(
    record: &CullRecord,
    view: &CullViewUniform,
    levels: &[(Vec<f32>, [u32; 2])],
    strict: bool,
    lean: f32,
) -> bool {
    if !record.is_live() || view.depth[3] == 0 {
        return false;
    }
    let [low, high] = record.bounds();
    let base: [f32; 3] =
        std::array::from_fn(|axis| (record.origin[axis] - view.camera[axis]) as f32);
    let m = view.clip_from_rel;
    let (mut ndc_min, mut ndc_max, mut nearest) = ([1.0e30_f32; 2], [-1.0e30_f32; 2], 0.0_f32);
    for corner in 0..8 {
        let point: [f32; 3] = std::array::from_fn(|axis| {
            base[axis]
                + if corner >> axis & 1 != 0 {
                    high[axis] as f32 + HIZ_PADDING
                } else {
                    low[axis] as f32 - HIZ_PADDING
                }
        });
        let clip: [f32; 4] = std::array::from_fn(|row| {
            m[0][row] * point[0] + m[1][row] * point[1] + m[2][row] * point[2] + m[3][row]
        });
        if clip[3].is_nan() || clip[3] <= 1.0e-4 {
            return false;
        }
        for axis in 0..2 {
            ndc_min[axis] = ndc_min[axis].min(clip[axis] / clip[3]);
            ndc_max[axis] = ndc_max[axis].max(clip[axis] / clip[3]);
        }
        nearest = nearest.max(clip[2] / clip[3]);
    }
    let edge = 1.0 - lean * 1.0e-5;
    if strict
        && (ndc_min.iter().any(|&value| value < -edge) || ndc_max.iter().any(|&value| value > edge))
    {
        return false;
    }
    let ndc_min = ndc_min.map(|value| value.clamp(-1.0, 1.0));
    let ndc_max = ndc_max.map(|value| value.clamp(-1.0, 1.0));
    let vp = view.viewport;
    let left = vp[0] + (ndc_min[0] * 0.5 + 0.5) * vp[2];
    let right = vp[0] + (ndc_max[0] * 0.5 + 0.5) * vp[2];
    let top = vp[1] + (0.5 - ndc_max[1] * 0.5) * vp[3];
    let bottom = vp[1] + (0.5 - ndc_min[1] * 0.5) * vp[3];
    let limit = [view.depth[0] as f32 - 1.0, view.depth[1] as f32 - 1.0];
    let dilation = 1.0 + lean;
    let pixel = |value: f32, axis: usize| value.clamp(0.0, limit[axis]) as u32;
    let p0 = [
        pixel(left.floor() - dilation, 0),
        pixel(top.floor() - dilation, 1),
    ];
    let p1 = [
        pixel(right.floor() + dilation, 0),
        pixel(bottom.floor() + dilation, 1),
    ];
    let mut level = 0;
    while level + 1 < view.depth[2] {
        let shift = level + 1;
        let span = |axis: usize| (p1[axis] >> shift) - (p0[axis] >> shift);
        if span(0) <= 3 && span(1) <= 3 {
            break;
        }
        level += 1;
    }
    let (texels, size) = &levels[level as usize];
    let shift = level + 1;
    let last = [size[0] - 1, size[1] - 1];
    let t0 = [(p0[0] >> shift).min(last[0]), (p0[1] >> shift).min(last[1])];
    let t1 = [(p1[0] >> shift).min(last[0]), (p1[1] >> shift).min(last[1])];
    let mut farthest = 1.0_f32;
    for y in t0[1]..=t1[1] {
        for x in t0[0]..=t1[0] {
            farthest = farthest.min(texels[(y * size[0] + x) as usize]);
        }
    }
    nearest + lean * 1.0e-6 < farthest
}
