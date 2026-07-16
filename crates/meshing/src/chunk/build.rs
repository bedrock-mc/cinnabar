/// Greedy-mesh cube/model streams from one sub-chunk and six face neighbours.
///
/// Occupancy is represented as three sets of 16x16 `u64` axis columns. Face
/// masks are calculated with shifts/AND-NOT operations, then coplanar runs of
/// equal face material are merged before emitting one 8-byte record per quad.
/// This legacy API cannot supply the diagonal/vertical 23-slot snapshot needed
/// for correct liquids, so its liquid streams are explicitly empty. Liquid
/// callers must use [`mesh_sub_chunk_in_neighbourhood`].
#[must_use]
pub fn mesh_sub_chunk(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbours: &Neighbourhood<'_>,
    sub_chunk: &SubChunk,
) -> ChunkMesh {
    mesh_sub_chunk_with_lighting(
        classifier,
        visuals,
        network_id_mode,
        neighbours,
        sub_chunk,
        &crate::lighting::FullBrightLightSampler,
    )
}

/// Greedy-mesh with allocation-free solved block/sky light sampling.
#[must_use]
pub fn mesh_sub_chunk_with_lighting<S: crate::lighting::MeshLightSampler + ?Sized>(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbours: &Neighbourhood<'_>,
    sub_chunk: &SubChunk,
    light_sampler: &S,
) -> ChunkMesh {
    let mut neighbourhood = MeshNeighbourhood::new(sub_chunk);
    for face in Face::ALL {
        if let Some(neighbour) = neighbours.get(face) {
            let _ = neighbourhood.insert(face_offset(face), neighbour);
        }
    }
    mesh_sub_chunk_core(
        classifier,
        visuals,
        network_id_mode,
        &neighbourhood,
        light_sampler,
        false,
    )
}

/// Greedy-mesh from the shared bounded 3x3x3 palette-native snapshot.
#[must_use]
pub fn mesh_sub_chunk_in_neighbourhood(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
) -> ChunkMesh {
    mesh_sub_chunk_in_neighbourhood_with_lighting(
        classifier,
        visuals,
        network_id_mode,
        neighbourhood,
        &crate::lighting::FullBrightLightSampler,
    )
}

/// Greedy-mesh the bounded palette snapshot with solved block/sky light.
#[must_use]
pub fn mesh_sub_chunk_in_neighbourhood_with_lighting<
    S: crate::lighting::MeshLightSampler + ?Sized,
>(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    light_sampler: &S,
) -> ChunkMesh {
    mesh_sub_chunk_core(
        classifier,
        visuals,
        network_id_mode,
        neighbourhood,
        light_sampler,
        true,
    )
}

