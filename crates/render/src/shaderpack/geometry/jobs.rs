//! CPU planning of one section's expansion: which faces each packed record produces, in which
//! Iris geometry category, and the jobs the expansion kernels run to write them.
//!
//! A job is four words `[record, origin slot, first output face, flags]`. Cube jobs cover one
//! block of a greedy quad (its width and height steps in flag bits 0-15); every job reserves an
//! extra slot for a grass-side overlay ([`JOB_OVERLAY_FACE`]) and for the back of a two-sided
//! face ([`JOB_REVERSE_FACE`]) when it may need them, and the kernel leaves unneeded slots
//! degenerate. Cube materials and their positional variants reserve reverse faces when two-sided;
//! liquids take their sidedness from the record, block-model
//! quads from the template quad's own two-sided flag, as the chunk renderer's model fragment
//! gate does.

use std::ops::Range;

use assets::{Material, ModelQuad, ModelTemplate};
use meshing::{PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad};

/// Job flag: the slot after the face holds a grass-side overlay face.
pub(crate) const JOB_OVERLAY_FACE: u32 = 1 << 16;
/// Job flag: the last slot holds the back of a two-sided face.
pub(crate) const JOB_REVERSE_FACE: u32 = 1 << 17;
/// Job flag: the job's only slot holds the back of a two-sided face, as its own job because
/// it faces the other way and so belongs to the opposite facing group.
pub(crate) const JOB_BACK_ONLY: u32 = 1 << 18;

/// Iris geometry categories, in the order a section's faces are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeometryCategory {
    /// Opaque single-sided terrain and lava (`gbuffers_terrain`, Iris `gbuffers_terrain_solid`).
    Solid,
    /// Alpha-tested and two-sided terrain, block models and grass-side overlays
    /// (`gbuffers_terrain`, Iris `gbuffers_terrain_cutout`).
    Cutout,
    /// Water and translucent block models (`gbuffers_water`), drawn far to near by section.
    Translucent,
}

impl GeometryCategory {
    pub const ALL: [Self; 3] = [Self::Solid, Self::Cutout, Self::Translucent];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// Expansion kernel a job runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobKind {
    Cube,
    Model,
    Liquid,
}

pub(crate) const JOB_KINDS: [JobKind; 3] = [JobKind::Cube, JobKind::Model, JobKind::Liquid];

/// Everything planning reads about one resident section. Record bases are the section's first
/// global record in each GPU stream, as its allocation placed them.
pub(crate) struct SectionGeometry<'a> {
    pub(crate) metadata_index: u32,
    /// Global index of the first cube quad.
    pub(crate) quad_base: u32,
    pub(crate) cube_quads: &'a [PackedQuad],
    /// Leading cube quads that are opaque and single-sided.
    pub(crate) solid_len: u32,
    pub(crate) model_refs: &'a [PackedModelRef],
    /// Global record of the first opaque model draw ref.
    pub(crate) model_draw_base: u32,
    pub(crate) model_draw_refs: &'a [PackedModelDrawRef],
    /// Global record of the first translucent model draw ref.
    pub(crate) transparent_model_draw_base: u32,
    pub(crate) transparent_model_draw_refs: &'a [PackedModelDrawRef],
    /// Global record of the first liquid record.
    pub(crate) liquid_base: u32,
    pub(crate) liquid_quads: &'a [PackedLiquidQuad],
}

/// The carrier tables planning resolves materials through.
pub(crate) struct MaterialTables<'a> {
    pub(crate) materials: &'a [Material],
    pub(crate) model_templates: &'a [ModelTemplate],
    pub(crate) model_quads: &'a [ModelQuad],
}

/// Groups of a category's faces: single-sided cube faces by the axis direction they face, in
/// `meshing::Face` order, then every other face (two-sided cubes, models, liquids), as
/// [`FACING_REST`]. A view only needs the axis groups whose faces it can see from the front.
pub(crate) const FACING_GROUPS: usize = 7;
/// Group of the faces that face no single axis direction.
pub(crate) const FACING_REST: usize = 6;

/// A planned section: its face count, per-category face ranges and kernel jobs, with output
/// faces relative to the section's first face until [`SectionPlan::place`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SectionPlan {
    pub(crate) faces: u32,
    pub(crate) categories: [Range<u32>; 3],
    /// Each category's faces by [`FACING_GROUPS`] group, consecutive within the category.
    pub(crate) facing: [[Range<u32>; FACING_GROUPS]; 3],
    pub(crate) jobs: [Vec<[u32; 4]>; 3],
}

impl SectionPlan {
    /// Moves every output face to start at arena face `first_face`.
    pub(crate) fn place(&mut self, first_face: u32) {
        let shift =
            |range: &mut Range<u32>| *range = range.start + first_face..range.end + first_face;
        self.categories.iter_mut().for_each(shift);
        self.facing.iter_mut().flatten().for_each(shift);
        for jobs in &mut self.jobs {
            for job in jobs {
                job[2] += first_face;
            }
        }
    }
}

/// Flags used by a material or any of its positional variants.
fn material_flags(materials: &[Material], id: u32) -> u32 {
    let Some(base) = materials.get(id as usize) else {
        return 0;
    };
    let start = base.variation_start as usize;
    let variants = materials
        .get(start..start.saturating_add(base.variation_count as usize))
        .unwrap_or(&[]);
    variants
        .iter()
        .fold(base.flags, |flags, variant| flags | variant.flags)
}

/// Reserves an overlay slot when any positional material variant needs it.
fn overlay_flag(flags: u32) -> u32 {
    if flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0 {
        JOB_OVERLAY_FACE
    } else {
        0
    }
}

