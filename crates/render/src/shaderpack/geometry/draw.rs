//! Per-view draw lists over resident expanded sections, retained while what they were built
//! from holds, and recording them into a pass.

use std::hash::{BuildHasher, Hash, Hasher};
use std::ops::Range;

use crate::chunk::transparent::{face_metric::TransparentFaceMetric, mixed::plan::sort_faces};
use bevy::math::DVec3;
use bevy::platform::hash::FixedHasher;
use bevy::render::render_phase::TrackedRenderPass;
use bevy::render::render_resource::{Buffer, BufferDescriptor, BufferUsages, IndexFormat};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use wgpu::util::DrawIndexedIndirectArgs;
use world::SubChunkKey;

use super::jobs::{FACING_GROUPS, FACING_REST, GeometryCategory};
use super::residency::{ResidentFaces, section_distance};
use super::vertex::{CONSTANT_VERTEX_BUFFER, DRAW_ORIGIN_VERTEX_BUFFER, GEOMETRY_VERTEX_BUFFER};

/// Indices of one quad face, counter-clockwise like its corners.
pub(crate) const FACE_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
/// Edge of a section, in blocks.
const SECTION_SIZE: f64 = 16.0;

/// Consecutive commands of one category that read the same vertex page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DrawRun {
    pub(crate) page: usize,
    pub(crate) commands: Range<u32>,
}

/// One view's draw commands: every category's commands back to back, split into page runs.
#[derive(Default)]
pub(crate) struct ViewDraws {
    pub(crate) commands: Vec<DrawIndexedIndirectArgs>,
    pub(crate) runs: [Vec<DrawRun>; 3],
    /// The commands as indirect arguments, when the device draws terrain through
    /// multi-draw-indirect; kept across rebuilds and grown when they outgrow it.
    indirect: Option<Buffer>,
    translucent_indices: Vec<u32>,
    translucent_buffer: Option<Buffer>,
    /// Fingerprint of the inputs the lists were built from.
    key: Option<u64>,
}

/// The facing groups of the section at block `origin` whose faces `eye` may see from the
/// front: an axis-facing face lies on a plane within the section's extent and faces the eye
/// only from that plane's front side, as Sodium's block face culling decides.
fn facing_visible(origin: [i32; 3], eye: DVec3) -> [bool; FACING_GROUPS] {
    let min = DVec3::new(
        f64::from(origin[0]),
        f64::from(origin[1]),
        f64::from(origin[2]),
    );
    let max = min + DVec3::splat(SECTION_SIZE);
    let mut visible = [true; FACING_GROUPS];
    for axis in 0..3 {
        // `meshing::Face` order: the negative, then the positive direction of each axis.
        visible[axis * 2] = eye[axis] <= max[axis];
        visible[axis * 2 + 1] = eye[axis] >= min[axis];
    }
    visible[FACING_REST] = true;
    visible
}

/// Centre of the section the eye is in. Like Sodium, which orders sections by its graph
/// walk from the camera's section and culls them by render distance from it, lists that
/// depend on the camera only through this point stay valid while it moves within a section.
pub(crate) fn eye_section_centre(eye: DVec3) -> DVec3 {
    (eye / SECTION_SIZE).floor() * SECTION_SIZE + DVec3::splat(SECTION_SIZE / 2.0)
}

/// The sections one view's lists are built from and the camera they are seen from, reduced
/// to what the lists depend on: [`Self::key`] changes exactly when [`ViewDraws::build`]
/// would list something else, so a moving camera rebuilds only when the sections it lists,
/// its native transparent-face ordering class or the facing groups it sees change.
pub(crate) struct DrawInputs<'a> {
    /// Current-epoch sections, in the order given.
    sections: Vec<&'a ResidentFaces>,
    /// [`eye_section_centre`] of the eye translucent sections are ordered from.
    eye: Option<DVec3>,
    /// Camera position used by the native within-section face metric.
    face_eye: Option<DVec3>,
    /// Camera position solid and cutout facing groups are culled for.
    facing: Option<DVec3>,
}

