//! Recording the pack's geometry: the shadow pass, the sky, opaque and translucent terrain,
//! the effects (see `effects`), actors, block entities and the hand (through
//! [`super::actors`]), the depth copies Iris's `depthtex*`/`shadowtex*` semantics need, and
//! the Vanilla overlay that stands in for geometry the host does not draw yet.

use super::actors::ActorSources;
use super::draws::DrawKind;
use super::effects::textures::ShaderpackEffectTextures;
use super::effects::{Effect, EffectInputs, prepare_effects};
use super::geometry::{
    CONSTANT_VERTEX_BUFFER, GEOMETRY_VERTEX_BUFFER, GeometryCategory, TerrainLayout,
};
use super::loader::PackGpu;
use super::passes::{ViewState, clear_attachment, load_attachment, pass};
use super::sky::{SkyGeometry, SkyInputs, sky_geometry};
use super::targets::AtlasViews;
use crate::gpu_timing::{GpuTimestamps, within_stage};
use bevy::render::render_phase::TrackedRenderPass;
use bevy::render::renderer::RenderDevice;
use render_scene::RuntimeStage;
use wgpu::{CommandEncoder, Device, Queue, TextureView};

/// Terrain the pack's geometry programs draw.
pub(crate) trait TerrainDraws {
    /// Records `category` into `pass`, which holds the draw's pipeline and bind groups:
    /// the camera's visible sections, or every resident section for the shadow pass.
    /// Returns the draws issued.
    fn draw<'w>(
        &'w self,
        pass: &mut TrackedRenderPass<'w>,
        category: GeometryCategory,
        shadow: bool,
    ) -> u32;

    /// The expanded vertex the terrain pages hold.
    fn layout(&self) -> TerrainLayout;
}

/// What a view supplies for the pack's geometry this frame.
pub(crate) struct FrameGeometry<'a> {
    pub device: &'a RenderDevice,
    pub terrain: &'a dyn TerrainDraws,
    /// The block atlases and their generation, once built.
    pub atlas: Option<(AtlasViews<'a>, u64)>,
    /// The effects' textures, once the render world has them.
    pub effect_textures: Option<&'a ShaderpackEffectTextures>,
    pub sky: SkyInputs,
    /// What the effects are built from.
    pub effects: EffectInputs<'a>,
    /// Actors, block entities and the hand the pack draws.
    pub actors: ActorSources<'a>,
}

pub(crate) fn depth_attachment(
    view: &TextureView,
    clear: bool,
) -> Option<wgpu::RenderPassDepthStencilAttachment<'_>> {
    Some(wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: if clear {
                wgpu::LoadOp::Clear(1.0)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    })
}

/// Clears a depth attachment to the far plane.
fn clear_depth(encoder: &mut CommandEncoder, view: &TextureView) {
    drop(pass(
        encoder,
        "shader pack depth clear",
        &[],
        depth_attachment(view, true),
    ));
}

/// Admits an attachment only when it shares the draw's established extent.
pub(crate) fn admit_extent(expected: &mut Option<[u32; 2]>, actual: [u32; 2]) -> bool {
    *expected.get_or_insert(actual) == actual
}