fn mesh_sub_chunk_core<S: crate::lighting::MeshLightSampler + ?Sized>(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    light_sampler: &S,
    include_liquids: bool,
) -> ChunkMesh {
    let sub_chunk = neighbourhood
        .sub_chunk([0, 0, 0])
        .expect("MeshNeighbourhood always contains its center");
    let facts = PaletteFacts::new(*classifier, visuals, network_id_mode, sub_chunk);
    let connectivity = cave_connectivity(&facts);
    if facts.is_air() {
        return ChunkMesh {
            cube_streams: Box::default(),
            model_refs: Box::new([]),
            model_lighting: Box::new([]),
            model_draw_refs: Box::default(),
            liquid_quads: Box::new([]),
            liquid_lighting: Box::new([]),
            connectivity,
        };
    }
    let neighbour_facts: [OnceCell<PaletteFacts<'_>>; Face::ALL.len()] =
        std::array::from_fn(|_| OnceCell::new());
    let palette_context = PaletteResolutionContext {
        classifier: *classifier,
        visuals,
        network_id_mode,
        neighbourhood,
    };

    let masks = VisibilityMasks::from_facts(&facts);
    let mut quads = Vec::new();
    let mut cube_lighting = Vec::new();
    for face in Face::ALL {
        let columns = exposed_columns(
            *classifier,
            visuals,
            network_id_mode,
            neighbourhood,
            face,
            &facts,
            &masks,
        );
        for slice in 0..SIDE {
            let mut rows = [0_u64; SIDE];
            let mut lighting_scratch = [PackedQuadLighting::default(); SIDE * SIDE];
            for (v, row) in rows.iter_mut().enumerate() {
                for (u, column) in columns[v].iter().enumerate() {
                    let visible = (*column >> slice) & 1;
                    *row |= visible << u;
                    if visible != 0 {
                        let coordinate = block_coordinate(face, slice, u, v);
                        lighting_scratch[v * SIDE + u] =
                            crate::lighting::bake_quad_lighting_with_sampler(
                                classifier,
                                visuals,
                                network_id_mode,
                                neighbourhood,
                                light_sampler,
                                coordinate.map(|value| value as i32),
                                face,
                                crate::lighting::cube_face_positions(face),
                            );
                    }
                }
            }
            greedy_slice(
                &facts,
                face,
                slice,
                &mut rows,
                &lighting_scratch,
                &mut quads,
                &mut cube_lighting,
            );
        }
    }
    let mut model_refs = Vec::new();
    let mut model_lighting = Vec::new();
    let mut model_draw_refs = Vec::new();
    let mut transparent_model_draw_refs = Vec::new();
    for x in 0..SIDE {
        for y in 0..SIDE {
            for z in 0..SIDE {
                let entry = facts.at(x, y, z);
                if !matches!(entry.kind, VisualKind::Cross | VisualKind::Model)
                    || entry.model_template == NO_MODEL_TEMPLATE
                {
                    continue;
                }
                let (selected_templates, selected_template_count) = select_model_templates(
                    palette_context,
                    &facts,
                    &neighbour_facts,
                    [x, y, z],
                    entry,
                );
                for &selected_template in
                    &selected_templates[..usize::from(selected_template_count)]
                {
                    let Some(selected) = visuals.model_templates().get(selected_template as usize)
                    else {
                        continue;
                    };
                    let part_count = if selected.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                        2
                    } else {
                        1
                    };
                    for part in 0..part_count {
                        let part_template = selected_template + part;
                        let Some(template) = visuals.model_templates().get(part_template as usize)
                        else {
                            continue;
                        };
                        if template.quad_count == 0 {
                            continue;
                        }
                        let quad_start = template.quad_start as usize;
                        let Some(template_quads) = visuals.model_quads().get(
                            quad_start..quad_start.saturating_add(template.quad_count as usize),
                        ) else {
                            continue;
                        };
                        let mut visible_quad_mask =
                            if template.flags & MODEL_TEMPLATE_FLAG_KELP != 0 {
                                let above = if y + 1 < SIDE {
                                    Some(facts.at(x, y + 1, z))
                                } else {
                                    neighbourhood.sub_chunk([0, 1, 0]).map(|sub_chunk| {
                                        neighbour_facts[Face::PositiveY.index()]
                                            .get_or_init(|| {
                                                PaletteFacts::new(
                                                    *classifier,
                                                    visuals,
                                                    network_id_mode,
                                                    sub_chunk,
                                                )
                                            })
                                            .at(x, 0, z)
                                    })
                                };
                                if above.is_some_and(|entry| is_kelp_entry(visuals, entry)) {
                                    0b00_1111
                                } else {
                                    0b11_0000
                                }
                            } else {
                                match template.quad_count {
                                    0 => 0,
                                    32 => u32::MAX,
                                    count => (1_u32 << count) - 1,
                                }
                            };
                        for (quad_index, quad) in template_quads.iter().enumerate() {
                            let bit = 1_u32 << quad_index;
                            if visible_quad_mask & bit == 0 {
                                continue;
                            }
                            let cull_flags =
                                if template.flags & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0 {
                                    (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4
                                } else {
                                    quad.flags
                                };
                            let Some(cull_face) =
                                model_quad_cull_face(cull_flags, entry.variant & 3)
                            else {
                                continue;
                            };
                            let neighbour = adjacent_palette_entry(
                                palette_context,
                                &facts,
                                &neighbour_facts,
                                [x, y, z],
                                cull_face,
                            );
                            let equal_pane = template.flags & MODEL_TEMPLATE_FLAG_PANE != 0
                                && model_template_flags(visuals, neighbour)
                                    & MODEL_TEMPLATE_FLAG_PANE
                                    != 0
                                && neighbour.faces == entry.faces;
                            let equal_transparent_cube =
                                template.flags & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
                                    && model_template_flags(visuals, neighbour)
                                        & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
                                        != 0
                                    && neighbour.network_value == entry.network_value;
                            if neighbour.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
                                || equal_pane
                                || equal_transparent_cube
                            {
                                visible_quad_mask &= !bit;
                            }
                        }
                        if visible_quad_mask == 0 {
                            continue;
                        }
                        let Ok(lighting_base_index) = u32::try_from(model_lighting.len()) else {
                            continue;
                        };
                        let Some(lighting) = crate::lighting::bake_template_lighting_with_sampler(
                            classifier,
                            visuals,
                            network_id_mode,
                            neighbourhood,
                            light_sampler,
                            [x as i32, y as i32, z as i32],
                            part_template,
                            entry.variant & 3,
                        ) else {
                            continue;
                        };
                        let Ok(model_ref_index) = u32::try_from(model_refs.len()) else {
                            continue;
                        };
                        model_refs.push(PackedModelRef::new(
                            pack_model_transform(
                                [x as u8, y as u8, z as u8],
                                if entry.kind == VisualKind::Cross {
                                    0
                                } else {
                                    entry.variant
                                },
                            ),
                            part_template,
                            lighting_base_index,
                            visible_quad_mask,
                        ));
                        model_lighting.extend(lighting);
                        let mut remaining = visible_quad_mask;
                        while remaining != 0 {
                            let quad_index = remaining.trailing_zeros();
                            let draw_ref = PackedModelDrawRef::new(model_ref_index, quad_index);
                            if template_quads[quad_index as usize].material != DIAGNOSTIC_MATERIAL
                                && visuals
                                    .material(template_quads[quad_index as usize].material)
                                    .flags
                                    & MATERIAL_FLAG_ALPHA_BLEND
                                    != 0
                            {
                                transparent_model_draw_refs.push(draw_ref);
                            } else {
                                model_draw_refs.push(draw_ref);
                            }
                            remaining &= remaining - 1;
                        }
                    }
                }
            }
        }
    }

    let (liquid_quads, liquid_lighting) = if include_liquids {
        mesh_liquids(
            *classifier,
            visuals,
            network_id_mode,
            neighbourhood,
            light_sampler,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    ChunkMesh {
        cube_streams: Box::new(CubeStreams {
            cube_quads: quads.into_boxed_slice(),
            cube_lighting: cube_lighting.into_boxed_slice(),
        }),
        model_refs: model_refs.into_boxed_slice(),
        model_lighting: model_lighting.into_boxed_slice(),
        model_draw_refs: Box::new(ModelDrawRefs {
            opaque: model_draw_refs.into_boxed_slice(),
            transparent: transparent_model_draw_refs.into_boxed_slice(),
        }),
        liquid_quads: liquid_quads.into_boxed_slice(),
        liquid_lighting: liquid_lighting.into_boxed_slice(),
        connectivity,
    }
}
use std::cell::OnceCell;

use assets::{
    BlockFlags, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND, MODEL_QUAD_FLAG_FACE_MASK,
    MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE,
    MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets,
    VisualKind,
};
use world::{MeshNeighbourhood, SubChunk};

use super::{
    liquids::mesh_liquids,
    models::{
        PaletteResolutionContext, adjacent_palette_entry, is_kelp_entry, model_quad_cull_face,
        model_template_flags, select_model_templates,
    },
    opaque::{VisibilityMasks, block_coordinate, exposed_columns, face_offset, greedy_slice},
};
use crate::{
    BlockClassifier, ChunkMesh, Face, Neighbourhood, PackedModelDrawRef, PackedModelRef,
    PackedQuadLighting, SIDE,
    connectivity::cave_connectivity,
    contributors::{PaletteFacts, pack_model_transform},
    types::{CubeStreams, ModelDrawRefs},
};
