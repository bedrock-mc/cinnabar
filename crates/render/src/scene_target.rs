//! Main-pass colour retains sample coverage until the final resolve before post-processing.

#[path = "scene_target/nodes.rs"]
mod nodes;
#[cfg(test)]
#[path = "scene_target/tests.rs"]
mod tests;

use bevy::{
    camera::CameraMainTextureUsages,
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_graph::{Node, RenderGraph, ViewNodeRunner},
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
    },
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Compatible views share every colour sample across linear and encoded material passes.
#[derive(Component)]
pub(crate) struct SceneTarget {
    pub(crate) texture: Texture,
    encoded: TextureView,
    linear: TextureView,
    format: TextureFormat,
    resolved: AtomicBool,
}

impl SceneTarget {
    /// Allocates only renderable samples; the single-sample path permits its final raw transfer.
    fn new(device: &RenderDevice, size: Extent3d, format: TextureFormat, samples: u32) -> Self {
        let encoded_format = format.remove_srgb_suffix();
        let view_formats = (format != encoded_format).then_some([format]);
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("shared main-pass colour"),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: TextureDimension::D2,
            format: encoded_format,
            usage: TextureUsages::RENDER_ATTACHMENT
                | if samples == 1 {
                    TextureUsages::COPY_SRC | TextureUsages::COPY_DST
                } else {
                    TextureUsages::empty()
                },
            view_formats: view_formats.as_ref().map_or(&[], |formats| formats),
        });
        let encoded = texture.create_view(&TextureViewDescriptor::default());
        let linear = texture.create_view(&TextureViewDescriptor {
            format: Some(format),
            ..default()
        });
        Self {
            texture,
            encoded,
            linear,
            format,
            resolved: AtomicBool::new(false),
        }
    }

    /// Reuses the attachment until its dimensions, colour format or sample count changes.
    fn matches(&self, size: Extent3d, format: TextureFormat, samples: u32) -> bool {
        self.texture.size() == size
            && self.format == format
            && self.texture.sample_count() == samples
    }

    /// Selects the material's blend colour space without changing the underlying samples.
    pub(crate) fn color_view(&self, encoded: bool) -> &TextureView {
        if encoded { &self.encoded } else { &self.linear }
    }

    /// Bevy owns the first-clear flag; subsequent world passes load these same colour samples.
    pub(crate) fn color_attachment<'a>(
        &'a self,
        target: &ViewTarget,
        encoded: bool,
    ) -> RenderPassColorAttachment<'a> {
        let mut load = target.get_unsampled_color_attachment().ops.load;
        if encoded
            && self.format.is_srgb()
            && let LoadOp::Clear(colour) = load
        {
            let colour = Srgba::from(LinearRgba::new(
                colour.r as f32,
                colour.g as f32,
                colour.b as f32,
                colour.a as f32,
            ));
            load = LoadOp::Clear(wgpu::Color {
                r: colour.red.into(),
                g: colour.green.into(),
                b: colour.blue.into(),
                a: colour.alpha.into(),
            });
        }
        self.attachment(encoded, load, None)
    }

    /// A resolve consumes the final colour samples; intermediate passes always retain them.
    fn attachment<'a>(
        &'a self,
        encoded: bool,
        load: LoadOp<wgpu::Color>,
        resolve: Option<&'a TextureView>,
    ) -> RenderPassColorAttachment<'a> {
        RenderPassColorAttachment {
            view: self.color_view(encoded),
            depth_slice: None,
            resolve_target: resolve.map(|view| &**view),
            ops: Operations {
                load,
                store: if resolve.is_some() {
                    StoreOp::Discard
                } else {
                    StoreOp::Store
                },
            },
        }
    }

    /// A colour-reading consumer may resolve without discarding samples still needed by geometry.
    pub(crate) fn resolve_attachment<'a>(
        &'a self,
        destination: &'a TextureView,
        store: StoreOp,
    ) -> RenderPassColorAttachment<'a> {
        let mut attachment = self.attachment(false, LoadOp::Load, Some(destination));
        attachment.ops.store = store;
        attachment
    }

    /// The last ordinary draw resolves directly and makes the separate end boundary a no-op.
    pub(crate) fn final_attachment<'a>(
        &'a self,
        destination: &'a TextureView,
    ) -> RenderPassColorAttachment<'a> {
        self.resolved.store(true, Ordering::Relaxed);
        self.resolve_attachment(destination, StoreOp::Discard)
    }

    /// Lets overlays continue drawing after an effect has restored the scene colour.
    pub(crate) fn resume(&self) {
        self.resolved.store(false, Ordering::Relaxed);
    }

    /// A retained attachment must resolve again even when the viewport is unchanged.
    fn begin_frame(&self) {
        self.resolved.store(false, Ordering::Relaxed);
    }

    /// Makes the completed scene available to single-sample post-processing without a draw.
    pub(crate) fn finish(&self, context: &mut RenderContext, world: &World, target: &ViewTarget) {
        if self.resolved.load(Ordering::Relaxed) {
            return;
        }
        if self.texture.sample_count() == 1 {
            context.command_encoder().copy_texture_to_texture(
                self.texture.as_image_copy(),
                target.main_texture().as_image_copy(),
                self.texture.size(),
            );
            self.resolved.store(true, Ordering::Relaxed);
            return;
        }
        let attachments = [Some(self.final_attachment(target.main_texture_view()))];
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("final scene colour resolve"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuBlit,
                ),
                occlusion_query_set: None,
            });
    }
}

