//! Adjacent actor draws retain exact geometry, order and receipt coverage.

use super::super::artwork::draw_spans;
use crate::actor::gpu::ActorDrawSpan;
use render_model::{ActorGpuInstance, ActorRigGeometrySpan};

#[test]
fn adjacent_distinct_equal_count_opaque_rigs_share_one_draw() {
    let geometry: Vec<_> = (0..8)
        .map(|index| ActorRigGeometrySpan {
            first_vertex: index * 36,
            vertex_count: 36,
        })
        .collect();
    let instances: Vec<_> = [7, 0, 5, 2, 6, 1, 4, 3]
        .map(|geometry_id| ActorGpuInstance {
            geometry_id,
            ..Default::default()
        })
        .into();
    let mut spans = Vec::new();
    draw_spans(
        &mut spans,
        &[1; 8],
        &instances,
        &geometry,
        std::iter::repeat(0),
    );
    assert_eq!(
        spans,
        [ActorDrawSpan {
            material: 0,
            page: 1,
            first: 0,
            count: instances.len() as u32,
            vertex_count: 36,
        }]
    );
    let selected: Vec<_> = spans
        .iter()
        .flat_map(|span| span.first..span.first + span.count)
        .map(|index| {
            let instance = &instances[index as usize];
            geometry[instance.geometry_id as usize].first_vertex
        })
        .collect();
    assert_eq!(selected, [252, 0, 180, 72, 216, 36, 144, 108]);
}

#[test]
fn matching_opaque_rigs_keep_layer_boundaries() {
    let instances = [ActorGpuInstance::default(); 4];
    let mut spans = Vec::new();
    draw_spans(
        &mut spans,
        &[1; 4],
        &instances,
        &[ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 36,
        }],
        [0, 0, 1, 1],
    );
    assert_eq!(
        spans
            .iter()
            .map(|span| (span.first, span.count))
            .collect::<Vec<_>>(),
        [(0, 2), (2, 2)]
    );
}

#[test]
fn merged_opaque_rigs_encode_one_span_and_receipt_for_every_instance() {
    use super::super::ActorGpu;
    use super::allocation_fault_tests::{draw_frame, fixture, publication};
    use bevy::render::RenderApp;
    use std::sync::Arc;

    let mut app = fixture();
    let mut frame = publication(1, &[55; 8], 1);
    frame.instance_pages = Arc::from([1; 8]);
    frame.rig.geometry_spans = (0..8)
        .map(|index| ActorRigGeometrySpan {
            first_vertex: index * 3,
            vertex_count: 3,
        })
        .collect::<Vec<_>>()
        .into();
    frame.rig.geometry_vertices = render_model::ActorRigVertexSegments::from_vertices(
        [render_model::ActorRigVertex::default(); 24],
    );
    frame.rig.maximum_vertex_count = 3;
    for (index, instance) in Arc::make_mut(&mut frame.rig.instances)
        .iter_mut()
        .enumerate()
    {
        instance.geometry_id = 7 - index as u32;
    }
    let drawn = draw_frame(&mut app, &frame);
    assert_eq!(drawn.len(), 1);
    assert_eq!(
        (drawn[0].first, drawn[0].count, drawn[0].vertex_count),
        (0, 8, 3)
    );
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    assert_eq!(
        gpu.executed_instances
            .load(std::sync::atomic::Ordering::Relaxed),
        8
    );
    assert_eq!(
        gpu.manifest
            .iter()
            .map(|entry| entry.identity)
            .collect::<Vec<_>>(),
        frame
            .rig
            .manifest
            .iter()
            .map(|entry| entry.identity)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        gpu.instances
            .iter()
            .map(|instance| instance.geometry_id)
            .collect::<Vec<_>>(),
        [7, 6, 5, 4, 3, 2, 1, 0]
    );
}