/// Output slots of a job with `flags`.
pub(crate) const fn job_slots(flags: u32) -> u32 {
    1 + (flags & JOB_OVERLAY_FACE != 0) as u32 + (flags & JOB_REVERSE_FACE != 0) as u32
}

/// Slots the template quad a model draw ref draws may need, if its addresses are in range: the
/// material's overlay slot, and a back face when the quad itself is two-sided.
fn model_flags(
    tables: &MaterialTables<'_>,
    model_refs: &[PackedModelRef],
    draw: PackedModelDrawRef,
) -> Option<u32> {
    let [model_ref, quad_index] = draw.words();
    let template_id = model_refs.get(model_ref as usize)?.words()[1];
    let template = tables.model_templates.get(template_id as usize)?;
    (quad_index < template.quad_count).then_some(())?;
    let quad = tables
        .model_quads
        .get(template.quad_start.checked_add(quad_index)? as usize)?;
    let overlay = overlay_flag(material_flags(tables.materials, quad.material));
    let reverse = if quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0 {
        JOB_REVERSE_FACE
    } else {
        0
    };
    Some(overlay | reverse)
}

/// One category's jobs by facing group, each job's output face relative to its group.
#[derive(Default)]
struct CategoryJobs {
    groups: [Vec<(JobKind, [u32; 4])>; FACING_GROUPS],
    faces: [u32; FACING_GROUPS],
}

impl CategoryJobs {
    fn push(&mut self, group: usize, kind: JobKind, record: u32, metadata_index: u32, flags: u32) {
        self.groups[group].push((kind, [record, metadata_index, self.faces[group], flags]));
        self.faces[group] += job_slots(flags);
    }
}

/// The axis direction a liquid face faces, when it lies in one axis plane: every side and
/// bottom face, and a top face whose four corners stand at one height (Sodium's aligned
/// fluid quads). A sloped top faces no single axis direction.
fn liquid_facing(liquid: PackedLiquidQuad) -> Option<meshing::Face> {
    let face = liquid.face();
    let heights = liquid.heights();
    (face != meshing::Face::PositiveY || heights.iter().all(|h| *h == heights[0])).then_some(face)
}

/// Plans one section. Overlay materials leave the solid run for the cutout category, because
/// their overlay face is alpha-tested.
pub(crate) fn plan_section(
    section: &SectionGeometry<'_>,
    tables: &MaterialTables<'_>,
) -> SectionPlan {
    let mut categories: [CategoryJobs; 3] = Default::default();
    let metadata = section.metadata_index;
    for (index, quad) in section.cube_quads.iter().enumerate() {
        let material_flags = material_flags(tables.materials, quad.material_id());
        let two_sided = material_flags & assets::MATERIAL_FLAG_TWO_SIDED != 0;
        let flags = overlay_flag(material_flags) | if two_sided { JOB_REVERSE_FACE } else { 0 };
        let category = if (index as u32) < section.solid_len
            && flags & (JOB_OVERLAY_FACE | JOB_REVERSE_FACE) == 0
        {
            GeometryCategory::Solid
        } else {
            GeometryCategory::Cutout
        };
        let group = if two_sided {
            FACING_REST
        } else {
            quad.face() as usize
        };
        for height in 0..u32::from(quad.height()) {
            for width in 0..u32::from(quad.width()) {
                categories[category.index()].push(
                    group,
                    JobKind::Cube,
                    section.quad_base + index as u32,
                    metadata,
                    flags | width | (height << 8),
                );
            }
        }
    }
    for (base, draws, category) in [
        (
            section.model_draw_base,
            section.model_draw_refs,
            GeometryCategory::Cutout,
        ),
        (
            section.transparent_model_draw_base,
            section.transparent_model_draw_refs,
            GeometryCategory::Translucent,
        ),
    ] {
        for (index, draw) in draws.iter().enumerate() {
            let flags = model_flags(tables, section.model_refs, *draw).unwrap_or(0);
            categories[category.index()].push(
                FACING_REST,
                JobKind::Model,
                base + index as u32,
                metadata,
                flags,
            );
        }
    }
    for (index, liquid) in section.liquid_quads.iter().enumerate() {
        let category = if liquid.is_depth_writing() {
            GeometryCategory::Solid
        } else {
            GeometryCategory::Translucent
        };
        let record = section.liquid_base + index as u32;
        let jobs = &mut categories[category.index()];
        match liquid_facing(*liquid) {
            // An axis-aligned face and its back face away from each other: as Sodium's fluid
            // quads, each is culled when the eye is behind its plane.
            Some(face) => {
                jobs.push(face as usize, JobKind::Liquid, record, metadata, 0);
                if liquid.is_two_sided() {
                    let back = face as usize ^ 1;
                    jobs.push(back, JobKind::Liquid, record, metadata, JOB_BACK_ONLY);
                }
            }
            None => {
                let flags = if liquid.is_two_sided() {
                    JOB_REVERSE_FACE
                } else {
                    0
                };
                jobs.push(FACING_REST, JobKind::Liquid, record, metadata, flags);
            }
        }
    }

    let mut plan = SectionPlan::default();
    for (category, planned) in categories.into_iter().enumerate() {
        let first = plan.faces;
        for (group, jobs) in planned.groups.into_iter().enumerate() {
            let start = plan.faces;
            plan.faces += planned.faces[group];
            plan.facing[category][group] = start..plan.faces;
            for (kind, mut job) in jobs {
                job[2] += start;
                plan.jobs[kind as usize].push(job);
            }
        }
        plan.categories[category] = first..plan.faces;
    }
    plan
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
