//! Java-style 2D block atlases (`gtexture`, `normals`, `specular`) built from Cinnabar's
//! texture-array pages for shader packs.
//!
//! # Layout
//! Every live carrier layer becomes one power-of-two square sprite holding the region of the
//! physical layer Cinnabar's shaders sample for sprite-local UV `[0, 1)²` (the upper-left
//! `page_side >> grid` square), selected by source LOD before removing physical expansion.
//! Sprites are packed largest first, then by slot, each aligned to
//! its own side, so every one of the atlas's [`ShaderpackAtlas::mip_levels`] lands on whole
//! texels. The level count is Java's [`JAVA_MIPMAP_LEVELS`] plus the base, reduced so the
//! smallest sprite keeps one texel.
//!
//! The remap buffer is an array of two-word slots. Slot 0 is `[width, height]` of mip 0;
//! sprite slots are `[x | y << 16, w | h << 16]` in mip-0 texels:
//! - [`static_sprite_slot`] for each carrier page layer;
//! - [`opaque_sprite_slot`]: the same texture with alpha forced to 255, packed for overlay-mask
//!   materials and otherwise repeating the static slot;
//! - [`animation_sprite_slot`]: a display sprite (sized to the largest frame) that always shows
//!   the frame the world animation clock selects, blending into the next frame when the
//!   animation blends.
//!
//! Absent textures and unusable animations name a generated missing sprite.
//!
//! # Overflow
//! When the sprites do not fit `max_texture_dimension_2d`, every sprite is halved (using the
//! next source mip as its base) until the atlas fits, and one warning reports the scale.
//! Remap rectangles always describe the stored sprites.
//!
//! # PBR atlases
//! `normals` and `specular` share the colour layout and hold LabPBR 1.3 texels: neutral
//! `(128, 128, 255, 255)` and `(0, 0, 0, 0)` without authored maps. Enhanced builds with
//! authored maps resolve carrier slots and agreeing material/frame overrides. Conflicting
//! overrides stay neutral because one atlas sprite serves every material sharing a layer.

mod animate;
mod blend;
#[cfg(test)]
mod gpu_tests;
mod labpbr;
mod pack;

mod pixels;
mod plan;
#[cfg(test)]
mod tests;
mod upload;

pub(crate) use labpbr::{NEUTRAL_NORMAL, NEUTRAL_SPECULAR};

use super::{geometry::ShaderpackGeometrySettings, plugin::LoadedShaderPack};
use animate::AtlasAnimations;
use bevy::{
    asset::load_internal_asset,
    ecs::system::SystemParam,
    math::UVec2,
    prelude::{App, Commands, IntoScheduleConfigs, Plugin, Query, Res, ResMut, Resource, With},
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_resource::{Buffer, PipelineCache, Sampler, TextureView},
        renderer::{PendingCommandBuffers, RenderDevice, RenderQueue},
    },
    tasks::{AsyncComputeTaskPool, Task, block_on},
};
use plan::{AtlasInputs, AtlasPlan, LayerLayout};
use render_scene::{ChunkAnimationClock, ChunkTextureAssets, shaderpack::ShaderPackRendering};
use std::sync::Arc;
use upload::UploadedAtlas;

/// Slot 0 of the remap table is the header `[atlas_width, atlas_height]`.
pub const ATLAS_HEADER_SLOTS: u32 = 1;
/// One slot per carrier texture page layer, indexed `page * (assets::TEXTURE_LAYER_MASK + 1) + layer`.
pub const STATIC_SPRITE_SLOTS: u32 =
    assets::MAX_TEXTURE_PAGES as u32 * (assets::TEXTURE_LAYER_MASK + 1);
/// Highest Java mipmap-level setting; the atlas stores at most this many levels below its base.
pub const JAVA_MIPMAP_LEVELS: u32 = 4;

/// Remap slot of a carrier layer as sampled.
pub const fn static_sprite_slot(page: u32, layer: u32) -> u32 {
    ATLAS_HEADER_SLOTS + page * (assets::TEXTURE_LAYER_MASK + 1) + layer
}

/// Remap slot of a carrier layer with alpha forced opaque.
pub const fn opaque_sprite_slot(page: u32, layer: u32) -> u32 {
    ATLAS_HEADER_SLOTS + STATIC_SPRITE_SLOTS + page * (assets::TEXTURE_LAYER_MASK + 1) + layer
}

/// Remap slot of animation `animation`'s display sprite.
pub const fn animation_sprite_slot(animation: u32) -> u32 {
    ATLAS_HEADER_SLOTS + 2 * STATIC_SPRITE_SLOTS + animation
}

