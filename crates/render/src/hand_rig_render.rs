//! Near-camera first-person pass that draws the local player's own animated rig (arms + hands)
//! over the scene, reusing the actor rig's packed buffers with a hand-local view and lighting.
//! The rendered content is the player's own skin on the standard samples player geometry.
use crate::{ActorGpuInstance, ActorRigGeometrySpan, ActorRigRenderFrame};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, graph::Core3d},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{RenderGraph, RenderLabel, ViewNodeRunner},
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, ViewTarget},
    },
};
use render_api::SkinRgba8;
use render_model::ActorRigVertex;
use std::{mem::size_of, sync::Arc};

mod gpu;
mod node;
use gpu::*;
#[cfg(test)]
mod tests;

const HAND_RIG_SHADER: Handle<Shader> = uuid_handle!("6f2b1c74-4a2e-49d8-9c1a-2f7b0d5e3a61");
/// Near plane of vanilla's first-person projection.
const HAND_RIG_NEAR_PLANE: f32 = 0.025;

/// Instance texture-selector bits shared with the hand shader.
pub const HAND_ITEM_LAYER_FLAG: u32 = 0x8000_0000;
pub const HAND_OFFHAND_LAYER_FLAG: u32 = 0x4000_0000;
const HAND_BLEND_LAYER_FLAG: u32 = 0x2000_0000;
const HAND_CUTOUT_LAYER_FLAG: u32 = 0x1000_0000;
const HAND_TEXTURE_LAYER_MASK: u32 = !(HAND_ITEM_LAYER_FLAG
    | HAND_OFFHAND_LAYER_FLAG
    | HAND_BLEND_LAYER_FLAG
    | HAND_CUTOUT_LAYER_FLAG);

/// Alpha treatment selected from a held block's admitted face materials.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HandItemAlphaMode {
    #[default]
    Opaque,
    Cutout,
    Blend,
}

