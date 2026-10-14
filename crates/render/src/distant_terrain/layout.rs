//! GPU layout of the distant terrain renderer: capacities, buffer regions, the uniform blocks
//! and the WGSL prelude generated from the Rust single sources of every shared constant.

use std::fmt::Write as _;

use bytemuck::{Pod, Zeroable};
use lod::{
    bit_field::BitField,
    colour::{FACE_COLOUR_FIELDS, FaceColour, TintClass},
    draw::{
        NEIGHBOUR_LEVEL_BITS, NeighbourLevel,
        biomes::{BIOME_CELL_BITS, BIOME_CELLS_PER_WORD, BIOME_GRID_WORDS, GPU_UNKNOWN_BIOME},
        records::{
            CLUSTER_INFO_FIELDS, CLUSTER_MAX_FIELDS, CLUSTER_MIN_FIELDS, CLUSTER_RECORD_WORDS,
            MAX_SECTION_SLOTS, SECTION_BIOMES_WORD, SECTION_INFO_FIELDS, SECTION_INFO_WORD,
            SECTION_RECORD_WORDS, SECTION_X_WORD,
        },
    },
    geometry::{Face, SECTION_SIDE},
    mesh::{Bucket, MeshList, output::CLUSTER_QUADS, quad::QUAD_GEOMETRY_FIELDS},
    voxel::{MaterialId, VOXEL_FIELDS},
};
use render_scene::distant_terrain::{
    CLUSTER_SIZE_UNIT_PX, DISTANT_TERRAIN_SIZE_BINS, RESIDENT_MASK_SIDE, RESIDENT_MASK_WORDS,
    SECTION_SIZE_UNIT_PX,
};

/// Bytes of one pool unit: one quad, or two words of a packed biome grid.
pub(crate) const POOL_UNIT_BYTES: u64 = 8;
/// Pool units of one packed biome grid, stored ahead of its section's quads.
pub(crate) const BIOME_UNITS: u32 = (BIOME_GRID_WORDS / 2) as u32;
/// Pool quads per cluster slot the cluster table is sized for.
pub(crate) const QUADS_PER_CLUSTER_SLOT: u64 = 32;
/// Section slots; slot 0 is never assigned, so a zeroed cluster record names a dead slot.
pub(crate) const MAX_SECTIONS: u32 = 1 << 16;
/// Threads per culling workgroup.
pub(crate) const WORKGROUP: u32 = 256;
/// Threads per scatter workgroup; one workgroup copies one command.
pub(crate) const SCATTER_WORKGROUP: u32 = 64;
/// Longest run one scatter command copies, so large copies spread over workgroups.
pub(crate) const MAX_COMMAND_WORDS: u32 = 2048;
/// Scatter commands one frame stages at most.
pub(crate) const MAX_COMMANDS: u32 = 16_384;
/// Words of one scatter command: target, destination word, source word or fill value, length.
pub(crate) const COMMAND_WORDS: u32 = 4;
/// Words ahead of the commands in the staging buffer; the first holds the command count.
pub(crate) const STAGING_HEADER_WORDS: u32 = 4;
/// Set in a command's length word: fill with the source word instead of copying.
pub(crate) const FILL_FLAG: u32 = 1 << 31;
/// Indices of the static index buffer: two triangles per quad slot of a cluster.
pub(crate) const INDEX_COUNT: u32 = CLUSTER_QUADS as u32 * 6;
/// Largest per-frame upload of new geometry and records.
pub(crate) const MAX_UPLOAD_BYTES: u64 = 8 << 20;
/// Per-frame upload a renderer starts with.
pub(crate) const START_UPLOAD_BYTES: u64 = 2 << 20;
/// Opacity of distant water. Provisional: not matched to vanilla water.
pub(crate) const WATER_ALPHA: f32 = 0.7;
/// Fixed brightness of each face direction by [`Face::index`]: the ordinary terrain
/// coefficients near chunks use.
pub(crate) const FACE_SHADE: [f32; 6] = {
    let shade = crate::material_shader::ORDINARY_FACE_SHADE;
    let mut out = [0.0; 6];
    let mut index = 0;
    while index < Face::ALL.len() {
        let face = Face::ALL[index];
        out[face.index()] = match (face.axis(), face.is_positive()) {
            (0, _) => shade.x,
            (1, false) => shade.bottom,
            (1, true) => shade.top,
            _ => shade.z,
        };
        index += 1;
    }
    out
};