/// Opens the render pass of `kind`'s draw (the pack has at most one) over its outputs:
/// `colortex` sides per `write_alt`, or `shadowcolor` targets in the shadow pass, with its
/// pipeline set. `None` without such a draw, or when its outputs differ in size from the
/// depth attachment (logged once). Returns the pass and the draw's index.
pub(crate) fn open_kind<'e>(
    encoder: &'e mut CommandEncoder,
    state: &mut ViewState,
    kind: DrawKind,
    write_alt: u32,
    depth: Option<(&TextureView, [u32; 2])>,
) -> Option<(wgpu::RenderPass<'e>, usize)> {
    let pack = std::sync::Arc::clone(&state.pack);
    let index = pack.draws.iter().position(|d| d.kind == kind)?;
    let draw = &pack.draws[index];
    let targets = &state.targets;
    let mut size = depth.map(|(_, size)| size);
    let mut consistent = true;
    let attachments: Vec<_> = draw
        .outputs
        .iter()
        .map(|output| {
            let buffer = (*output)?;
            let (view, extent) = if kind.shadow() {
                let shadow = pack.shadow.as_ref()?;
                let (.., slot, _) = shadow.slots.color.iter().find(|(i, ..)| *i == buffer)?;
                let side = shadow.slots.resolution;
                (&pack.constants.textures[*slot].view, [side, side])
            } else {
                let colour = targets.buffers[buffer as usize].as_ref()?;
                let side = colour.side(write_alt & (1 << buffer) != 0);
                (&colour.attachment[side], colour.size)
            };
            consistent &= admit_extent(&mut size, extent);
            load_attachment(view)
        })
        .collect();
    if !consistent {
        if state.skipped.insert(format!("{kind:?}")) {
            bevy::log::warn!(
                target: "cinnabar::shaderpack",
                pack = %pack.name, draw = ?kind,
                "draw skipped: its outputs differ in size"
            );
        }
        return None;
    }
    targets.take_snapshots(
        encoder,
        &pack.programs[draw.program],
        super::targets::draw_reads(&pack, draw).alt,
    );
    let depth = depth.and_then(|(view, _)| depth_attachment(view, false));
    let mut render = pass(
        encoder,
        &format!("{} {kind:?}", pack.name),
        &attachments,
        depth,
    );
    render.set_pipeline(&draw.pipeline);
    Some((render, index))
}

/// Records the terrain, sky or effect draw of `kind` with the draw's own bind groups.
fn record_kind(
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    geometry: &FrameGeometry<'_>,
    kind: DrawKind,
    write_alt: u32,
    depth: Option<(&TextureView, [u32; 2])>,
    sky: Option<&SkyGeometry>,
) {
    let pack = std::sync::Arc::clone(&state.pack);
    let effect = match kind {
        DrawKind::Effect(effect) => match state.effects.range(effect) {
            Some(range) => Some(range),
            None => return,
        },
        _ => None,
    };
    let Some((render, index)) = open_kind(encoder, state, kind, write_alt, depth) else {
        return;
    };
    let mut tracked = TrackedRenderPass::new(geometry.device, render);
    let groups = &state.bindings.draws[index].groups[state.targets.parity];
    if pack.draws[index]
        .layout
        .is_some_and(|layout| layout != geometry.terrain.layout())
    {
        // The pages still hold another layout (the pack just changed).
        return;
    }
    {
        let raw = tracked.wgpu_pass();
        for (i, group) in groups.iter().enumerate() {
            if let Some(group) = group {
                raw.set_bind_group(i as u32, group, &[]);
            }
        }
    }
    if let Some(range) = effect {
        let (Some(vertices), Some(indices)) = (&state.effects.vertices, &state.effects.indices)
        else {
            return;
        };
        let raw = tracked.wgpu_pass();
        raw.set_vertex_buffer(GEOMETRY_VERTEX_BUFFER, vertices.slice(..));
        raw.set_vertex_buffer(CONSTANT_VERTEX_BUFFER, pack.host.constant_stream.slice(..));
        raw.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        raw.draw_indexed(range, 0, 0..1);
        return;
    }
    match kind.category() {
        Some(category) => {
            geometry.terrain.draw(&mut tracked, category, kind.shadow());
        }
        None => {
            let (Some(sky), Some(buffer)) = (sky, state.targets.sky.as_ref()) else {
                return;
            };
            let range = if kind == DrawKind::Sunset {
                sky.sunset.clone()
            } else {
                sky.discs.clone()
            };
            if range.is_empty() {
                return;
            }
            let raw = tracked.wgpu_pass();
            raw.set_vertex_buffer(GEOMETRY_VERTEX_BUFFER, buffer.slice(..));
            raw.set_vertex_buffer(CONSTANT_VERTEX_BUFFER, pack.host.constant_stream.slice(..));
            raw.draw(range, 0..1);
        }
    }
}

