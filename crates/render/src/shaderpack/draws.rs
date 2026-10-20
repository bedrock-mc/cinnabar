//! The geometry draws the host records with a pack's `gbuffers_*` and `shadow*` programs:
//! which program each draw uses (Iris's fallback chains), the fixed-function state Iris
//! gives it unless `shaders.properties` overrides it (`alphaTest.*`, `blend.*`), and its
//! pipeline over the geometry adapter's vertex layout.

use super::alpha_test::{ONE_TENTH_ALPHA, inject_alpha_test};
use super::effects::{Effect, EffectSettings};
use super::geometry::{GeometryCategory, TerrainLayout, vertex_buffer_layouts};
use super::loader::GpuProgram;
use super::pipelines::GpuContext;
use super::pipelines::{blend_state, color_target};
use super::plan::MAX_COLOR_BUFFERS;
use super::resources::ShadowSlots;
use shaderpack::{
    AlphaTest, AlphaTestRule, BlendFactor, BlendMode, BufferId, ProgramReflection,
    ShaderProperties, StageKind, geometry_fallback,
};
use std::borrow::Cow;
use wgpu::TextureFormat;

/// Iris's `renderStage` values (`MC_RENDER_STAGE_*`) of the stages the host draws.
pub(super) mod render_stage {
    pub(crate) const SKY: i32 = 1;
    pub(crate) const SUNSET: i32 = 2;
    pub(crate) const SUN: i32 = 4;
    pub(crate) const MOON: i32 = 5;
    pub(crate) const STARS: i32 = 6;
    pub(crate) const TERRAIN_SOLID: i32 = 8;
    pub(crate) const TERRAIN_CUTOUT: i32 = 10;
    pub(crate) const ENTITIES: i32 = 11;
    pub(crate) const BLOCK_ENTITIES: i32 = 12;
    pub(crate) const DESTROY: i32 = 13;
    pub(crate) const OUTLINE: i32 = 14;
    pub(crate) const HAND_SOLID: i32 = 16;
    pub(crate) const TERRAIN_TRANSLUCENT: i32 = 17;
    pub(crate) const PARTICLES: i32 = 19;
    pub(crate) const CLOUDS: i32 = 20;
    pub(crate) const RAIN_SNOW: i32 = 21;
    pub(crate) const HAND_TRANSLUCENT: i32 = 23;
}

/// One geometry draw of the frame, in recording order within its pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DrawKind {
    /// The sky discs (`gbuffers_skybasic`).
    Sky,
    /// The sunrise/sunset fan (`gbuffers_skybasic`, blended).
    Sunset,
    TerrainSolid,
    TerrainCutout,
    /// Translucent terrain (`gbuffers_water`), drawn after the deferred passes.
    Translucent,
    /// Opaque and cutout actors (`gbuffers_entities`), after opaque terrain.
    Entities,
    /// Blended actors (`gbuffers_entities_translucent`), after the deferred passes.
    EntitiesTranslucent,
    /// Additive emissive actor layers (`gbuffers_spidereyes`).
    SpiderEyes,
    /// The enchantment foil over glinted actors (`gbuffers_armor_glint`).
    ArmorGlint,
    /// Foil after blended world actors.
    ArmorGlintTranslucent,
    /// Foil in hand space, after first-person base geometry.
    HandGlint,
    /// Block entities (`gbuffers_block`).
    Block,
    /// Blended block-entity layers (`gbuffers_block_translucent`), after the deferred passes.
    BlockTranslucent,
    /// The beacon beam (`gbuffers_beaconbeam`), after the deferred passes.
    BeaconBeam,
    /// The first-person hand (`gbuffers_hand`), after the `depthtex2` copy.
    Hand,
    /// Blended first-person geometry (`gbuffers_hand_water`), last before composite.
    HandWater,
    ShadowSolid,
    ShadowCutout,
    /// Actors casting shadows (`shadow_entities`).
    ShadowEntities,
    /// Block entities casting shadows (`shadow_block`).
    ShadowBlock,
    ShadowTranslucent,
    /// Particles, weather, clouds, the sun and moon and the other effects.
    Effect(Effect),
}

/// Vanilla's translucent blending: `SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA`.
pub(crate) const TRANSLUCENT_BLEND: BlendMode = BlendMode::Factors {
    src_color: BlendFactor::SrcAlpha,
    dst_color: BlendFactor::OneMinusSrcAlpha,
    src_alpha: BlendFactor::One,
    dst_alpha: BlendFactor::OneMinusSrcAlpha,
};