/// Word offsets of the indirect draws and the counters in the args buffer.
pub(crate) const ARGS_PASS_A: u32 = 0;
pub(crate) const ARGS_PASS_B: u32 = 8;
pub(crate) const ARGS_TRANSLUCENT: u32 = 16;
/// Visible opaque clusters, newly visible ones, visible translucent ones, dropped entries.
pub(crate) const ARGS_STATS: u32 = 24;
/// Visible sections by screen size ([`SIZE_BINS`] words); culling adds to them, so they are
/// cleared with what follows before it runs.
pub(crate) const ARGS_SECTION_BINS: u32 = 32;
/// Visible opaque clusters by screen size, then their empty quad slots.
pub(crate) const ARGS_CLUSTER_BINS: u32 = ARGS_SECTION_BINS + SIZE_BINS;
/// Empty quad slots of visible opaque clusters.
pub(crate) const ARGS_DEGENERATE: u32 = ARGS_CLUSTER_BINS + SIZE_BINS;
pub(crate) const ARGS_WORDS: u32 = (ARGS_DEGENERATE + 1).next_multiple_of(4);
/// Bins of each screen-size histogram.
pub(crate) const SIZE_BINS: u32 = DISTANT_TERRAIN_SIZE_BINS as u32;
/// Words of one culling workgroup's statistics: the size bins and the empty slots.
pub(crate) const STAT_SCRATCH_WORDS: u32 = SIZE_BINS + 1;

/// Scatter destinations, each its own storage binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub(crate) enum Target {
    Pool = 0,
    Clusters = 1,
    Sections = 2,
    Flags = 3,
    Order = 4,
}

const _: () = assert!(MAX_SECTIONS <= MAX_SECTION_SLOTS);
const _: () = assert!(MAX_COMMANDS <= 65_535, "one workgroup per command");
const _: () = assert!(MAX_COMMAND_WORDS.is_multiple_of(SCATTER_WORKGROUP));
const _: () = assert!(INDEX_COUNT <= u16::MAX as u32, "u16 indices");
const _: () = assert!(BIOME_GRID_WORDS.is_multiple_of(2));
const _: () = assert!(CLUSTER_RECORD_WORDS == 4, "one vec4 per cluster record");
const _: () = assert!(SECTION_X_WORD == 0);
const _: () = assert!(ARGS_STATS + 4 <= ARGS_SECTION_BINS && ARGS_WORDS <= 64);
const _: () = assert!(
    STAT_SCRATCH_WORDS <= WORKGROUP,
    "one thread flushes each word"
);

/// The sizes of one renderer's buffers, from its pool budget and the adapter's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capacity {
    /// Quad-sized units of the geometry pool.
    pub(crate) pool_units: u32,
    /// Cluster record slots; also each draw list's and the translucent order's entries.
    pub(crate) clusters: u32,
}

impl Capacity {
    /// The capacity of a `pool_bytes` pool on a device with `limits`; `None` when the
    /// device's storage bindings are too small for even a minimal pool.
    pub(crate) fn new(pool_bytes: u64, limits: &wgpu::Limits) -> Option<Self> {
        let binding = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let pool_units = (pool_bytes.min(binding) / POOL_UNIT_BYTES).min(u64::from(u32::MAX));
        let by_pool = pool_units / QUADS_PER_CLUSTER_SLOT;
        let by_binding = binding / (4 * u64::from(CLUSTER_RECORD_WORDS as u32));
        let clusters = by_pool.min(by_binding).min(u64::from(u32::MAX) / 4) as u32;
        let capacity = Self {
            pool_units: pool_units as u32,
            clusters: clusters / WORKGROUP * WORKGROUP,
        };
        let fits = [
            capacity.sections_bytes(),
            capacity.flags_bytes(),
            capacity.lists_bytes(),
            capacity.staging_bytes(),
        ]
        .iter()
        .all(|&bytes| bytes <= binding);
        (capacity.clusters >= WORKGROUP
            && u64::from(capacity.pool_units) > u64::from(BIOME_UNITS)
            && fits)
            .then_some(capacity)
    }