impl<'a> DrawInputs<'a> {
    /// `sections` for one view: translucent sections far to near from the section `eye` is
    /// in, with faces sorted by the native metric (in page order without an eye); with
    /// `facing`, the camera position, solid and cutout faces it can only see from behind are
    /// left out. Faces from other origin epochs or expansion generations are skipped.
    pub(crate) fn new(
        sections: impl Iterator<Item = &'a ResidentFaces>,
        eye: Option<DVec3>,
        epoch: u64,
        generation: u64,
        facing: Option<DVec3>,
    ) -> Self {
        Self {
            sections: sections
                .filter(|section| section.epoch == epoch && section.generation == generation)
                .collect(),
            eye: eye.map(eye_section_centre),
            face_eye: eye,
            facing,
        }
    }

    /// Fingerprint of the lists these inputs build.
    pub(crate) fn key(&self) -> u64 {
        let mut hasher = FixedHasher.build_hasher();
        self.sections.len().hash(&mut hasher);
        for section in &self.sections {
            section.stamp.hash(&mut hasher);
            if !section.translucent.is_empty()
                && let Some(eye) = self.face_eye
            {
                TransparentFaceMetric::new(eye.as_vec3())
                    .class(section_key(section))
                    .hash(&mut hasher);
            }
        }
        self.eye
            .map(|eye| eye.to_array().map(f64::to_bits))
            .hash(&mut hasher);
        // `facing_visible` compares the eye with section bounds, which lie on the 16-block
        // grid, so the grid cells the eye lies on decide every comparison.
        self.facing
            .map(|eye| {
                (eye / SECTION_SIZE)
                    .to_array()
                    .map(|cell| (cell.floor() as i64, cell.ceil() as i64))
            })
            .hash(&mut hasher);
        hasher.finish()
    }
}

/// Native section key for the block origin retained with expanded faces.
fn section_key(section: &ResidentFaces) -> SubChunkKey {
    let [x, y, z] = section.origin.map(|axis| {
        axis.div_euclid(render_scene::chunk::chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0])
    });
    SubChunkKey::new(0, x, y, z)
}