/// Vanilla's `ADDITIVE_TRANSPARENCY` (`ONE ONE`), which the eyes layers draw with.
pub(crate) const ADDITIVE_BLEND: BlendMode = BlendMode::Factors {
    src_color: BlendFactor::One,
    dst_color: BlendFactor::One,
    src_alpha: BlendFactor::One,
    dst_alpha: BlendFactor::One,
};

/// Vanilla's `GLINT_TRANSPARENCY`: `SRC_COLOR ONE ZERO ONE`.
pub(crate) const GLINT_BLEND: BlendMode = BlendMode::Factors {
    src_color: BlendFactor::SrcColor,
    dst_color: BlendFactor::One,
    src_alpha: BlendFactor::Zero,
    dst_alpha: BlendFactor::One,
};

impl DrawKind {
    pub(crate) const ALL: [DrawKind; 32] = [
        DrawKind::Sky,
        DrawKind::Sunset,
        DrawKind::Effect(Effect::Sun),
        DrawKind::Effect(Effect::Moon),
        DrawKind::Effect(Effect::Stars),
        DrawKind::TerrainSolid,
        DrawKind::TerrainCutout,
        DrawKind::Effect(Effect::Lightning),
        DrawKind::Effect(Effect::DamagedBlock),
        DrawKind::Effect(Effect::Outline),
        DrawKind::Effect(Effect::Particles),
        DrawKind::Translucent,
        DrawKind::Effect(Effect::ParticlesTranslucent),
        DrawKind::Effect(Effect::ParticlesAdditive),
        DrawKind::Effect(Effect::Clouds),
        DrawKind::Effect(Effect::Weather),
        DrawKind::Entities,
        DrawKind::EntitiesTranslucent,
        DrawKind::SpiderEyes,
        DrawKind::ArmorGlint,
        DrawKind::ArmorGlintTranslucent,
        DrawKind::HandGlint,
        DrawKind::Block,
        DrawKind::BlockTranslucent,
        DrawKind::BeaconBeam,
        DrawKind::Hand,
        DrawKind::HandWater,
        DrawKind::ShadowSolid,
        DrawKind::ShadowCutout,
        DrawKind::ShadowEntities,
        DrawKind::ShadowBlock,
        DrawKind::ShadowTranslucent,
    ];

