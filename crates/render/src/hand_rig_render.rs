//! Near-camera first-person pass that draws the local player's own animated rig (arms + hands)
//! over the scene, reusing the actor rig's packed buffers with a hand-local view and lighting.
//! The rendered content is the player's own skin on the standard samples player geometry.
use crate::{ActorGpuInstance, ActorRigGeometrySpan, ActorRigRenderFrame};
#[cfg(all(test, target_os = "macos"))]
use bevy::prelude::{Entity, GlobalTransform, UVec4};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{Core3d, core_3d::CORE_3D_DEPTH_FORMAT},
    prelude::{
        App, BevyError, Commands, Handle, IntoScheduleConfigs, Mat4, Msaa, Plugin, Query, Res,
        ResMut, Resource, Result, Shader, SystemSet, Vec3, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Texture, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        view::ExtractedView,
    },
};
use render_api::{CAMERA_NEAR_PLANE_BLOCKS, SkinRgba8};
use render_model::ActorRigVertex;
use std::{mem::size_of, sync::Arc};

mod gpu;
mod node;
use gpu::*;
#[cfg(test)]
mod tests;

const HAND_RIG_SHADER: Handle<Shader> = uuid_handle!("6f2b1c74-4a2e-49d8-9c1a-2f7b0d5e3a61");

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

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
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
        render_model::actor_skin_side(skin).is_some()
            && fov_radians > 0.0
            && fov_radians < std::f32::consts::PI
    }

    /// Accepts a single-instance rig frame with an admitted square skin and a finite positive FOV;
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
    crate::upload_staging::install(app);
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
pub(crate) fn enhanced_post_pass(
    world: &World,
) -> Option<bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem>> {
    world.contains_resource::<Installed>().then(|| {
        crate::gpu_timing::profiled(
            node::hand_rig,
            Some(crate::RuntimeStage::GpuHand),
            "EnhancedHandRigLabel",
        )
        .run_if(crate::ui_render::overlay::grade_stage::<true>)
    })
}

pub(crate) fn install_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() || world.contains_resource::<RigPassInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    node::hand_rig,
                    Some(crate::RuntimeStage::GpuHand),
                    "HandRigLabel",
                )
                .in_set(HandRigLabel)
                .after(crate::ui_render::UiWorldLabel)
                .after(crate::viewmodel_render::HandLabel)
                .before(crate::scene_target::ScenePass::Finish)
                .in_set(bevy::core_pipeline::Core3dSystems::MainPass)
                .run_if(crate::ui_render::overlay::grade_stage::<false>),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(RigPassInstalled);
    }
}

#[derive(Resource)]
struct RigPassInstalled;

#[derive(Resource)]
struct Installed;