    /// Bytes of the geometry pool.
    pub(crate) fn pool_bytes(self) -> u64 {
        u64::from(self.pool_units) * POOL_UNIT_BYTES
    }

    /// Bytes of the cluster records.
    pub(crate) fn clusters_bytes(self) -> u64 {
        u64::from(self.clusters) * CLUSTER_RECORD_WORDS as u64 * 4
    }

    /// Words of the section records, followed by the resident column mask.
    pub(crate) fn sections_words(self) -> u64 {
        u64::from(MAX_SECTIONS) * SECTION_RECORD_WORDS as u64 + RESIDENT_MASK_WORDS as u64
    }

    /// Bytes of [`Self::sections_words`].
    pub(crate) fn sections_bytes(self) -> u64 {
        self.sections_words() * 4
    }

    /// The first word of the resident column mask in the sections buffer.
    pub(crate) fn resident_word(self) -> u32 {
        MAX_SECTIONS * SECTION_RECORD_WORDS as u32
    }

    /// The first word of the visible-last-frame history among the flag regions: opaque
    /// decisions, the history, translucent decisions, then section visibility, each region
    /// [`Self::clusters`] words but the last.
    pub(crate) fn history_word(self) -> u32 {
        self.clusters
    }

    /// Bytes of the four flag regions.
    pub(crate) fn flags_bytes(self) -> u64 {
        (3 * u64::from(self.clusters) + u64::from(MAX_SECTIONS)) * 4
    }

    /// Pass A's list from the start and pass B's from the end of the first half, the
    /// translucent list in the second.
    pub(crate) fn lists_bytes(self) -> u64 {
        2 * u64::from(self.clusters) * 4
    }

    /// Workgroup totals of the compaction scans.
    pub(crate) fn group_sums_bytes(self) -> u64 {
        u64::from(self.clusters.div_ceil(WORKGROUP).max(1)) * 8
    }

    /// Bytes of the water order: one cluster slot per entry.
    pub(crate) fn order_bytes(self) -> u64 {
        u64::from(self.clusters) * 4
    }

    /// Words of the staging buffer: the header, the commands, a full upload, a full
    /// translucent order and every section record.
    pub(crate) fn staging_words(self) -> u64 {
        u64::from(STAGING_HEADER_WORDS)
            + u64::from(MAX_COMMANDS * COMMAND_WORDS)
            + MAX_UPLOAD_BYTES / 4
            + u64::from(self.clusters)
            + u64::from(MAX_SECTIONS) * SECTION_RECORD_WORDS as u64
    }

    /// Bytes of [`Self::staging_words`].
    pub(crate) fn staging_bytes(self) -> u64 {
        self.staging_words() * 4
    }
}

/// The view every distant terrain kernel and pipeline reads; see the WGSL `LodView`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct LodView {
    /// Clip space from blocks relative to the integer eye.
    pub(crate) clip_from_rel: [[f32; 4]; 4],
    /// Left, right, bottom, top and near planes relative to the integer eye, inside positive.
    pub(crate) planes: [[f32; 4]; 5],
    /// The eye's integer block; `w` unused.
    pub(crate) camera: [i32; 4],
    /// The eye's offset from its integer block; `w` unused.
    pub(crate) camera_frac: [f32; 4],
    pub(crate) viewport: [f32; 4],
    /// Depth width, height, pyramid mips, Hi-Z enabled; named as `hiz_test.wgsl` reads it.
    pub(crate) depth: [u32; 4],
    /// Section slots, cluster slots and translucent order entries in use, and the cluster
    /// capacity.
    pub(crate) counts: [u32; 4],
    /// Resident mask centre column X and Z, its radius, and 1 when it holds columns.
    pub(crate) resident: [i32; 4],
    /// Fog colour and start.
    pub(crate) fog: [f32; 4],
    /// Fog end, water alpha, unused.
    pub(crate) fog_end: [f32; 4],
    /// [`FACE_SHADE`] by face index.
    pub(crate) shade: [[f32; 4]; 2],
    /// Material entries, raw biome ids and their first word, tint records' first word.
    pub(crate) tables: [u32; 4],
    /// Tint records; unused.
    pub(crate) tables_extra: [u32; 4],
}