    /// The Iris program this draw asks for first.
    pub(crate) fn program(self) -> &'static str {
        match self {
            DrawKind::Sky | DrawKind::Sunset => "gbuffers_skybasic",
            DrawKind::TerrainSolid => "gbuffers_terrain_solid",
            DrawKind::TerrainCutout => "gbuffers_terrain_cutout",
            DrawKind::Translucent => "gbuffers_water",
            DrawKind::Entities => "gbuffers_entities",
            DrawKind::EntitiesTranslucent => "gbuffers_entities_translucent",
            DrawKind::SpiderEyes => "gbuffers_spidereyes",
            DrawKind::ArmorGlint | DrawKind::ArmorGlintTranslucent | DrawKind::HandGlint => {
                "gbuffers_armor_glint"
            }
            DrawKind::Block => "gbuffers_block",
            DrawKind::BlockTranslucent => "gbuffers_block_translucent",
            DrawKind::BeaconBeam => "gbuffers_beaconbeam",
            DrawKind::Hand => "gbuffers_hand",
            DrawKind::HandWater => "gbuffers_hand_water",
            DrawKind::ShadowSolid => "shadow_solid",
            DrawKind::ShadowCutout => "shadow_cutout",
            DrawKind::ShadowEntities => "shadow_entities",
            DrawKind::ShadowBlock => "shadow_block",
            DrawKind::ShadowTranslucent => "shadow_water",
            DrawKind::Effect(effect) => effect.program(),
        }
    }

    /// Whether the pack's directives let this draw happen (`clouds=off`, `sun=false`, …).
    pub(crate) fn drawn_by(self, settings: &EffectSettings) -> bool {
        match self {
            DrawKind::Effect(effect) => settings.draws(effect),
            _ => true,
        }
    }

    /// The terrain category it draws; `None` for the sky, effects and actors.
    pub(crate) fn category(self) -> Option<GeometryCategory> {
        match self {
            DrawKind::TerrainSolid | DrawKind::ShadowSolid => Some(GeometryCategory::Solid),
            DrawKind::TerrainCutout | DrawKind::ShadowCutout => Some(GeometryCategory::Cutout),
            DrawKind::Translucent | DrawKind::ShadowTranslucent => {
                Some(GeometryCategory::Translucent)
            }
            _ => None,
        }
    }

    /// Draws actors, block entities or the hand through [`super::actors`].
    pub(crate) fn actor(self) -> bool {
        !matches!(self, DrawKind::Sky | DrawKind::Sunset | DrawKind::Effect(_))
            && self.category().is_none()
    }

    /// Drawn in the shadow pass from the light's view.
    pub(crate) fn shadow(self) -> bool {
        matches!(
            self,
            DrawKind::ShadowSolid
                | DrawKind::ShadowCutout
                | DrawKind::ShadowEntities
                | DrawKind::ShadowBlock
                | DrawKind::ShadowTranslucent
        )
    }

    /// Drawn after the deferred passes, on the side composite passes read.
    pub(crate) fn after_deferred(self) -> bool {
        matches!(
            self,
            DrawKind::Translucent
                | DrawKind::EntitiesTranslucent
                | DrawKind::BlockTranslucent
                | DrawKind::BeaconBeam
                | DrawKind::HandWater
                | DrawKind::HandGlint
                | DrawKind::ArmorGlintTranslucent
        )
    }

    /// The first-person hand, drawn in hand space with the hand projection.
    pub(crate) fn hand(self) -> bool {
        matches!(
            self,
            DrawKind::Hand | DrawKind::HandWater | DrawKind::HandGlint
        )
    }

    /// Tests depth: everything but the sky and the sky effects, which draw behind everything.
    pub(crate) fn depth(self) -> bool {
        match self {
            DrawKind::Effect(effect) => !effect.sky(),
            _ => !matches!(self, DrawKind::Sky | DrawKind::Sunset),
        }
    }

    /// Writes depth when it tests it, except effects Java draws without depth writes and
    /// Vanilla's no-depth-write layers (eyes, glint and the beacon beam's translucent shell).
    pub(crate) fn depth_write(self, settings: &EffectSettings) -> bool {
        match self {
            DrawKind::Effect(effect) => effect.depth_write(settings.rain_depth),
            _ => {
                self.depth()
                    && !matches!(
                        self,
                        DrawKind::SpiderEyes
                            | DrawKind::ArmorGlint
                            | DrawKind::ArmorGlintTranslucent
                            | DrawKind::HandGlint
                            | DrawKind::BeaconBeam
                    )
            }
        }
    }

    /// Foil only covers samples whose base geometry wrote the same depth.
    pub(crate) fn depth_compare(self) -> wgpu::CompareFunction {
        if matches!(
            self,
            Self::ArmorGlint | Self::ArmorGlintTranslucent | Self::HandGlint
        ) {
            wgpu::CompareFunction::Equal
        } else {
            wgpu::CompareFunction::LessEqual
        }
    }

    /// Its lightmap coordinates are raw `light × 16` values for `gl_TextureMatrix[1]`.
    pub(crate) fn lightmap_matrix(self) -> bool {
        matches!(self, DrawKind::Effect(_)) || self.category().is_some() || self.actor()
    }

    /// Culls back faces: camera terrain, actors and clouds do, as Java (actor two-sided faces
    /// carry explicit back faces); the sky, block entities (whose meshes mix one- and
    /// two-sided quads of either winding), shadows and other effects draw both sides.
    pub(crate) fn culls(self) -> bool {
        match self {
            DrawKind::Effect(effect) => effect.culls(),
            _ => matches!(
                self,
                DrawKind::TerrainSolid
                    | DrawKind::TerrainCutout
                    | DrawKind::Translucent
                    | DrawKind::Entities
                    | DrawKind::EntitiesTranslucent
                    | DrawKind::SpiderEyes
                    | DrawKind::ArmorGlint
                    | DrawKind::ArmorGlintTranslucent
                    | DrawKind::HandGlint
                    | DrawKind::Hand
                    | DrawKind::HandWater
            ),
        }
    }

    /// Iris's `renderStage` during the draw.
    pub(crate) fn render_stage(self) -> i32 {
        match self {
            DrawKind::Sky => render_stage::SKY,
            DrawKind::Sunset => render_stage::SUNSET,
            DrawKind::TerrainSolid | DrawKind::ShadowSolid => render_stage::TERRAIN_SOLID,
            DrawKind::TerrainCutout | DrawKind::ShadowCutout => render_stage::TERRAIN_CUTOUT,
            DrawKind::Translucent | DrawKind::ShadowTranslucent => {
                render_stage::TERRAIN_TRANSLUCENT
            }
            DrawKind::Entities
            | DrawKind::EntitiesTranslucent
            | DrawKind::SpiderEyes
            | DrawKind::ArmorGlint
            | DrawKind::ArmorGlintTranslucent
            | DrawKind::ShadowEntities => render_stage::ENTITIES,
            DrawKind::Block
            | DrawKind::BlockTranslucent
            | DrawKind::BeaconBeam
            | DrawKind::ShadowBlock => render_stage::BLOCK_ENTITIES,
            DrawKind::Hand | DrawKind::HandGlint => render_stage::HAND_SOLID,
            DrawKind::HandWater => render_stage::HAND_TRANSLUCENT,
            DrawKind::Effect(effect) => effect.render_stage(),
        }
    }

    /// Iris's alpha test without an `alphaTest.<program>` rule: cutout terrain, every actor
    /// program but the beacon beam and the effects whose shader key tests keep `alpha > 0.1`,
    /// everything else is untested.
    pub(crate) fn default_alpha_test(self) -> Option<AlphaTest> {
        if let DrawKind::Effect(effect) = self {
            return effect.default_alpha_test();
        }
        let tested = matches!(self, DrawKind::TerrainCutout | DrawKind::ShadowCutout)
            || (self.actor() && self != DrawKind::BeaconBeam);
        tested.then_some(ONE_TENTH_ALPHA)
    }

    /// Vanilla's blending without a `blend.<program>` rule.
    pub(crate) fn default_blend(self) -> BlendMode {
        match self {
            DrawKind::Sunset
            | DrawKind::Translucent
            | DrawKind::ShadowTranslucent
            | DrawKind::EntitiesTranslucent
            | DrawKind::BlockTranslucent
            | DrawKind::BeaconBeam
            | DrawKind::HandWater => TRANSLUCENT_BLEND,
            DrawKind::SpiderEyes => ADDITIVE_BLEND,
            DrawKind::ArmorGlint | DrawKind::ArmorGlintTranslucent | DrawKind::HandGlint => {
                GLINT_BLEND
            }
            DrawKind::Effect(effect) => effect.default_blend(),
            _ => BlendMode::Off,
        }
    }

    /// The alpha test of this draw with `program`: the last `alphaTest.<program>` rule, else
    /// the default.
    pub(crate) fn alpha_test(self, program: &str, rules: &[AlphaTestRule]) -> Option<AlphaTest> {
        rules
            .iter()
            .rev()
            .find(|rule| rule.program == program)
            .map_or(self.default_alpha_test(), |rule| rule.test)
    }
}

