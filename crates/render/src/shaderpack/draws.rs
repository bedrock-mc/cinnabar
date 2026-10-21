//! The geometry draws the host records with a pack's `gbuffers_*` and `shadow*` programs:
//! which program each draw uses (Iris's fallback chains), the fixed-function state Iris
//! gives it unless `shaders.properties` overrides it (`alphaTest.*`, `blend.*`), and its
//! pipeline over the geometry adapter's vertex layout.

use super::alpha_test::{ONE_TENTH_ALPHA, inject_alpha_test};
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
    pub(crate) const TERRAIN_SOLID: i32 = 8;
    pub(crate) const TERRAIN_CUTOUT: i32 = 10;
    pub(crate) const TERRAIN_TRANSLUCENT: i32 = 17;
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
    ShadowSolid,
    ShadowCutout,
    ShadowTranslucent,
}

/// Vanilla's translucent blending: `SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA`.
pub(crate) const TRANSLUCENT_BLEND: BlendMode = BlendMode::Factors {
    src_color: BlendFactor::SrcAlpha,
    dst_color: BlendFactor::OneMinusSrcAlpha,
    src_alpha: BlendFactor::One,
    dst_alpha: BlendFactor::OneMinusSrcAlpha,
};

impl DrawKind {
    pub(crate) const ALL: [DrawKind; 8] = [
        DrawKind::Sky,
        DrawKind::Sunset,
        DrawKind::TerrainSolid,
        DrawKind::TerrainCutout,
        DrawKind::Translucent,
        DrawKind::ShadowSolid,
        DrawKind::ShadowCutout,
        DrawKind::ShadowTranslucent,
    ];

    /// The Iris program this draw asks for first.
    pub(crate) fn program(self) -> &'static str {
        match self {
            DrawKind::Sky | DrawKind::Sunset => "gbuffers_skybasic",
            DrawKind::TerrainSolid => "gbuffers_terrain_solid",
            DrawKind::TerrainCutout => "gbuffers_terrain_cutout",
            DrawKind::Translucent => "gbuffers_water",
            DrawKind::ShadowSolid => "shadow_solid",
            DrawKind::ShadowCutout => "shadow_cutout",
            DrawKind::ShadowTranslucent => "shadow_water",
        }
    }

    /// The terrain category it draws; `None` for the sky.
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

    /// Drawn in the shadow pass from the light's view.
    pub(crate) fn shadow(self) -> bool {
        matches!(
            self,
            DrawKind::ShadowSolid | DrawKind::ShadowCutout | DrawKind::ShadowTranslucent
        )
    }

    /// Drawn after the deferred passes, on the side composite passes read.
    pub(crate) fn after_deferred(self) -> bool {
        self == DrawKind::Translucent
    }

    /// Tests depth: everything but the sky, which draws behind everything.
    pub(crate) fn depth(self) -> bool {
        !matches!(self, DrawKind::Sky | DrawKind::Sunset)
    }

    /// Its lightmap coordinates are raw `light × 16` values for `gl_TextureMatrix[1]`.
    pub(crate) fn lightmap_matrix(self) -> bool {
        self.category().is_some()
    }

    /// Culls back faces: camera terrain does, as Java; the sky and shadows draw both sides.
    pub(crate) fn culls(self) -> bool {
        matches!(
            self,
            DrawKind::TerrainSolid | DrawKind::TerrainCutout | DrawKind::Translucent
        )
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
        }
    }

    /// Iris's alpha test without an `alphaTest.<program>` rule: cutout terrain keeps
    /// `alpha > 0.1`, everything else is untested.
    pub(crate) fn default_alpha_test(self) -> Option<AlphaTest> {
        let tested = matches!(self, DrawKind::TerrainCutout | DrawKind::ShadowCutout);
        tested.then_some(ONE_TENTH_ALPHA)
    }

    /// Vanilla's blending without a `blend.<program>` rule.
    pub(crate) fn default_blend(self) -> BlendMode {
        match self {
            DrawKind::Sunset | DrawKind::Translucent | DrawKind::ShadowTranslucent => {
                TRANSLUCENT_BLEND
            }
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

/// The geometry programs the frame's draws use, each once, with the program index of every
/// draw that resolved. Only enabled render programs that translated count as provided, so a
/// failed program falls back like a missing one. Without a camera terrain program nothing is
/// selected: the host then keeps the Vanilla scene copy.
pub(crate) fn select_draws(
    programs: &[shaderpack::ProgramTranslation],
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
    for kind in DrawKind::ALL {
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
    if !complete_terrain(|kind| draws.iter().any(|(draw, _)| *draw == kind)) {
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
    let layouts = vertex_buffer_layouts(
        &reflection.vertex_inputs,
        &draw_layout(kind, inputs.terrain_layout),
    )
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
                depth_write_enabled: Some(kind.depth()),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
    if !complete_terrain(|kind| draws.iter().any(|draw| draw.kind == kind)) {
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
        let (selected, draws) = select_draws(&translation.programs);
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