/// The published atlas generation; replaced as a whole when the carrier changes.
#[derive(Resource)]
pub struct ShaderpackAtlas {
    /// Encoded RGBA8 colour with mips, sampled as the pack's `gtexture`/`texture`/`tex`.
    pub color: TextureView,
    /// LabPBR normal atlas in the same layout.
    pub normals: TextureView,
    /// LabPBR specular atlas in the same layout.
    pub specular: TextureView,
    /// Nearest texels with linear mip blending, clamped.
    pub sampler: Sampler,
    pub remap: Buffer,
    pub size: UVec2,
    pub mip_levels: u32,
    /// Increments whenever sprite rectangles may have changed.
    pub generation: u64,
    /// Texture bindings retained for the lifetime of this atlas generation.
    pub(crate) source: ChunkTextureAssets,
}

/// Builds the atlas off the render thread and keeps its animated sprites current.
pub struct ShaderpackAtlasPlugin;

impl Plugin for ShaderpackAtlasPlugin {
    fn build(&self, app: &mut App) {
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_internal_asset!(
            app,
            blend::BLEND_SHADER,
            "atlas/blend.wgsl",
            |source, path| { crate::shader_safety::from_wgsl(source, path) }
        );
        app.sub_app_mut(RenderApp)
            .init_resource::<AtlasBuild>()
            .add_systems(RenderStartup, blend::init_blend_pipelines)
            .add_systems(
                Render,
                (prepare_shaderpack_atlas, update_shaderpack_animations)
                    .chain()
                    .run_if(atlas_requested)
                    .in_set(RenderSystems::PrepareResources),
            )
            .add_systems(
                Render,
                release_shaderpack_atlas
                    .run_if(bevy::ecs::schedule::common_conditions::not(atlas_requested))
                    .in_set(RenderSystems::PrepareResources),
            );
    }
}

/// Atlas work follows a loaded camera's pack or an explicitly enabled geometry adapter.
fn atlas_requested(
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<(), With<ShaderPackRendering>>,
    geometry: Option<Res<ShaderpackGeometrySettings>>,
) -> bool {
    geometry.is_some_and(|settings| settings.enabled)
        || (pack.is_some_and(|pack| pack.0.is_some()) && !cameras.is_empty())
}

/// Releases atlas textures, animation state and pending work when no camera uses a pack.
fn release_shaderpack_atlas(mut commands: Commands, mut build: ResMut<AtlasBuild>) {
    build.requested = None;
    build.task = None;
    commands.remove_resource::<ShaderpackAtlas>();
    commands.remove_resource::<AtlasAnimations>();
}

/// A finished background build.
struct BuiltAtlas {
    plan: AtlasPlan,
    uploaded: UploadedAtlas,
    sampler: wgpu::Sampler,
    source: ChunkTextureAssets,
}

/// The requested carrier and its in-flight replacement; the published atlas stays until then.
#[derive(Resource, Default)]
struct AtlasBuild {
    requested: Option<ChunkTextureAssets>,
    task: Option<Task<Option<BuiltAtlas>>>,
    generation: u64,
}

impl AtlasBuild {
    /// Claims a changed source once and owns its pointer identities until the next request.
    fn request(&mut self, assets: &ChunkTextureAssets) -> bool {
        if self
            .requested
            .as_ref()
            .is_some_and(|requested| requested.binding_identity() == assets.binding_identity())
        {
            return false;
        }
        self.requested = Some(assets.clone());
        true
    }
}

/// Builds replacement atlases off the render thread, retaining the previous generation until ready.
fn prepare_shaderpack_atlas(
    mut commands: Commands,
    assets: Option<Res<ChunkTextureAssets>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut build: ResMut<AtlasBuild>,
) {
    if let Some(assets) = assets
        && build.request(&assets)
    {
        let (assets, device, queue) = (assets.clone(), device.clone(), queue.clone());
        build.task = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { build_atlas(assets, device.wgpu_device(), &queue) }),
        );
    }
    if !build.task.as_ref().is_some_and(Task::is_finished) {
        return;
    }
    let Some(built) = build.task.take().and_then(block_on) else {
        return;
    };
    build.generation += 1;
    let (atlas, animations) = publish(built, build.generation);
    commands.insert_resource(atlas);
    commands.insert_resource(animations);
}