/// The program Iris runs for `requested`: the first of it and its fallbacks
/// (`gbuffers_terrain_solid` → `gbuffers_terrain` → `gbuffers_textured_lit` →
/// `gbuffers_textured` → `gbuffers_basic`) that `provided` reports, or `None` when the chain
/// ends without one.
pub(crate) fn resolve_program(
    requested: &'static str,
    provided: impl Fn(&str) -> bool,
) -> Option<&'static str> {
    let mut name = Some(requested);
    while let Some(current) = name {
        if provided(current) {
            return Some(current);
        }
        name = geometry_fallback(current);
    }
    None
}

/// Whether every camera terrain category has a usable draw.
pub(crate) fn complete_terrain(mut draws: impl FnMut(DrawKind) -> bool) -> bool {
    GeometryCategory::ALL.into_iter().all(|category| {
        let kind = match category {
            GeometryCategory::Solid => DrawKind::TerrainSolid,
            GeometryCategory::Cutout => DrawKind::TerrainCutout,
            GeometryCategory::Translucent => DrawKind::Translucent,
        };
        draws(kind)
    })
}

/// Requires every camera terrain, actor, block-entity and hand draw before replacing the scene.
pub(crate) fn complete_world(mut draws: impl FnMut(DrawKind) -> bool) -> bool {
    complete_terrain(&mut draws)
        && DrawKind::ALL
            .into_iter()
            .filter(|kind| kind.actor() && !kind.shadow())
            .all(draws)
}

/// The geometry programs the frame's draws use, each once, with the program index of every
/// draw that resolved. Only enabled render programs that translated count as provided, so a
/// failed program falls back like a missing one; draws `wanted` rejects (directives such as
/// `clouds=off`) are left out. Without a camera terrain program nothing is selected: the
/// host then keeps the Vanilla scene copy.
pub(crate) fn select_draws(
    programs: &[shaderpack::ProgramTranslation],
    wanted: impl Fn(DrawKind) -> bool,
) -> (Vec<&shaderpack::ProgramTranslation>, Vec<(DrawKind, usize)>) {
    let find = |name: &str| {
        programs.iter().find(|p| {
            p.program.name == name
                && p.enabled
                && p.reflection.is_some()
                && p.program.kind == shaderpack::ProgramKind::Render
        })
    };
    let mut selected: Vec<&shaderpack::ProgramTranslation> = vec![];
    let mut draws = vec![];
    for kind in DrawKind::ALL.into_iter().filter(|kind| wanted(*kind)) {
        let Some(program) = resolve_program(kind.program(), |n| find(n).is_some()).and_then(find)
        else {
            continue;
        };
        let index = match selected.iter().position(|p| std::ptr::eq(*p, program)) {
            Some(index) => index,
            None => {
                selected.push(program);
                selected.len() - 1
            }
        };
        draws.push((kind, index));
    }
    if !complete_world(|kind| draws.iter().any(|(draw, _)| *draw == kind)) {
        return (vec![], vec![]);
    }
    (selected, draws)
}