#[test]
fn incompatible_or_sorted_instances_split_adjacent_opaque_runs_without_reordering() {
    let opaque = assets::EntityRenderMaterial::Default as u32;
    let other = assets::EntityRenderMaterial::Dragon as u32;
    let blended =
        assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
            blend: true,
            ..Default::default()
        }));
    let always =
        assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
            depth_always: true,
            ..Default::default()
        }));
    let late = assets::EntityRenderMaterial::DissolveColor as u32
        | crate::actor::material::LATE_DISSOLVE_COLOR;
    let geometry = [36, 36, 72].map(|vertex_count| ActorRigGeometrySpan {
        first_vertex: 0,
        vertex_count,
    });
    for sorted in [blended, always, late] {
        let instances = [
            (0, opaque),
            (1, opaque),
            (2, opaque),
            (0, opaque),
            (1, opaque),
            (0, other),
            (1, other),
            (0, sorted),
            (1, sorted),
            (0, other),
            (1, other),
        ]
        .map(|(geometry_id, material)| ActorGpuInstance {
            geometry_id,
            material,
            ..Default::default()
        });
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &[1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2],
            &instances,
            &geometry,
            std::iter::repeat(0),
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| (
                    span.page,
                    span.first,
                    span.count,
                    span.vertex_count,
                    span.material
                ))
                .collect::<Vec<_>>(),
            [
                (1, 0, 2, 36, opaque),
                (1, 2, 1, 72, opaque),
                (1, 3, 1, 36, opaque),
                (2, 4, 1, 36, opaque),
                (2, 5, 2, 36, other),
                (2, 7, 1, 36, sorted),
                (2, 8, 1, 36, sorted),
                (2, 9, 2, 36, other),
            ]
        );
        assert_eq!(
            spans
                .iter()
                .flat_map(|span| span.first..span.first + span.count)
                .collect::<Vec<_>>(),
            (0..instances.len() as u32).collect::<Vec<_>>()
        );
    }
}

#[test]
fn merged_distinct_rigs_match_separate_draw_pixels() {
    use crate::{actor_raster, gpu_snapshot};
    use bevy::math::{Mat4, Vec3, Vec4};

    let Some(gpu) =
        gpu_snapshot::Gpu::for_fixture("merged_distinct_rigs_match_separate_draw_pixels")
    else {
        return;
    };
    let mut vertices = actor_raster::cube([8, 0, 16], true, false);
    let count = vertices.len() as u32;
    for vertex in &mut vertices {
        vertex.uv = [0.25, 0.5];
    }
    let mut second = actor_raster::cube([8, 0, 8], true, false);
    assert_eq!(second.len(), vertices.len());
    for vertex in &mut second {
        vertex.position[0] += 0.25;
        vertex.uv = [0.75, 0.5];
    }
    vertices.extend(second);
    let geometry = [0, count].map(|first_vertex| ActorRigGeometrySpan {
        first_vertex,
        vertex_count: count,
    });
    let world_from_actor = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let instances = [0, 1].map(|geometry_id| ActorGpuInstance {
        geometry_id,
        world_from_actor,
        partial_tick: 1.0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        multitexture_layers: [u32::MAX; 2],
        ..Default::default()
    });
    let mut spans = Vec::new();
    draw_spans(
        &mut spans,
        &[1; 2],
        &instances,
        &geometry,
        std::iter::repeat(0),
    );
    assert_eq!(spans.len(), 1);
    let merged: Vec<_> = spans
        .iter()
        .map(|span| (0..span.vertex_count, span.first..span.first + span.count))
        .collect();
    let raster = |draws: &[_]| {
        actor_raster::raster_batch(
            &gpu,
            actor_raster::RasterBatch {
                vertices: &vertices,
                geometry: &geometry,
                instances: &instances,
                draws,
            },
            actor_raster::RasterScene {
                clip_from_world: Mat4::from_cols(
                    Vec4::new(2.0, 0.0, 0.0, 0.0),
                    Vec4::ZERO,
                    Vec4::new(0.0, -2.0, 0.0, 0.0),
                    Vec4::new(0.0, 0.0, 0.5, 1.0),
                ),
                world_from_actor,
                camera_position: Vec3::ZERO,
                texture_size: [2, 1],
                texels: &[[220, 0, 0, 255], [0, 220, 0, 255]],
            },
        )
    };
    let separate = raster(&[(0..count, 0..1), (0..count, 1..2)]);
    let merged = raster(&merged);
    assert_eq!(merged, separate);
    assert!(
        merged
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 100 && pixel[1] == 0)
    );
    assert!(
        merged
            .chunks_exact(4)
            .any(|pixel| pixel[1] > 100 && pixel[0] == 0)
    );
}