/// Plans, assembles and uploads one generation; `None` logs why the previous atlas stays.
fn build_atlas(
    assets: ChunkTextureAssets,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Option<BuiltAtlas> {
    let _zone = bevy::log::info_span!("shaderpack.atlas_build").entered();
    let runtime = Arc::clone(assets.assets());
    let pages = (0..runtime.texture_pages().len())
        .map(|page| runtime.terrain_texture_page(page))
        .collect::<Result<Vec<_>, _>>()
        .inspect_err(|error| bevy::log::error!(?error, "invalid shader-pack atlas source page"))
        .ok()?;
    let pages = pages.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    let layout = |reference| LayerLayout {
        grid: runtime.texture_source_grid(reference),
        source_side: runtime
            .texture_source_size(reference)
            .into_iter()
            .max()
            .unwrap_or(1),
    };
    #[cfg(feature = "enhanced")]
    let authored = assets.enhanced().map(|enhanced| {
        plan::AuthoredPbr::new(
            enhanced.normal_pages.each_ref(),
            enhanced.mer_pages.each_ref(),
            &enhanced.texture_refs,
        )
    });
    #[cfg(not(feature = "enhanced"))]
    let authored = None;
    let inputs = AtlasInputs {
        pages: &pages,
        materials: runtime.materials(),
        animations: runtime.animations(),
        animation_frames: runtime.animation_frames(),
        layout: &layout,
        authored,
    };
    let limits = device.limits();
    let Some(plan) = plan::plan_atlas(&inputs, limits.max_texture_dimension_2d) else {
        bevy::log::error!(
            max_dimension = limits.max_texture_dimension_2d,
            "shader-pack atlas does not fit even with one-texel sprites"
        );
        return None;
    };
    if plan.scale > 0 {
        bevy::log::warn!(
            scale = format!("1/{}", 1_u64 << plan.scale),
            max_dimension = limits.max_texture_dimension_2d,
            "shader-pack atlas sprites are stored below source resolution to fit the device"
        );
    }
    let remap_bytes = std::mem::size_of_val(plan.remap.as_slice()) as u64;
    if remap_bytes > limits.max_storage_buffer_binding_size || remap_bytes > limits.max_buffer_size
    {
        bevy::log::error!(
            remap_bytes,
            "shader-pack atlas remap table exceeds adapter limits"
        );
        return None;
    }
    let images = pixels::build_images(&inputs, &plan);
    let uploaded = upload::upload_atlas(device, queue, &plan, &images);
    let sampler = device.create_sampler(&upload::atlas_sampler_descriptor(plan.mip_levels));
    Some(BuiltAtlas {
        plan,
        uploaded,
        sampler,
        source: assets.clone(),
    })
}

/// Creates the published views and the animation state of a finished build.
fn publish(built: BuiltAtlas, generation: u64) -> (ShaderpackAtlas, AtlasAnimations) {
    let BuiltAtlas {
        plan,
        uploaded,
        sampler,
        source,
    } = built;
    let [normals, specular] = uploaded
        .atlas
        .pbr
        .as_ref()
        .expect("the atlas always uploads PBR layers")
        .each_ref()
        .map(|texture| texture.create_view(&Default::default()));
    let color = uploaded
        .atlas
        .color
        .create_view(&wgpu::TextureViewDescriptor {
            label: Some("shader-pack gtexture"),
            format: Some(upload::COLOR_VIEW_FORMAT),
            ..Default::default()
        });
    let atlas = ShaderpackAtlas {
        color: color.into(),
        normals: normals.into(),
        specular: specular.into(),
        sampler: sampler.into(),
        remap: uploaded.remap.clone().into(),
        size: UVec2::new(plan.atlas.width, plan.atlas.height),
        mip_levels: plan.mip_levels,
        generation,
        source: source.clone(),
    };
    let runtime = source.assets();
    let animations = AtlasAnimations::new(
        &uploaded,
        runtime.animations(),
        runtime.animation_frames(),
        plan.animations,
        plan.mip_levels,
        [plan.atlas.width, plan.atlas.height],
    );
    (atlas, animations)
}

/// Optional blend resources used while animation pipelines are compiling.
#[derive(SystemParam)]
struct AnimationBlendResources<'w> {
    pipelines: Option<Res<'w, blend::AtlasBlendPipelines>>,
    cache: Option<Res<'w, PipelineCache>>,
}

/// Refreshes animated display sprites for the extracted world animation clock. The refresh
/// joins the frame's graph submission ahead of the graph's own command buffers.
fn update_shaderpack_animations(
    animations: Option<ResMut<AtlasAnimations>>,
    clock: Option<Res<ChunkAnimationClock>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    blend_resources: AnimationBlendResources,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let (Some(mut animations), Some(clock)) = (animations, clock) else {
        return;
    };
    let compiled = blend_resources
        .pipelines
        .as_deref()
        .zip(blend_resources.cache.as_deref());
    if animations.needs_blend_sources()
        && let Some((pipelines, cache)) = compiled
    {
        let layout = cache.get_bind_group_layout(&pipelines.layout);
        animations.bind_blend_sources(device.wgpu_device(), &layout);
    }
    let blend = compiled.and_then(|(pipelines, cache)| pipelines.get(cache));
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shader-pack animated sprites"),
    });
    if animations.encode(device.wgpu_device(), &queue, &mut encoder, *clock, blend) > 0 {
        pending.push_encoder(encoder, "shader-pack animated sprites");
    }
}