/// One geometry draw ready to record.
pub(crate) struct GeometryDraw {
    pub kind: DrawKind,
    /// Index of its program in `PackGpu::programs`.
    pub program: usize,
    pub pipeline: wgpu::RenderPipeline,
    /// Per fragment output location: the `colortex` (shadow pass: `shadowcolor`) index it
    /// writes, when the pipeline has a target there.
    pub outputs: Vec<Option<u32>>,
    /// The expanded vertex an expanded terrain draw's pipeline reads; it draws only while
    /// the terrain pages hold that layout.
    pub layout: Option<TerrainLayout>,
}

/// A geometry program's pipeline pieces, from which its draws' pipelines are built.
pub(crate) struct GeometryStage {
    /// Index of the program in `PackGpu::programs`.
    pub program: usize,
    pub layout: wgpu::PipelineLayout,
    pub vertex: wgpu::ShaderModule,
    pub fragment: naga::Module,
}

/// What building one draw's pipeline needs from its program.
pub(crate) struct DrawPipelineInputs<'a> {
    pub device: &'a wgpu::Device,
    pub cache: Option<&'a wgpu::PipelineCache>,
    pub layout: &'a wgpu::PipelineLayout,
    /// The resolved program's name, which `alphaTest.*` and `blend.*` rules name.
    pub name: &'a str,
    pub reflection: &'a ProgramReflection,
    /// The program's vertex stage module.
    pub vertex: &'a wgpu::ShaderModule,
    /// The program's translated fragment stage, which the alpha test is injected into.
    pub fragment: &'a naga::Module,
    pub properties: &'a ShaderProperties,
    /// The pack's effect directives (`rain.depth`).
    pub effects: &'a EffectSettings,
    /// Format of the target an output's draw buffer names (`colortexN`, or `shadowcolorN`
    /// for shadow draws); `None` drops the output.
    pub target_format: &'a dyn Fn(u32) -> Option<TextureFormat>,
    /// The expanded vertex terrain draws read; other draws read [`TerrainLayout::WIDE`].
    pub terrain_layout: TerrainLayout,
}

/// The draw buffer a `blend.<program>.<buffer>` rule names, in the draw's buffer space.
fn rule_buffer(name: &str, shadow: bool) -> Option<u32> {
    match (BufferId::parse(name)?, shadow) {
        (BufferId::Color(i), false) | (BufferId::ShadowColor(i), true) => Some(i),
        _ => None,
    }
}

/// The vertex layout `kind` reads: the pack's terrain layout for terrain, the wide stream the
/// host builds for everything else.
fn draw_layout(kind: DrawKind, terrain: TerrainLayout) -> TerrainLayout {
    if kind.category().is_some() {
        terrain
    } else {
        TerrainLayout::WIDE
    }
}