/// One draw pass's choice of list: entry `i` reads list word `base + i`, or `base - i` when
/// `backward` is set.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct DrawPass {
    pub(crate) base: u32,
    pub(crate) backward: u32,
    pub(crate) translucent: u32,
    pub(crate) spare: u32,
}

/// Bytes between the draw passes' entries in their dynamic-offset uniform buffer.
pub(crate) const DRAW_PASS_STRIDE: u64 = 256;
/// Draw passes: A, B and translucent.
pub(crate) const DRAW_PASSES: usize = 3;

/// The upper-case WGSL spelling of a field's name, `QUAD_X` for `x` in `QUAD`.
fn field_const(prefix: &str, field: &BitField) -> String {
    format!("{prefix}_{}", field.name.to_ascii_uppercase())
}

/// Every unsigned constant the shaders share with Rust, by WGSL name.
pub(crate) fn shader_constants() -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let sets: [(&str, &[BitField]); 7] = [
        ("QUAD", &QUAD_GEOMETRY_FIELDS),
        ("VOXEL", &VOXEL_FIELDS),
        ("COLOUR", &FACE_COLOUR_FIELDS),
        ("CLUSTER", &CLUSTER_INFO_FIELDS),
        ("CLUSTER_MIN", &CLUSTER_MIN_FIELDS),
        ("CLUSTER_MAX", &CLUSTER_MAX_FIELDS),
        ("SECTION", &SECTION_INFO_FIELDS),
    ];
    for (prefix, fields) in sets {
        for field in fields.iter().filter(|field| field.name != "spare") {
            let name = field_const(prefix, field);
            out.push((format!("{name}_SHIFT"), field.shift));
            out.push((format!("{name}_MASK"), field.max_value()));
        }
    }
    let list = |list: MeshList| list.index() as u32;
    out.extend(
        [
            ("SECTION_SIDE", SECTION_SIDE as u32),
            (
                "SECTION_CHUNKS",
                (SECTION_SIDE / world::SUB_CHUNK_SIDE) as u32,
            ),
            ("SECTION_RECORD_WORDS", SECTION_RECORD_WORDS as u32),
            ("SECTION_INFO_WORD", SECTION_INFO_WORD as u32),
            ("SECTION_BIOMES_WORD", SECTION_BIOMES_WORD as u32),
            ("CLUSTER_QUADS", CLUSTER_QUADS as u32),
            ("BIOME_CELL_BITS", BIOME_CELL_BITS),
            ("BIOME_CELLS_PER_WORD", BIOME_CELLS_PER_WORD as u32),
            ("GPU_UNKNOWN_BIOME", u32::from(GPU_UNKNOWN_BIOME)),
            ("TINT_CLASSES", TintClass::COUNT as u32),
            ("FACES", Face::ALL.len() as u32),
            ("HIDDEN_MATERIAL", MaterialId::HIDDEN.raw()),
            ("UNKNOWN_COLOUR", FaceColour::UNKNOWN.bits()),
            ("HIDDEN_COLOUR", FaceColour::HIDDEN.bits()),
            ("NEIGHBOUR_LEVEL_BITS", NEIGHBOUR_LEVEL_BITS),
            ("NEIGHBOUR_COARSER", NeighbourLevel::Coarser as u32),
            ("NEIGHBOUR_FINER", NeighbourLevel::Finer as u32),
            ("LIST_MAIN", list(MeshList::Main)),
            ("LIST_TO_COARSER", list(MeshList::ToCoarser)),
            ("LIST_TO_FINER", list(MeshList::ToFiner)),
            ("BUCKET_OPAQUE", Bucket::Opaque.index() as u32),
            ("BUCKET_TRANSLUCENT", Bucket::Translucent.index() as u32),
            ("BIOME_UNITS", BIOME_UNITS),
            ("WORKGROUP", WORKGROUP),
            ("SCATTER_WORKGROUP", SCATTER_WORKGROUP),
            ("COMMAND_WORDS", COMMAND_WORDS),
            ("STAGING_HEADER_WORDS", STAGING_HEADER_WORDS),
            ("FILL_FLAG", FILL_FLAG),
            ("TARGET_POOL", Target::Pool as u32),
            ("TARGET_CLUSTERS", Target::Clusters as u32),
            ("TARGET_SECTIONS", Target::Sections as u32),
            ("TARGET_FLAGS", Target::Flags as u32),
            ("TARGET_ORDER", Target::Order as u32),
            ("ARGS_PASS_A", ARGS_PASS_A),
            ("ARGS_PASS_B", ARGS_PASS_B),
            ("ARGS_TRANSLUCENT", ARGS_TRANSLUCENT),
            ("ARGS_STATS", ARGS_STATS),
            ("ARGS_SECTION_BINS", ARGS_SECTION_BINS),
            ("ARGS_CLUSTER_BINS", ARGS_CLUSTER_BINS),
            ("SIZE_BINS", SIZE_BINS),
            ("STAT_SCRATCH_WORDS", STAT_SCRATCH_WORDS),
            ("INDEX_COUNT", INDEX_COUNT),
            ("RESIDENT_MASK_SIDE", RESIDENT_MASK_SIDE),
            ("MAX_SECTIONS", MAX_SECTIONS),
            ("LIGHTMAP_ENTRIES", render_model::LIGHTMAP_ENTRIES as u32),
        ]
        .map(|(name, value)| (name.to_owned(), value)),
    );
    out
}