impl ViewDraws {
    /// The lists `inputs` describe: solid and cutout page by page (one vertex buffer bind per
    /// page), each page's sections near to far from the eye like Sodium's, so early depth
    /// testing skips the pack's fragment shading of hidden faces; translucent far to near.
    /// Each command's first instance is its section's draw-origin slot.
    pub(crate) fn build(inputs: &DrawInputs<'_>) -> Self {
        let mut by_page = inputs.sections.clone();
        by_page.sort_unstable_by_key(|section| (section.page, section.faces.start));
        let mut opaque = by_page.clone();
        if let Some(eye) = inputs.eye {
            let mut keyed: Vec<_> = opaque
                .iter()
                .map(|section| {
                    (
                        section.page,
                        section_distance(section.origin, eye),
                        *section,
                    )
                })
                .collect();
            // Stable over page order, so sections as far as each other keep one order.
            keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            opaque = keyed.into_iter().map(|(.., section)| section).collect();
        }
        let translucent = inputs.eye.map(|eye| {
            let mut ordered: Vec<_> = by_page
                .iter()
                .filter(|section| {
                    !section.categories[GeometryCategory::Translucent.index()].is_empty()
                })
                .map(|section| (section_distance(section.origin, eye), *section))
                .collect();
            // Stable over page order, so sections as far as each other keep one order.
            ordered.sort_by(|a, b| b.0.total_cmp(&a.0));
            ordered
                .into_iter()
                .map(|(_, section)| section)
                .collect::<Vec<_>>()
        });
        let mut draws = Self::default();
        let sorted_translucent = inputs.face_eye.is_some()
            && inputs
                .sections
                .iter()
                .any(|section| !section.translucent.is_empty());
        for category in GeometryCategory::ALL {
            let ordered = match (category, &translucent) {
                (GeometryCategory::Translucent, Some(ordered)) => ordered,
                _ => &opaque,
            };
            let facing = inputs.facing;
            for section in ordered {
                if section.categories[category.index()].is_empty() {
                    continue;
                }
                if category == GeometryCategory::Translucent
                    && sorted_translucent
                    && let Some(eye) = inputs.face_eye
                {
                    let mut faces = section.translucent.clone();
                    if faces.is_empty() {
                        use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream};
                        faces.extend(section.categories[category.index()].clone().map(|index| {
                            MixedFace {
                                index,
                                stream: MixedStream::Model,
                                stable: [index, 0],
                                centroid: bevy::math::Vec3::from_array(
                                    section.origin.map(|axis| axis as f32),
                                ),
                            }
                        }));
                    }

                    sort_faces(section_key(section), eye.as_vec3(), &mut faces);
                    let first = draws.translucent_indices.len() as u32;
                    for face in &faces {
                        draws
                            .translucent_indices
                            .extend(FACE_INDICES.map(|index| face.index * 4 + index));
                    }
                    let command = draws.commands.len();
                    draws.push(category, section, 0..faces.len() as u32);
                    draws.commands[command].first_index = first;
                    continue;
                }
                let groups = &section.facing[category.index()];
                let visible = facing.map_or([true; FACING_GROUPS], |eye| {
                    facing_visible(section.origin, eye)
                });
                // Visible groups that follow one another merge into one command. A
                // translucent section is one command, as Sodium draws it: the groups it
                // spans but the eye cannot see face away and the culling rasterizer drops
                // them, which costs less than a draw each.
                let bridge = category == GeometryCategory::Translucent;
                let mut pending: Option<Range<u32>> = None;
                for (group, faces) in groups.iter().enumerate() {
                    if !visible[group] || faces.is_empty() {
                        continue;
                    }
                    match &mut pending {
                        Some(open) if bridge || open.end == faces.start => open.end = faces.end,
                        _ => {
                            if let Some(done) = pending.replace(faces.clone()) {
                                draws.push(category, section, done);
                            }
                        }
                    }
                }
                if let Some(done) = pending {
                    draws.push(category, section, done);
                }
            }
        }
        draws
    }

    fn push(&mut self, category: GeometryCategory, section: &ResidentFaces, faces: Range<u32>) {
        let page = section.page;
        let command = self.commands.len() as u32;
        let count = faces.end - faces.start;
        self.commands.push(DrawIndexedIndirectArgs {
            index_count: count * FACE_INDICES.len() as u32,
            instance_count: 1,
            first_index: 0,
            base_vertex: (faces.start * 4) as i32,
            first_instance: section.slot,
        });
        let runs = &mut self.runs[category.index()];
        match runs.last_mut() {
            Some(run) if run.page == page && run.commands.end == command => {
                run.commands.end += 1;
            }
            _ => runs.push(DrawRun {
                page,
                commands: command..command + 1,
            }),
        }
    }

    /// Whether the lists were built from inputs with fingerprint `key`.
    pub(crate) fn is_current(&self, key: u64) -> bool {
        self.key == Some(key)
    }

    /// Takes `built`'s lists as the ones for inputs `key`, writing them into the retained
    /// indirect buffer (grown to fit) when the device draws through multi-draw-indirect.
    pub(crate) fn replace(
        &mut self,
        built: ViewDraws,
        key: u64,
        device: &RenderDevice,
        queue: &RenderQueue,
        multi_draw: bool,
    ) {
        self.commands = built.commands;
        self.runs = built.runs;
        self.translucent_indices = built.translucent_indices;
        self.key = Some(key);
        if !self.translucent_indices.is_empty() {
            let contents = bytemuck::cast_slice(&self.translucent_indices);
            if self
                .translucent_buffer
                .as_ref()
                .is_none_or(|buffer| buffer.size() < contents.len() as u64)
            {
                self.translucent_buffer = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("shader-pack sorted translucent indices"),
                    size: contents.len() as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            queue.write_buffer(
                self.translucent_buffer.as_ref().expect("created above"),
                0,
                contents,
            );
        }
        if !multi_draw {
            self.indirect = None;
            return;
        }
        if self.commands.is_empty() {
            return;
        }
        let bytes: Vec<u8> = self
            .commands
            .iter()
            .flat_map(|command| command.as_bytes())
            .copied()
            .collect();
        let size = bytes.len() as u64;
        if self
            .indirect
            .as_ref()
            .is_none_or(|buffer| buffer.size() < size)
        {
            self.indirect = Some(device.create_buffer(&BufferDescriptor {
                label: Some("shader-pack terrain draws"),
                size: size.next_power_of_two(),
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.indirect {
            queue.write_buffer(buffer, 0, &bytes);
        }
    }

    /// Largest command in faces, which the shared index buffer must cover.
    pub(crate) fn max_faces(&self) -> u32 {
        self.commands
            .iter()
            .map(|command| command.index_count / FACE_INDICES.len() as u32)
            .max()
            .unwrap_or(0)
    }

    /// Records `category` into `pass`, which must already have the program's pipeline and bind
    /// groups set; `origins` is the draw-origin table packed layouts read. Returns the draws
    /// issued.
    pub(crate) fn record<'w>(
        &'w self,
        pass: &mut TrackedRenderPass<'w>,
        category: GeometryCategory,
        pages: &'w [Buffer],
        index_buffer: &'w Buffer,
        constants: &'w Buffer,
        origins: Option<&'w Buffer>,
    ) -> u32 {
        let runs = &self.runs[category.index()];
        if runs.is_empty() {
            return 0;
        }
        let index_buffer =
            if category == GeometryCategory::Translucent && !self.translucent_indices.is_empty() {
                self.translucent_buffer
                    .as_ref()
                    .expect("uploaded with draw lists")
            } else {
                index_buffer
            };
        pass.set_index_buffer(index_buffer.slice(..), IndexFormat::Uint32);
        pass.set_vertex_buffer(CONSTANT_VERTEX_BUFFER as usize, constants.slice(..));
        if let Some(origins) = origins {
            pass.set_vertex_buffer(DRAW_ORIGIN_VERTEX_BUFFER as usize, origins.slice(..));
        }
        let stride = std::mem::size_of::<DrawIndexedIndirectArgs>() as u64;
        for run in runs {
            pass.set_vertex_buffer(GEOMETRY_VERTEX_BUFFER as usize, pages[run.page].slice(..));
            match &self.indirect {
                Some(indirect) => pass.multi_draw_indexed_indirect(
                    indirect,
                    u64::from(run.commands.start) * stride,
                    run.commands.end - run.commands.start,
                ),
                None => {
                    for command in
                        &self.commands[run.commands.start as usize..run.commands.end as usize]
                    {
                        pass.draw_indexed(
                            command.first_index..command.first_index + command.index_count,
                            command.base_vertex,
                            command.first_instance..command.first_instance + command.instance_count,
                        );
                    }
                }
            }
        }
        runs.iter()
            .map(|run| run.commands.end - run.commands.start)
            .sum()
    }
}

/// Index buffer words for `faces` consecutive faces.
pub(crate) fn face_indices(faces: u32) -> Vec<u32> {
    (0..faces)
        .flat_map(|face| FACE_INDICES.map(|index| face * 4 + index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest(range: Range<u32>) -> [Range<u32>; FACING_GROUPS] {
        let mut groups: [Range<u32>; FACING_GROUPS] =
            std::array::from_fn(|_| range.start..range.start);
        groups[FACING_REST] = range;
        groups
    }

    fn section(
        page: usize,
        faces: Range<u32>,
        translucent: Range<u32>,
        origin: [i32; 3],
    ) -> ResidentFaces {
        let categories = [
            faces.start..translucent.start,
            translucent.start..translucent.start,
            translucent,
        ];
        ResidentFaces {
            translucent: Vec::new(),
            page,
            faces: faces.clone(),
            facing: categories.clone().map(rest),
            categories,
            origin,
            epoch: 1,
            generation: 0,
            stamp: 0,
            slot: origin[0] as u32,
        }
    }

    #[test]
    fn translucent_sections_draw_far_to_near_and_stale_epochs_are_skipped() {
        let near = section(0, 0..4, 2..4, [0, 0, 0]);
        let far = section(0, 4..8, 6..8, [64, 0, 0]);
        let mut stale = section(1, 0..2, 2..2, [0, 16, 0]);
        stale.epoch = 0;
        let draws = ViewDraws::build(&DrawInputs::new(
            [&near, &far, &stale].into_iter(),
            Some(DVec3::ZERO),
            1,
            0,
            None,
        ));
        let translucent: Vec<_> = draws.runs[GeometryCategory::Translucent.index()]
            .iter()
            .flat_map(|run| run.commands.clone())
            .map(|command| draws.commands[command as usize].base_vertex)
            .collect();
        assert_eq!(translucent, [6 * 4, 2 * 4], "far section first");
        let solid = &draws.runs[GeometryCategory::Solid.index()];
        assert_eq!(solid.len(), 1, "both solid ranges share page 0 in one run");
        assert_eq!(solid[0].commands.len(), 2);
        assert_eq!(draws.max_faces(), 2);
        // Each command selects its section's draw origin (the fixture's slot is origin x).
        let first_instances: Vec<_> = draws.commands.iter().map(|c| c.first_instance).collect();
        assert_eq!(first_instances, [0, 64, 64, 0]);
    }

    #[test]
    fn faces_seen_only_from_behind_are_left_out_and_visible_groups_merge() {
        // Solid faces 0..12, two per facing group in `meshing::Face` order, none in the rest.
        let mut section = section(0, 0..12, 12..12, [0, 0, 0]);
        section.facing[0] = std::array::from_fn(|group| {
            let start = (group as u32 * 2).min(12);
            start..(start + 2).min(12)
        });
        // Above and beyond +X and +Z of the section: -X, -Y and -Z faces face away.
        let eye = DVec3::new(40.0, 40.0, 40.0);
        let draws = ViewDraws::build(&DrawInputs::new(
            [&section].into_iter(),
            Some(eye),
            1,
            0,
            Some(eye),
        ));
        let solid: Vec<_> = draws
            .commands
            .iter()
            .map(|c| (c.base_vertex / 4, c.index_count / 6))
            .collect();
        assert_eq!(
            solid,
            [(2, 2), (6, 2), (10, 2)],
            "+X, +Y and +Z groups only"
        );
        // Inside the section every group may face the eye: one merged command.
        let inside = DVec3::splat(8.0);
        let draws = ViewDraws::build(&DrawInputs::new(
            [&section].into_iter(),
            Some(inside),
            1,
            0,
            Some(inside),
        ));
        assert_eq!(draws.commands.len(), 1);
        assert_eq!(draws.max_faces(), 12);
    }

    #[test]
    fn a_translucent_section_is_one_command_spanning_its_visible_groups() {
        // Translucent faces 0..14, two per facing group, the last two sloped (rest).
        let mut section = section(0, 0..14, 0..14, [0, 0, 0]);
        section.facing[GeometryCategory::Translucent.index()] = std::array::from_fn(|group| {
            let start = group as u32 * 2;
            start..start + 2
        });
        // Above and beyond +X and +Z: -X, -Y and -Z face away.
        let eye = DVec3::new(40.0, 40.0, 40.0);
        let draws = ViewDraws::build(&DrawInputs::new(
            [&section].into_iter(),
            Some(eye),
            1,
            0,
            Some(eye),
        ));
        let commands: Vec<_> = draws
            .commands
            .iter()
            .map(|c| (c.base_vertex / 4, c.index_count / 6))
            .collect();
        assert_eq!(
            commands,
            [(2, 12)],
            "+X through the rest group, in one draw"
        );
        // Straight above, only the down faces face away; the one command spans them too.
        let above = DVec3::new(8.0, 40.0, 8.0);
        let draws = ViewDraws::build(&DrawInputs::new(
            [&section].into_iter(),
            Some(above),
            1,
            0,
            Some(above),
        ));
        let commands: Vec<_> = draws
            .commands
            .iter()
            .map(|c| (c.base_vertex / 4, c.index_count / 6))
            .collect();
        assert_eq!(commands, [(0, 14)]);
    }

    #[test]
    fn a_moving_camera_keeps_its_key_exactly_while_the_lists_stay_the_same() {
        // A 3×3×3 block of sections, each with two solid faces per facing group and two
        // translucent faces.
        let sections: Vec<_> = (0..27_u32)
            .map(|index| {
                let at = |step: u32| (step as i32 - 1) * 16;
                let origin = [at(index % 3), at(index / 3 % 3), at(index / 9)];
                let first = index * 14;
                let mut section = section(0, first..first + 14, first + 12..first + 14, origin);
                section.facing[0] = std::array::from_fn(|group| {
                    let start = (first + group as u32 * 2).min(first + 12);
                    start..(start + 2).min(first + 12)
                });
                section.stamp = u64::from(index);
                section
            })
            .collect();
        let listed = |eye: DVec3| {
            let inputs = DrawInputs::new(sections.iter(), Some(eye), 1, 0, Some(eye));
            let commands: Vec<_> = ViewDraws::build(&inputs)
                .commands
                .iter()
                .map(|command| (command.base_vertex, command.index_count))
                .collect();
            (inputs.key(), commands)
        };
        let mut previous = listed(DVec3::new(-20.0, 3.0, 5.0));
        let mut rebuilds = 0;
        for step in 1..400 {
            let step = f64::from(step);
            let next = listed(DVec3::new(-20.0 + step * 0.1, 3.0 + step * 0.05, 5.0));
            if next.0 == previous.0 {
                assert_eq!(next.1, previous.1, "an unchanged key keeps the lists");
            } else {
                rebuilds += 1;
            }
            previous = next;
        }
        // The eye crosses four section boundaries (x = -16, 0, 16 and y = 16).
        assert!(rebuilds <= 8, "rebuilt {rebuilds} times over 400 frames");
        // A section placed again (a new stamp) changes the key even at the same eye.
        let eye = DVec3::splat(4.0);
        let key = DrawInputs::new(sections.iter(), Some(eye), 1, 0, Some(eye)).key();
        let mut replaced = sections.clone();
        replaced[13].stamp = 100;
        assert_ne!(
            DrawInputs::new(replaced.iter(), Some(eye), 1, 0, Some(eye)).key(),
            key
        );
    }

    #[test]
    fn face_indices_form_two_counter_clockwise_triangles_per_face() {
        assert_eq!(face_indices(2), [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
    }
    #[test]
    fn stale_atlas_faces_stay_hidden_across_packed_origin_rebases() {
        use super::super::jobs::SectionPlan;
        use super::super::residency::Residency;
        let entity = bevy::prelude::Entity::from_raw_u32(1).unwrap();
        let mut residency = Residency::new(1024, 1024, 28);
        let plan = SectionPlan {
            faces: 1,
            categories: [0..1, 1..1, 1..1],
            facing: [rest(0..1), rest(1..1), rest(1..1)],
            ..Default::default()
        };
        residency.follow_camera(DVec3::ZERO);
        let placement = residency.place(entity, &plan).unwrap();
        residency.commit(entity, &placement, &plan, [0; 3]);
        residency.invalidate_contents([entity]);
        residency.follow_camera(DVec3::new(6000.0, 0.0, 0.0));
        residency.keep_faces_across_rebase();
        let draws = || {
            ViewDraws::build(&DrawInputs::new(
                residency.sections.values(),
                None,
                residency.epoch,
                residency.generation,
                None,
            ))
        };
        assert!(draws().commands.is_empty());
        let placement = residency.place(entity, &plan).unwrap();
        residency.commit(entity, &placement, &plan, [0; 3]);
        let current = ViewDraws::build(&DrawInputs::new(
            residency.sections.values(),
            None,
            residency.epoch,
            residency.generation,
            None,
        ));
        assert_eq!(current.commands.len(), 1);
    }
    /// The effective face sequence selected by one category's commands.
    fn translucent_order(draws: &ViewDraws) -> Vec<u32> {
        draws.runs[GeometryCategory::Translucent.index()]
            .iter()
            .flat_map(|run| run.commands.clone())
            .flat_map(|index| {
                let c = &draws.commands[index as usize];
                (0..c.index_count / 6).map(move |face| {
                    if draws.translucent_indices.is_empty() {
                        c.base_vertex as u32 / 4 + face
                    } else {
                        draws.translucent_indices[(c.first_index + face * 6) as usize] / 4
                    }
                })
            })
            .collect()
    }

    #[test]
    fn translucent_faces_mix_models_and_water_per_view_inside_one_section() {
        use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream};
        let mut mesh = section(0, 0..3, 0..3, [0; 3]);
        mesh.translucent = [
            (MixedStream::Model, 0, 1.0),
            (MixedStream::Water, 1, 12.0),
            (MixedStream::Model, 2, 6.0),
        ]
        .into_iter()
        .map(|(stream, index, x)| MixedFace {
            stream,
            index,
            centroid: bevy::math::Vec3::new(x, 1.0, 1.0),
            stable: [index, 0],
        })
        .collect();
        let build = |x| {
            ViewDraws::build(&DrawInputs::new(
                [&mesh].into_iter(),
                Some(DVec3::new(x, 1.0, 1.0)),
                1,
                0,
                None,
            ))
        };
        assert_eq!(translucent_order(&build(0.0)), [1, 2, 0]);
        assert_eq!(translucent_order(&build(15.0)), [0, 2, 1]);
        let left = DrawInputs::new(
            [&mesh].into_iter(),
            Some(DVec3::new(0.0, 1.0, 1.0)),
            1,
            0,
            None,
        );
        let right = DrawInputs::new(
            [&mesh].into_iter(),
            Some(DVec3::new(15.0, 1.0, 1.0)),
            1,
            0,
            None,
        );
        assert_ne!(left.key(), right.key());
    }
    #[test]
    fn sorted_translucent_indices_stay_relative_to_each_vertex_page() {
        use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream};
        let mut near = section(0, 20..22, 20..22, [0; 3]);
        let mut far = section(1, 4..6, 4..6, [64, 0, 0]);
        for mesh in [&mut near, &mut far] {
            mesh.translucent = mesh
                .faces
                .clone()
                .map(|index| MixedFace {
                    stream: MixedStream::Model,
                    index,
                    stable: [index, 0],
                    centroid: bevy::math::Vec3::from_array(mesh.origin.map(|v| v as f32)),
                })
                .collect();
        }
        let draws = ViewDraws::build(&DrawInputs::new(
            [&near, &far].into_iter(),
            Some(DVec3::ZERO),
            1,
            0,
            None,
        ));
        assert_eq!(translucent_order(&draws), [4, 5, 20, 21]);
        assert_eq!(
            draws.runs[2].iter().map(|run| run.page).collect::<Vec<_>>(),
            [1, 0]
        );
        assert!(
            draws
                .commands
                .iter()
                .all(|command| command.base_vertex == 0)
        );
    }
}