/// Builds the pipeline of `kind` with one program: the geometry vertex layout, Iris's
/// culling and depth state (geometry winds counter-clockwise; the translator's clip-space
/// Y flip mirrors it to clockwise), the alpha test and the blend state per draw buffer.
pub(crate) fn draw_pipeline(
    kind: DrawKind,
    inputs: &DrawPipelineInputs<'_>,
) -> Result<(wgpu::RenderPipeline, Vec<Option<u32>>), String> {
    let reflection = inputs.reflection;
    let layouts = if kind.actor() {
        super::actors::layout::vertex_layouts(&reflection.vertex_inputs)
    } else {
        vertex_buffer_layouts(
            &reflection.vertex_inputs,
            &draw_layout(kind, inputs.terrain_layout),
        )
    }
    .map_err(|e| e.to_string())?;
    let buffers: Vec<_> = layouts
        .iter()
        .map(|layout| {
            Some(wgpu::VertexBufferLayout {
                array_stride: layout.array_stride,
                step_mode: layout.step_mode,
                attributes: &layout.attributes,
            })
        })
        .collect();
    let rules = &inputs.properties.blends;
    let program_blend = rules
        .iter()
        .rev()
        .find(|b| b.program == inputs.name && b.buffer.is_none())
        .map(|b| b.mode);
    let len = reflection
        .color_outputs
        .iter()
        .map(|o| o.location as usize + 1)
        .max()
        .unwrap_or(0);
    let mut targets = vec![None; len];
    let mut outputs = vec![None; len];
    for output in &reflection.color_outputs {
        let Some(buffer) = output.buffer else {
            continue;
        };
        let Some(format) = (inputs.target_format)(buffer) else {
            continue;
        };
        let buffer_blend = rules
            .iter()
            .rev()
            .find(|r| {
                r.program == inputs.name
                    && r.buffer
                        .as_deref()
                        .and_then(|b| rule_buffer(b, kind.shadow()))
                        == Some(buffer)
            })
            .map(|r| r.mode);
        let mode = buffer_blend
            .or(program_blend)
            .unwrap_or(kind.default_blend());
        let target = color_target(format, output.ty, blend_state(mode));
        if target.is_some() {
            outputs[output.location as usize] = Some(buffer);
        }
        targets[output.location as usize] = target;
    }
    if !kind.depth() && targets.iter().all(Option::is_none) {
        return Err("it writes no buffer the frame has".into());
    }
    let alpha = kind.alpha_test(inputs.name, &inputs.properties.alpha_tests);
    let tested = alpha.and_then(|test| {
        let mut module = inputs.fragment.clone();
        inject_alpha_test(&mut module, test).then_some(module)
    });
    let fragment = inputs
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("shader pack {} {kind:?} fsh", inputs.name)),
            source: wgpu::ShaderSource::Naga(Cow::Owned(
                tested.unwrap_or_else(|| inputs.fragment.clone()),
            )),
        });
    let entry = |stage: StageKind| {
        reflection
            .entry_points
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, e)| e.as_str())
    };
    let label = format!("shader pack {} {kind:?}", inputs.name);
    let pipeline = inputs
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(inputs.layout),
            vertex: wgpu::VertexState {
                module: inputs.vertex,
                entry_point: entry(StageKind::Vertex),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: kind.culls().then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: kind.depth().then_some(wgpu::DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(kind.depth_write(inputs.effects)),
                depth_compare: Some(kind.depth_compare()),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: entry(StageKind::Fragment),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: inputs.cache,
        });
    Ok((pipeline, outputs))
}