impl HandItemAlphaMode {
    /// Encodes the item's alpha mode alongside its artwork-array layer.
    pub const fn texture_layer_flag(self) -> u32 {
        match self {
            Self::Opaque => 0,
            Self::Cutout => HAND_CUTOUT_LAYER_FLAG,
            Self::Blend => HAND_BLEND_LAYER_FLAG,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct HandMaterialUniform {
    texture_flags: [u32; 4],
    layer_mask: [u32; 4],
}

const HAND_MATERIAL: HandMaterialUniform = HandMaterialUniform {
    texture_flags: [
        HAND_ITEM_LAYER_FLAG,
        HAND_OFFHAND_LAYER_FLAG,
        HAND_BLEND_LAYER_FLAG,
        HAND_CUTOUT_LAYER_FLAG,
    ],
    layer_mask: [HAND_TEXTURE_LAYER_MASK, 0, 0, 0],
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct HandRigLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct HandRigRenderPlugin;

impl Plugin for HandRigRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}

/// World light levels and optional Java directional lights in the hand's camera frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HandRigLight {
    pub block_level: u32,
    pub sky_level: u32,
    pub daylight: f32,
    pub pad: u32,
    /// The first direction's W enables Java shading; zero preserves vanilla's material.
    pub java_lights: [[f32; 4]; 2],
    /// Native Z normals in each rig's frame: body, main hand, offhand.
    pub java_normal_axes: [[f32; 4]; 3],
}

impl HandRigLight {
    /// Sets Java's two fixed lights after view bob and look rotation, before hand sway.
    pub fn with_java_lighting(mut self, camera_from_light: Mat4) -> Self {
        self.java_lights =
            [Vec3::new(0.2, 1.0, -0.7), Vec3::new(-0.2, 1.0, 0.7)].map(|direction| {
                camera_from_light
                    .transform_vector3(direction.normalize())
                    .extend(1.0)
                    .to_array()
            });
        self.java_normal_axes = [Vec3::Z.extend(0.0).to_array(); 3];
        self
    }
}

const _: () = assert!(size_of::<HandRigLight>() == 96);

/// The equipment atlas page an item instance samples (layer chosen by the instance's
/// `texture_layer` with its top bit set).
#[derive(Clone, Debug)]
pub struct HandItemAtlas {
    pub width: u16,
    pub height: u16,
    pub layers: u32,
    pub rgba8: Arc<[u8]>,
}

/// One frame's local first-person arms and held items in camera space, with their
/// artwork, lighting, and base FOV.
#[derive(Clone, Debug)]
pub(crate) struct HandRigFrame {
    pub(crate) rig: ActorRigRenderFrame,
    pub(crate) skin: SkinRgba8,
    pub(crate) light: HandRigLight,
    pub(crate) fov_radians: f32,
    pub(crate) revision: u64,
    pub(crate) item_atlases: [Option<HandItemAtlas>; 2],
}

/// Published by the app each frame the first-person hand should draw; empty otherwise.
#[derive(Clone, Default, Debug, Resource, ExtractResource)]
pub struct HandRigScene {
    pub(crate) frame: Option<HandRigFrame>,
}

impl HandRigScene {
    pub fn clear(&mut self) {
        self.frame = None;
    }

    /// Shares the skin and projection admission used by early first-person readiness.
    pub fn accepts_skin_and_fov(skin: &SkinRgba8, fov_radians: f32) -> bool {
        skin.len() == render_model::STANDARD_SKIN_BYTES
            && fov_radians > 0.0
            && fov_radians < std::f32::consts::PI
    }

    /// Accepts a single-instance rig frame with a 64x64 RGBA skin and a finite positive FOV;
    /// anything else clears the scene so the fallback keeps rendering.
    pub fn publish(
        &mut self,
        rig: ActorRigRenderFrame,
        skin: SkinRgba8,
        light: HandRigLight,
        fov_radians: f32,
        revision: u64,
    ) -> bool {
        if rig.instances.is_empty()
            || rig.previous_bones.is_empty()
            || rig.previous_bones.len() != rig.current_bones.len()
            || rig.maximum_vertex_count == 0
            || !Self::accepts_skin_and_fov(&skin, fov_radians)
            || revision == 0
        {
            self.clear();
            return false;
        }
        self.frame = Some(HandRigFrame {
            rig,
            skin,
            light,
            fov_radians,
            revision,
            item_atlases: [None, None],
        });
        true
    }

    /// Supplies the atlas page for item instances of the published frame; ignored when inactive
    /// or when the page is malformed.
    pub fn set_item_atlases(&mut self, atlases: [Option<HandItemAtlas>; 2]) {
        let valid = |atlas: &HandItemAtlas| {
            atlas.width != 0
                && atlas.height != 0
                && atlas.layers != 0
                && usize::from(atlas.width)
                    .checked_mul(usize::from(atlas.height))
                    .and_then(|pixels| pixels.checked_mul(atlas.layers as usize))
                    .and_then(|pixels| pixels.checked_mul(4))
                    == Some(atlas.rgba8.len())
        };
        if let Some(frame) = &mut self.frame {
            frame.item_atlases = atlases.map(|atlas| atlas.filter(valid));
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.frame.is_some()
    }
}

fn install(app: &mut App) {
    app.init_resource::<HandRigScene>();
    crate::pipeline_warmup::register::<HandRigGpu>(app);
    crate::lighting::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<HandRigScene>::default());
    load_internal_asset!(
        app,
        HAND_RIG_SHADER,
        "hand_rig.wgsl",
        crate::shader_safety::from_actor_wgsl,
        crate::actor::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS
    );
    let render_app = app.sub_app_mut(RenderApp);
    render_app
        .insert_resource(Installed)
        .add_systems(RenderStartup, init_gpu)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
    install_graph(render_app.world_mut());
}

/// The rig pass Enhanced views run after Bloom and grading.
#[cfg(feature = "enhanced")]
pub(crate) fn enhanced_post_node(world: &mut World) -> impl bevy::render::render_graph::Node {
    ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, true>(node::HandRigViewNode),
        world,
    )
}

fn install_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() {
        return;
    }
    let runner = ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, false>(node::HandRigViewNode),
        world,
    );
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if graph
        .get_node_state(crate::ui_render::UiOverlayLabel)
        .is_err()
    {
        return;
    }
    if graph.get_node_state(HandRigLabel).is_err() {
        graph.add_node(HandRigLabel, runner);
    }
    // The last hand draw resolves world samples before post-processing and the HUD.
    graph.add_node_edges((
        crate::ui_render::UiWorldLabel,
        HandRigLabel,
        bevy::core_pipeline::core_3d::graph::Node3d::EndMainPass,
    ));
    if graph
        .get_node_state(crate::viewmodel_render::HandLabel)
        .is_ok()
    {
        let _ = graph.try_add_node_edge(crate::viewmodel_render::HandLabel, HandRigLabel);
    }
}

#[derive(Resource)]
struct Installed;
