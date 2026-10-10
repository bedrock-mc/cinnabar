use std::sync::atomic::Ordering;

use bevy::{
    core_pipeline::core_3d::{
        Opaque3d,
        graph::{Core3d, Node3d},
    },
    prelude::*,
    render::{
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_phase::{
            BinnedPhaseItem, DrawFunctions, PhaseItemExtraIndex, ViewBinnedRenderPhases,
        },
        render_resource::RenderPassDescriptor,
        renderer::RenderContext,
        view::ExtractedView,
    },
};

use super::{
    Probe,
    readback::{FAILED, IN_FLIGHT, MAPPED},
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct LayerProbeLabel;

/// Adds the diagnostic after the normal opaque pass without changing its attachments or pixels.
pub(super) fn install_graph(world: &mut World) {
    let node = ViewNodeRunner::new(LayerProbeNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(LayerProbeLabel, node);
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        LayerProbeLabel,
        Node3d::MainTransmissivePass,
    ));
}

struct LayerProbeNode;

impl ViewNode for LayerProbeNode {
    type ViewQuery = &'static ExtractedView;

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        view: &'w ExtractedView,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let probe = world.resource::<Probe>();
        if probe.view != Some(graph.view_entity()) {
            return Ok(());
        }
        let Some(target) = &probe.target else {
            return Ok(());
        };
        let phases = world.resource::<ViewBinnedRenderPhases<Opaque3d>>();
        let Some(phase) = phases.get(&view.retained_view_entity) else {
            return Ok(());
        };
        {
            let colors = [Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("opaque layer diagnostic"),
                color_attachments: &colors,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let Some(viewport) = crate::render_bounds::viewport(
                &target.viewport,
                crate::render_bounds::extent(&target.view),
            ) else {
                return Ok(());
            };
            pass.set_camera_viewport(&viewport);
            let functions = world.resource::<DrawFunctions<Opaque3d>>();
            let mut functions = functions.write();
            functions.prepare(world);
            for ((batch, bin), entries) in &phase.non_mesh_items {
                let Some(&pipeline) = probe.variants.get(&batch.pipeline) else {
                    continue;
                };
                if !super::counted(crate::chunk::pipeline::opaque::timing_category(
                    &functions,
                    batch.draw_function,
                )) {
                    continue;
                }
                let mut batch = batch.clone();
                batch.pipeline = pipeline;
                for (main, entity) in &entries.entities {
                    let item = Opaque3d::new(
                        batch.clone(),
                        bin.clone(),
                        (*entity, *main),
                        0..1,
                        PhaseItemExtraIndex::None,
                    );
                    if let Some(draw) = functions.get_mut(batch.draw_function)
                        && let Err(error) = draw.draw(world, &mut pass, graph.view_entity(), &item)
                    {
                        bevy::log::error!("Opaque layer diagnostic draw failed: {error:?}");
                    }
                }
            }
        }
        let encoder = context.command_encoder();
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &target.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.row_bytes),
                    rows_per_image: Some(target.size[1]),
                },
            },
            target.texture.size(),
        );
        target.state.store(IN_FLIGHT, Ordering::Release);
        let state = target.state.clone();
        encoder.map_buffer_on_submit(&target.buffer, wgpu::MapMode::Read, .., move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { FAILED },
                Ordering::Release,
            );
        });
        Ok(())
    }
}