/// Builds every resolved draw's pipeline. Terrain draws read a [`TerrainLayout::compact`]
/// vertex holding what any terrain program reads. Draws whose program cannot bind terrain
/// vertices, or whose pipeline the device rejects, are dropped (logged); without a solid
/// terrain draw none remain, so the Vanilla scene copy stays in place.
#[allow(
    clippy::too_many_arguments,
    reason = "the pack-wide plan pieces a draw pipeline draws on"
)]
pub(crate) fn geometry_draws(
    gpu: &GpuContext,
    cache: Option<&wgpu::PipelineCache>,
    selected: &[(DrawKind, usize)],
    stages: &[GeometryStage],
    programs: &[GpuProgram],
    properties: &shaderpack::ShaderProperties,
    effects: &EffectSettings,
    buffer_formats: &[Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    shadow: &ShadowSlots,
) -> Vec<GeometryDraw> {
    let terrain_layout = TerrainLayout::compact(
        selected
            .iter()
            .filter(|(kind, _)| kind.category().is_some())
            .flat_map(|(_, selected)| {
                &programs[stages[*selected].program].reflection.vertex_inputs
            }),
    );
    bevy::log::debug!(
        target: "cinnabar::shaderpack",
        bytes = terrain_layout.stride(),
        "shader-pack terrain vertex"
    );
    let mut draws = vec![];
    for (kind, selected) in selected {
        let stage = &stages[*selected];
        let program = &programs[stage.program];
        let target_format = |buffer: u32| -> Option<TextureFormat> {
            if kind.shadow() {
                shadow
                    .color
                    .iter()
                    .find(|(i, ..)| *i == buffer)
                    .map(|(.., format)| *format)
            } else {
                buffer_formats.get(buffer as usize).copied().flatten()
            }
        };
        let inputs = DrawPipelineInputs {
            device: &gpu.device,
            cache,
            layout: &stage.layout,
            name: &program.name,
            reflection: &program.reflection,
            vertex: &stage.vertex,
            fragment: &stage.fragment,
            properties,
            effects,
            target_format: &target_format,
            terrain_layout,
        };
        // Each draw gets its own error scope: a pipeline the device rejects drops only that
        // draw, as Iris falls back per program, instead of the pack-wide scope failing.
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let built = draw_pipeline(*kind, &inputs);
        let rejected = bevy::tasks::block_on(scope.pop());
        match (built, rejected) {
            (Ok((pipeline, outputs)), None) => draws.push(GeometryDraw {
                kind: *kind,
                program: stage.program,
                pipeline,
                outputs,
                layout: kind.category().is_some().then_some(terrain_layout),
            }),
            (Err(error), _) => bevy::log::warn!(
                target: "cinnabar::shaderpack",
                program = %program.name, draw = ?kind,
                "draw skipped: {error}"
            ),
            (Ok(_), Some(error)) => bevy::log::warn!(
                target: "cinnabar::shaderpack",
                program = %program.name, draw = ?kind,
                "draw skipped: the device rejected its pipeline: {error}"
            ),
        }
    }
    if !complete_world(|kind| draws.iter().any(|draw| draw.kind == kind)) {
        draws.clear();
    }
    draws
}

#[cfg(test)]
mod tests {
    use super::*;
    use shaderpack::CompareFunction;

    fn resolve(kind: DrawKind, provided: &[&str]) -> Option<&'static str> {
        resolve_program(kind.program(), |name| provided.contains(&name))
    }

    #[test]
    fn partial_actor_and_hand_programs_preserve_the_native_scene() {
        let mut files = Vec::new();
        for name in ["gbuffers_terrain", "gbuffers_entities", "gbuffers_hand"] {
            files.push((
                format!("{name}.vsh"),
                "#version 120\nvoid main() { gl_Position = gl_Vertex; }".to_owned(),
            ));
            files.push((
                format!("{name}.fsh"),
                "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }".to_owned(),
            ));
        }
        let pack = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files(files));
        let translation = pack.translate(&shaderpack::TranslateConfig::default());
        assert!(translation.programs.iter().all(|program| program.is_ok()));
        let (_, draws) = select_draws(&translation.programs, |_| true);
        assert!(draws.is_empty());
    }

    #[test]
    fn a_solid_only_pack_preserves_the_native_scene_copy() {
        let pack = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
            (
                "gbuffers_terrain_solid.vsh",
                "#version 120\nvoid main() { gl_Position = gl_Vertex; }",
            ),
            (
                "gbuffers_terrain_solid.fsh",
                "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }",
            ),
        ]));
        let translation = pack.translate(&shaderpack::TranslateConfig::default());
        assert!(translation.programs.iter().all(|program| program.is_ok()));
        let (selected, draws) = select_draws(&translation.programs, |_| true);
        assert!(selected.is_empty());
        assert!(draws.is_empty());
    }

    #[test]
    fn terrain_falls_back_through_textured_lit_to_basic() {
        let bsl = [
            "gbuffers_basic",
            "gbuffers_textured",
            "gbuffers_terrain",
            "gbuffers_water",
            "gbuffers_skybasic",
        ];
        assert_eq!(
            resolve(DrawKind::TerrainSolid, &bsl),
            Some("gbuffers_terrain")
        );
        assert_eq!(
            resolve(DrawKind::TerrainCutout, &bsl),
            Some("gbuffers_terrain")
        );
        assert_eq!(resolve(DrawKind::Translucent, &bsl), Some("gbuffers_water"));
        assert_eq!(resolve(DrawKind::Sky, &bsl), Some("gbuffers_skybasic"));
        let textured = ["gbuffers_basic", "gbuffers_textured"];
        assert_eq!(
            resolve(DrawKind::Translucent, &textured),
            Some("gbuffers_textured"),
            "water → terrain → textured_lit → textured"
        );
        assert_eq!(resolve(DrawKind::Sky, &textured), Some("gbuffers_basic"));
        let photon = ["gbuffers_terrain", "gbuffers_terrain_solid"];
        assert_eq!(
            resolve(DrawKind::TerrainSolid, &photon),
            Some("gbuffers_terrain_solid")
        );
        assert_eq!(
            resolve(DrawKind::TerrainCutout, &photon),
            Some("gbuffers_terrain")
        );
        assert_eq!(resolve(DrawKind::TerrainSolid, &[]), None);
    }

    #[test]
    fn shadow_variants_fall_back_to_shadow() {
        assert_eq!(resolve(DrawKind::ShadowCutout, &["shadow"]), Some("shadow"));
        assert_eq!(
            resolve(DrawKind::ShadowTranslucent, &["shadow", "shadow_water"]),
            Some("shadow_water")
        );
        assert_eq!(resolve(DrawKind::ShadowSolid, &["gbuffers_basic"]), None);
    }

    #[test]
    fn actor_draws_follow_iris_fallback_chains() {
        let generic = [
            "gbuffers_basic",
            "gbuffers_textured",
            "gbuffers_textured_lit",
        ];
        for (kind, expected) in [
            (DrawKind::Entities, "gbuffers_textured_lit"),
            (DrawKind::EntitiesTranslucent, "gbuffers_textured_lit"),
            (DrawKind::SpiderEyes, "gbuffers_textured"),
            (DrawKind::ArmorGlint, "gbuffers_textured"),
            (DrawKind::Block, "gbuffers_textured_lit"),
            (DrawKind::BlockTranslucent, "gbuffers_textured_lit"),
            (DrawKind::BeaconBeam, "gbuffers_textured"),
            (DrawKind::Hand, "gbuffers_textured_lit"),
            (DrawKind::HandWater, "gbuffers_textured_lit"),
        ] {
            assert_eq!(resolve(kind, &generic), Some(expected), "{kind:?}");
        }
        let specific = [
            "gbuffers_terrain",
            "gbuffers_entities",
            "gbuffers_block",
            "gbuffers_hand",
            "shadow",
        ];
        assert_eq!(
            resolve(DrawKind::EntitiesTranslucent, &specific),
            Some("gbuffers_entities"),
            "entities_translucent → entities"
        );
        assert_eq!(
            resolve(DrawKind::BlockTranslucent, &specific),
            Some("gbuffers_block"),
            "block_translucent → block"
        );
        assert_eq!(
            resolve(DrawKind::HandWater, &specific),
            Some("gbuffers_hand"),
            "hand_water → hand"
        );
        assert_eq!(
            resolve(DrawKind::Block, &["gbuffers_terrain"]),
            Some("gbuffers_terrain"),
            "block → terrain"
        );
        assert_eq!(resolve(DrawKind::ArmorGlint, &specific), None);
        assert_eq!(resolve(DrawKind::ShadowEntities, &specific), Some("shadow"));
        assert_eq!(resolve(DrawKind::ShadowBlock, &specific), Some("shadow"));
        assert_eq!(
            resolve(DrawKind::ShadowEntities, &["shadow", "shadow_entities"]),
            Some("shadow_entities")
        );
    }

    #[test]
    fn actor_draws_take_vanillas_render_type_state() {
        assert_eq!(DrawKind::Entities.default_blend(), BlendMode::Off);
        assert_eq!(
            DrawKind::EntitiesTranslucent.default_blend(),
            TRANSLUCENT_BLEND
        );
        assert_eq!(DrawKind::SpiderEyes.default_blend(), ADDITIVE_BLEND);
        assert_eq!(DrawKind::ArmorGlint.default_blend(), GLINT_BLEND);
        let settings = EffectSettings::default();
        assert!(DrawKind::Entities.depth_write(&settings) && DrawKind::Hand.depth_write(&settings));
        assert!(
            !DrawKind::ArmorGlint.depth_write(&settings)
                && !DrawKind::SpiderEyes.depth_write(&settings)
        );
        assert!(DrawKind::ArmorGlint.depth() && !DrawKind::Sky.depth());
        assert_eq!(
            DrawKind::Entities.alpha_test("gbuffers_entities", &[]),
            Some(ONE_TENTH_ALPHA)
        );
        assert_eq!(
            DrawKind::BeaconBeam.alpha_test("gbuffers_beaconbeam", &[]),
            None
        );
        assert_eq!(DrawKind::Entities.render_stage(), 11);
        assert_eq!(DrawKind::Block.render_stage(), 12);
        assert_eq!(DrawKind::Hand.render_stage(), 16);
        assert_eq!(DrawKind::HandWater.render_stage(), 23);
        let after: Vec<_> = DrawKind::ALL
            .into_iter()
            .filter(|k| k.after_deferred())
            .collect();
        assert_eq!(
            after,
            [
                DrawKind::Translucent,
                DrawKind::EntitiesTranslucent,
                DrawKind::BlockTranslucent,
                DrawKind::BeaconBeam,
                DrawKind::HandWater
            ]
        );
    }

    #[test]
    fn alpha_tests_default_by_kind_and_follow_the_resolved_programs_rule() {
        assert_eq!(
            DrawKind::TerrainSolid.alpha_test("gbuffers_terrain", &[]),
            None
        );
        assert_eq!(
            DrawKind::TerrainCutout.alpha_test("gbuffers_terrain", &[]),
            Some(ONE_TENTH_ALPHA)
        );
        let half = AlphaTest {
            function: CompareFunction::GreaterEqual,
            reference: 0.5,
        };
        let rules = [
            AlphaTestRule {
                program: "gbuffers_terrain".into(),
                test: Some(half),
            },
            AlphaTestRule {
                program: "gbuffers_water".into(),
                test: None,
            },
        ];
        assert_eq!(
            DrawKind::TerrainSolid.alpha_test("gbuffers_terrain", &rules),
            Some(half)
        );
        assert_eq!(
            DrawKind::Translucent.alpha_test("gbuffers_water", &rules),
            None
        );
        assert_eq!(DrawKind::Translucent.default_blend(), TRANSLUCENT_BLEND);
        assert_eq!(DrawKind::TerrainSolid.default_blend(), BlendMode::Off);
    }
}