/// Every float constant the shaders share with Rust, by WGSL name.
pub(crate) fn shader_float_constants() -> Vec<(String, f32)> {
    vec![
        ("HIZ_PADDING".to_owned(), crate::chunk::HIZ_PADDING),
        ("SECTION_SIZE_UNIT_PX".to_owned(), SECTION_SIZE_UNIT_PX),
        ("CLUSTER_SIZE_UNIT_PX".to_owned(), CLUSTER_SIZE_UNIT_PX),
    ]
}

/// The WGSL prelude every distant terrain shader starts with.
pub(crate) fn prelude() -> String {
    let mut out = String::from("// Generated from the Rust layouts; see `layout.rs`.\n");
    for (name, value) in shader_constants() {
        let _ = writeln!(out, "const {name}: u32 = {value}u;");
    }
    for (name, value) in shader_float_constants() {
        let _ = writeln!(out, "const {name}: f32 = {value:?}f;");
    }
    out.push_str(VIEW_WGSL);
    out
}

/// The WGSL mirror of [`LodView`] and [`DrawPass`]; [`tests`] checks their sizes and
/// offsets against the Rust structs through naga.
pub(crate) const VIEW_WGSL: &str = "
struct LodView {
    clip_from_rel: mat4x4<f32>,
    planes: array<vec4<f32>, 5>,
    camera: vec4<i32>,
    camera_frac: vec4<f32>,
    viewport: vec4<f32>,
    depth: vec4<u32>,
    counts: vec4<u32>,
    resident: vec4<i32>,
    fog: vec4<f32>,
    fog_end: vec4<f32>,
    shade: array<vec4<f32>, 2>,
    tables: vec4<u32>,
    tables_extra: vec4<u32>,
}

struct DrawPass {
    base: u32,
    backward: u32,
    translucent: u32,
    spare: u32,
}
";

/// The [`LodView::shade`] value of [`FACE_SHADE`].
pub(crate) fn shade_uniform() -> [[f32; 4]; 2] {
    [
        [FACE_SHADE[0], FACE_SHADE[1], FACE_SHADE[2], FACE_SHADE[3]],
        [FACE_SHADE[4], FACE_SHADE[5], 0.0, 0.0],
    ]
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