/// Registers the retained main attachment before any consumer prepares its view resources.
pub(crate) fn install(app: &mut App) {
    app.add_systems(Last, admit_copy_destination);
    app.sub_app_mut(RenderApp)
        .init_resource::<WithheldSamples>()
        .add_systems(Render, render_systems());
}

/// Allocates the shared attachment in place of Bevy's multisampled colour target.
fn render_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    use bevy::render::view::prepare_view_targets;
    (
        withhold_view_samples
            .in_set(RenderSystems::ManageViews)
            .before(prepare_view_targets),
        restore_view_samples
            .in_set(RenderSystems::ManageViews)
            .after(prepare_view_targets),
        prepare_scene_targets
            .in_set(RenderSystems::PrepareResources)
            .after(prepare_view_targets),
    )
        .into_configs()
}

/// 3D sample counts hidden from Bevy's view-target allocation for the current frame.
#[derive(Resource, Default)]
struct WithheldSamples(Vec<(Entity, Msaa)>);

/// Bevy's view target then keeps only single-sample textures; `SceneTarget` owns the samples.
fn withhold_view_samples(
    mut withheld: ResMut<WithheldSamples>,
    mut views: Query<(Entity, &mut Msaa), With<Camera3d>>,
) {
    withheld.0.clear();
    for (entity, mut msaa) in &mut views {
        if *msaa != Msaa::Off {
            withheld.0.push((entity, *msaa));
            *msaa = Msaa::Off;
        }
    }
}

/// Restores sample counts before depth, pipelines and the shared attachment read them.
fn restore_view_samples(mut withheld: ResMut<WithheldSamples>, mut views: Query<&mut Msaa>) {
    for (entity, samples) in withheld.0.drain(..) {
        if let Ok(mut msaa) = views.get_mut(entity) {
            *msaa = samples;
        }
    }
}

/// The AA-off path transfers encoded bytes into Bevy's post-processing image.
fn admit_copy_destination(mut cameras: Query<&mut CameraMainTextureUsages, With<Camera3d>>) {
    for mut usages in &mut cameras {
        usages.0 |= TextureUsages::COPY_DST;
    }
}

/// Keeps one bounded colour allocation per camera and drops stale camera resources.
pub(crate) fn prepare_scene_targets(
    mut commands: Commands,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ViewTarget, &Msaa, Option<&SceneTarget>), With<Camera3d>>,
    removed: Query<Entity, (With<SceneTarget>, Without<ViewTarget>)>,
) {
    for entity in &removed {
        commands.entity(entity).remove::<SceneTarget>();
    }
    for (entity, target, msaa, previous) in &views {
        let size = target.main_texture().size();
        let format = target.main_texture_format();
        if previous.is_some_and(|scene| scene.matches(size, format, msaa.samples())) {
            previous.unwrap().begin_frame();
            continue;
        }
        commands
            .entity(entity)
            .insert(SceneTarget::new(&device, size, format, msaa.samples()));
    }
}

/// Draws the opaque and cutout phases into the shared scene samples.
pub(crate) fn opaque_pass(world: &mut World) -> Box<dyn Node> {
    Box::new(ViewNodeRunner::new(nodes::SceneOpaquePass, world))
}

/// Recognises the installed shared-scene opaque pass.
pub(crate) fn is_opaque_pass(node: &dyn Node) -> bool {
    node.downcast_ref::<ViewNodeRunner<nodes::SceneOpaquePass>>()
        .is_some()
}

/// Replaces only main-pass nodes, preserving every installed dependency and post-processing node.
pub(crate) fn install_graph(world: &mut World) {
    let opaque = opaque_pass(world);
    let transmissive = ViewNodeRunner::new(nodes::SceneTransmissivePass, world);
    let finish = ViewNodeRunner::new(nodes::SceneFinish, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if let Ok(node) = graph.get_node_state_mut(Node3d::MainOpaquePass) {
        node.node = opaque;
        node.type_name = std::any::type_name::<ViewNodeRunner<nodes::SceneOpaquePass>>();
    }
    if let Ok(node) = graph.get_node_state_mut(Node3d::MainTransmissivePass) {
        node.node = Box::new(transmissive);
        node.type_name = std::any::type_name::<ViewNodeRunner<nodes::SceneTransmissivePass>>();
    }
    if let Ok(node) = graph.get_node_state_mut(Node3d::EndMainPass) {
        node.node = Box::new(finish);
        node.type_name = std::any::type_name::<ViewNodeRunner<nodes::SceneFinish>>();
    }
}