/// Copies a depth attachment into float targets with the host's copy pipeline.
fn copy_depth_to_floats(
    device: &Device,
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    source: &TextureView,
    targets: &[&TextureView],
) {
    if targets.is_empty() {
        return;
    }
    for target in targets {
        let pipeline = pack.host.depth_copy_for(target.texture().format());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader pack depth copy"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut render = pass(
            encoder,
            "shader pack depth copy",
            &[load_attachment(target)],
            None,
        );
        render.set_pipeline(pipeline);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// Copies the geometry depth into the float `depthtex{floats}` and the comparison
/// `depthtex{compares}` some program samples. `depthtex0` needs no float copy: programs read
/// the geometry depth itself ([`super::targets::ViewTargets`]).
fn copy_view_depth(
    device: &Device,
    encoder: &mut CommandEncoder,
    state: &ViewState,
    floats: &[usize],
    compares: &[usize],
) {
    let targets = &state.targets;
    let Some(depth) = targets.geometry_depth.as_ref() else {
        return;
    };
    let floats: Vec<&TextureView> = floats.iter().map(|i| targets.depth_view(*i)).collect();
    copy_depth_to_floats(device, encoder, &state.pack, &depth.view, &floats);
    for index in compares {
        if let Some(compare) = &targets.depth_compare[*index] {
            encoder.copy_texture_to_texture(
                depth.texture.as_image_copy(),
                compare.texture.as_image_copy(),
                depth.texture.size(),
            );
        }
    }
}

/// Copies the shadow depth into `shadowtex{index}` (and `shadowtex0` too when `index` is 1
/// and nothing translucent follows), except into the `shadowtex0` texture the pass drew into.
fn copy_shadow_depth(
    device: &Device,
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    indices: &[usize],
) {
    let Some(shadow) = pack.shadow.as_ref() else {
        return;
    };
    let floats: Vec<&TextureView> = indices
        .iter()
        .filter_map(|i| shadow.slots.float[*i])
        .map(|slot| &pack.constants.textures[slot].view)
        .collect();
    copy_depth_to_floats(device, encoder, pack, &shadow.depth_view, &floats);
    for slot in indices
        .iter()
        .filter_map(|i| shadow.slots.depth[*i])
        .filter(|slot| Some(*slot) != shadow.attachment_slot)
    {
        encoder.copy_texture_to_texture(
            shadow.depth.as_image_copy(),
            pack.constants.textures[slot].texture.as_image_copy(),
            shadow.depth.size(),
        );
    }
}

/// Brings the view's draw bind groups up to date with the atlas and effect textures.
pub(crate) fn bind_geometry(device: &Device, state: &mut ViewState, geometry: &FrameGeometry<'_>) {
    let generation = (
        geometry.atlas.as_ref().map(|(_, g)| *g),
        geometry.effect_textures.map(|t| t.generation),
    );
    if state.atlas_generation == Some(generation) {
        return;
    }
    let pack = std::sync::Arc::clone(&state.pack);
    let atlas = geometry.atlas.as_ref().map(|(views, _)| *views);
    state
        .bindings
        .bind_draws(device, &pack, &mut state.targets, |kind| match kind {
            DrawKind::Effect(effect) => geometry.effect_textures?.views(effect),
            _ => atlas,
        });
    state.atlas_generation = Some(generation);
}

/// Records the effects of `effects` that draw in this phase, in order, over `write_alt`.
fn record_effects(
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    geometry: &FrameGeometry<'_>,
    effects: impl IntoIterator<Item = Effect>,
    write_alt: u32,
    depth: Option<(&TextureView, [u32; 2])>,
) {
    for effect in effects {
        let depth = depth.filter(|_| !effect.sky());
        record_kind(
            encoder,
            state,
            geometry,
            DrawKind::Effect(effect),
            write_alt,
            depth,
            None,
        );
    }
}

/// The shadow pass: shadow colour clears, opaque shadow casters (terrain, then actors and
/// block entities) into `shadowtex1`, then translucent ones into `shadowtex0`, as Iris
/// renders them.
pub(crate) fn record_shadow(
    device: &Device,
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    geometry: &FrameGeometry<'_>,
) {
    let pack = std::sync::Arc::clone(&state.pack);
    let Some(shadow) = pack.shadow.as_ref() else {
        return;
    };
    for ((_, slot, _), clear) in shadow.slots.color.iter().zip(&shadow.color_clears) {
        if let Some(colour) = clear {
            drop(pass(
                encoder,
                "shader pack shadowcolor clear",
                &[clear_attachment(
                    &pack.constants.textures[*slot].view,
                    *colour,
                )],
                None,
            ));
        }
    }
    clear_depth(encoder, &shadow.depth_view);
    let side = shadow.slots.resolution;
    let depth = Some((&shadow.depth_view, [side, side]));
    if pack.shadow_terrain {
        for kind in [DrawKind::ShadowSolid, DrawKind::ShadowCutout] {
            record_kind(encoder, state, geometry, kind, 0, depth, None);
        }
    }
    for kind in [DrawKind::ShadowEntities, DrawKind::ShadowBlock] {
        super::actors::record(encoder, state, kind, 0, depth);
    }
    let translucent = pack.shadow_translucent
        && pack
            .draws
            .iter()
            .any(|d| d.kind == DrawKind::ShadowTranslucent);
    if translucent {
        copy_shadow_depth(device, encoder, &pack, &[1]);
        record_kind(
            encoder,
            state,
            geometry,
            DrawKind::ShadowTranslucent,
            0,
            depth,
            None,
        );
        copy_shadow_depth(device, encoder, &pack, &[0]);
    } else {
        copy_shadow_depth(device, encoder, &pack, &[0, 1]);
    }
}

/// The sky (no depth) with the sun, moon and stars, then opaque terrain, actors and block
/// entities against a cleared depth attachment and the effects Iris draws before the
/// deferred passes, the copy into `depthtex2` when sampled, the solid hand, then the copy
/// into `depthtex1`, which also stands in for `depthtex0` until translucents draw. Solid
/// and cutout terrain are timed as the terrain stages during aggregate profiling.
pub(crate) fn record_opaque(
    device: &Device,
    queue: &Queue,
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    geometry: &FrameGeometry<'_>,
    timestamps: Option<&GpuTimestamps>,
) {
    let scene_alt = state.pack.frame.scene_alt;
    let sky = sky_geometry(geometry.sky);
    if let Some(buffer) = &state.targets.sky {
        queue.write_buffer(buffer, 0, &sky.bytes);
    }
    let pack = std::sync::Arc::clone(&state.pack);
    prepare_effects(device, queue, &pack, &mut state.effects, &geometry.effects);
    for kind in [DrawKind::Sky, DrawKind::Sunset] {
        record_kind(encoder, state, geometry, kind, scene_alt, None, Some(&sky));
    }
    record_effects(
        encoder,
        state,
        geometry,
        [Effect::Sun, Effect::Moon, Effect::Stars],
        scene_alt,
        None,
    );
    let Some(depth) = state.targets.geometry_depth.as_ref() else {
        return;
    };
    // The attachment outlives this function's borrows of `state`; take a view handle.
    let view = depth.view.clone();
    let size = state.targets.screen_size;
    clear_depth(encoder, &view);
    for (kind, stage) in [
        (DrawKind::TerrainSolid, RuntimeStage::GpuTerrainSolid),
        (DrawKind::TerrainCutout, RuntimeStage::GpuTerrainCutout),
    ] {
        within_stage(timestamps, stage, encoder, |encoder| {
            record_kind(
                encoder,
                state,
                geometry,
                kind,
                scene_alt,
                Some((&view, size)),
                None,
            );
        });
    }
    for kind in [
        DrawKind::Entities,
        DrawKind::SpiderEyes,
        DrawKind::ArmorGlint,
        DrawKind::Block,
    ] {
        super::actors::record(encoder, state, kind, scene_alt, Some((&view, size)));
    }
    let before = [
        Effect::Lightning,
        Effect::DamagedBlock,
        Effect::Outline,
        Effect::Particles,
        Effect::ParticlesTranslucent,
        Effect::ParticlesAdditive,
    ]
    .into_iter()
    .filter(|effect| !DrawKind::Effect(*effect).after_deferred(&pack.effects));
    record_effects(
        encoder,
        state,
        geometry,
        before,
        scene_alt,
        Some((&view, size)),
    );
    // Iris's `depthtex2` holds the depth before the hand, `depthtex1` before translucents.
    let before_hand: &[usize] = if pack.depth_sampled[2] { &[2] } else { &[] };
    copy_view_depth(device, encoder, state, before_hand, &[2]);
    super::actors::record(
        encoder,
        state,
        DrawKind::Hand,
        scene_alt,
        Some((&view, size)),
    );
    // The Vanilla overlay compares against `depthtex1`, so it is always copied.
    copy_view_depth(device, encoder, state, &[1], &[0, 1]);
}

/// Draws translucent terrain,
/// actors and block entities after the deferred passes, the effects Iris draws after them
/// (particles by the pack's ordering, clouds, weather) and the translucent hand, all into
/// the geometry depth `depthtex0` reads.
pub(crate) fn record_translucent(
    device: &Device,
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    geometry: &FrameGeometry<'_>,
) {
    let Some(depth) = state.targets.geometry_depth.as_ref() else {
        return;
    };
    let view = depth.view.clone();
    let size = state.targets.screen_size;
    copy_view_depth(device, encoder, state, &[1], &[0, 1]);
    let translucent_alt = state.pack.frame.translucent_alt;
    record_kind(
        encoder,
        state,
        geometry,
        DrawKind::Translucent,
        translucent_alt,
        Some((&view, size)),
        None,
    );
    for kind in [
        DrawKind::EntitiesTranslucent,
        DrawKind::ArmorGlintTranslucent,
        DrawKind::BlockTranslucent,
        DrawKind::BeaconBeam,
    ] {
        super::actors::record(encoder, state, kind, translucent_alt, Some((&view, size)));
    }
    let pack = std::sync::Arc::clone(&state.pack);
    let after = [
        Effect::Particles,
        Effect::ParticlesTranslucent,
        Effect::ParticlesAdditive,
        Effect::Clouds,
        Effect::Weather,
    ]
    .into_iter()
    .filter(|effect| DrawKind::Effect(*effect).after_deferred(&pack.effects));
    record_effects(
        encoder,
        state,
        geometry,
        after,
        translucent_alt,
        Some((&view, size)),
    );
    for kind in [DrawKind::HandWater, DrawKind::HandGlint] {
        super::actors::record(encoder, state, kind, translucent_alt, Some((&view, size)));
    }
    copy_view_depth(device, encoder, state, &[], &[0]);
}

#[cfg(test)]
mod extent_tests {
    use super::*;
    #[test]
    fn geometry_replacement_requires_matching_attachment_extents() {
        let mut depth = Some([128, 64]);
        assert!(admit_extent(&mut depth, [128, 64]));
        assert!(!admit_extent(&mut depth, [64, 32]));
        let mut colour_only = None;
        assert!(admit_extent(&mut colour_only, [64, 32]));
        assert!(!admit_extent(&mut colour_only, [128, 64]));
    }
}
