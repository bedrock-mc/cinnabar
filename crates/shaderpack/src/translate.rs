//! One program through include expansion, preprocessing, transform, naga and reflection.

use crate::compile::{CompiledStage, OutputMap, compile};
use crate::diagnostics::{Diagnostic, Phase};
use crate::options::{OptionKind, OptionValues, PackOption};
use crate::preprocess::{ConstDirective, expand, preprocess};
use crate::programs::{ProgramKind, ProgramSource, StageKind};
use crate::properties::ProgramRule;
use crate::reflect::{StageNames, reflect};
use crate::reflection::ProgramReflection;
use crate::source::PackSource;
use crate::transform::{
    ProgramInterface, ResourceNames, StageDraft, assemble, merge_uniforms, transform,
};
use std::collections::{BTreeMap, HashMap};

/// The outcome for one stage file.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StageTranslation {
    pub stage: StageKind,
    /// `shaders/`-relative path of the stage file.
    pub path: String,
    pub result: Result<CompiledStage, Vec<Diagnostic>>,
}

/// The outcome for one program. A failing program never affects the others.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ProgramTranslation {
    pub program: ProgramSource,
    /// The pack's `program.*.enabled` rule holds for the configured options (true without a rule).
    pub enabled: bool,
    /// For a disabled program translated in force mode: the option values and macros that
    /// made its rule hold, as (name, value).
    pub forced: Vec<(String, String)>,
    /// Per stage file, in pipeline order; empty for a disabled program that was not forced.
    pub stages: Vec<StageTranslation>,
    /// Program-level problems: missing stages, stages that disagree on the interface.
    pub diagnostics: Vec<Diagnostic>,
    /// The binding contract; present when every stage compiled and the stages agree.
    pub reflection: Option<ProgramReflection>,
    /// Buffer index per fragment output location, from the last active RENDERTARGETS or
    /// DRAWBUFFERS directive (fragment stage first, then vertex), else `[0]` as in Iris.
    pub draw_buffers: Vec<u32>,
    /// Active `const` directives of every stage, in stage order.
    pub consts: Vec<ConstDirective>,
}

impl ProgramTranslation {
    /// Whether the program is ready to bind.
    pub fn is_ok(&self) -> bool {
        self.reflection.is_some()
    }

    /// Stage and program diagnostics together.
    pub fn all_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.stages
            .iter()
            .filter_map(|s| s.result.as_ref().err())
            .flatten()
            .chain(&self.diagnostics)
    }
}

/// Inputs shared by every program of one translation.
pub(crate) struct Context<'a> {
    pub source: &'a PackSource,
    pub options: &'a [PackOption],
    pub values: &'a OptionValues,
    /// Standard macros plus extra (forced) macros.
    pub defines: &'a [(String, String)],
    /// Image name → GLSL format qualifier.
    pub image_formats: &'a HashMap<String, String>,
    /// Sampler classes overriding [`crate::SamplerClass::for_name`] for this program.
    pub sampler_classes: &'a crate::transform::SamplerClasses,
}

/// Most rule identifiers force mode considers; the search tries every subset of them.
const MAX_FORCED_NAMES: usize = 12;

/// The fewest option flips and macro definitions that make `rule` hold, as (name, value)
/// where boolean options get "true"/"false" and undefined macros get "".
pub(crate) fn forcing(
    rule: &ProgramRule,
    options: &[PackOption],
    defines: &BTreeMap<String, String>,
) -> Option<Vec<(String, String)>> {
    let flips: Vec<(String, String)> = rule
        .expr
        .identifiers()
        .into_iter()
        .filter_map(|name| {
            let option = options.iter().find(|o| o.name == name);
            match option.map(|o| &o.kind) {
                Some(OptionKind::Bool { .. }) => {
                    let on = defines.contains_key(name);
                    Some((
                        name.to_string(),
                        if on { "false" } else { "true" }.to_string(),
                    ))
                }
                Some(_) => None,
                None if !defines.contains_key(name) && name != "true" && name != "false" => {
                    Some((name.to_string(), String::new()))
                }
                None => None,
            }
        })
        .take(MAX_FORCED_NAMES)
        .collect();
    let mut subsets: Vec<u32> = (1..(1u32 << flips.len())).collect();
    subsets.sort_by_key(|m| m.count_ones());
    subsets.into_iter().find_map(|m| {
        let chosen: Vec<(String, String)> = flips
            .iter()
            .enumerate()
            .filter(|(i, _)| m & (1 << i) != 0)
            .map(|(_, f)| f.clone())
            .collect();
        let mut d = defines.clone();
        for (name, value) in &chosen {
            match value.as_str() {
                "false" => {
                    d.remove(name);
                }
                "true" => {
                    d.insert(name.clone(), String::new());
                }
                v => {
                    d.insert(name.clone(), v.to_string());
                }
            }
        }
        rule.is_enabled(&d).then_some(chosen)
    })
}

/// Translates `program`'s stage files. `enabled`/`forced` are recorded as given.
pub(crate) fn translate_program(
    cx: &Context<'_>,
    program: &ProgramSource,
    enabled: bool,
    forced: Vec<(String, String)>,
) -> ProgramTranslation {
    let mut out = ProgramTranslation {
        program: program.clone(),
        enabled,
        forced,
        stages: vec![],
        diagnostics: vec![],
        reflection: None,
        draw_buffers: vec![0],
        consts: vec![],
    };
    let has = |s: StageKind| program.stages.iter().any(|(k, _)| *k == s);
    if program.kind == ProgramKind::Render {
        for (needed, what) in [
            (StageKind::Vertex, "vertex"),
            (StageKind::Fragment, "fragment"),
        ] {
            if !has(needed) {
                out.diagnostics.push(Diagnostic::new(
                    Phase::Link,
                    format!("program has no {what} stage"),
                ));
            }
        }
    }
    let mut iface = ProgramInterface::default();
    // Stage index → draft and line map, for stages that got through the transform.
    let mut drafts: Vec<(usize, StageDraft, crate::preprocess::LineMap)> = vec![];
    let mut fragment_targets = None;
    let mut vertex_targets = None;
    for (i, (stage, path)) in program.stages.iter().enumerate() {
        out.stages.push(StageTranslation {
            stage: *stage,
            path: path.clone(),
            result: Err(vec![]),
        });
        if crate::compile::naga_stage(*stage).is_none() {
            out.stages[i].result = Err(vec![Diagnostic::new(
                Phase::Transform,
                format!("{} shaders are not supported", stage.extension()),
            )]);
            continue;
        }
        let pp =
            expand(cx.source, path, cx.options, cx.values).and_then(|e| preprocess(e, cx.defines));
        let pp = match pp {
            Ok(pp) => pp,
            Err(d) => {
                out.stages[i].result = Err(vec![d]);
                continue;
            }
        };
        match stage {
            StageKind::Fragment => fragment_targets = pp.render_targets.clone(),
            StageKind::Vertex => vertex_targets = pp.render_targets.clone(),
            _ => {}
        }
        out.consts.extend(pp.consts.iter().cloned());
        let names = ResourceNames {
            image_formats: cx.image_formats,
            sampler_classes: cx.sampler_classes,
        };
        match transform(&pp, *stage, &mut iface, &names) {
            Ok(draft) => drafts.push((i, draft, pp.map)),
            Err(e) => {
                let d = Diagnostic::new(Phase::Transform, e.message).at(pp.map.locate(e.line));
                out.stages[i].result = Err(vec![d]);
            }
        }
    }
    if let Some(targets) = fragment_targets.or(vertex_targets) {
        out.draw_buffers = targets;
    }
    let ubo = match merge_uniforms(drafts.iter().map(|(_, d, _)| d)) {
        Ok(u) => u,
        Err(message) => {
            out.diagnostics.push(Diagnostic::new(Phase::Link, message));
            return out;
        }
    };
    let locations = match iface.location_defines() {
        Ok(locations) => locations,
        Err(message) => {
            out.diagnostics.push(Diagnostic::new(Phase::Link, message));
            return out;
        }
    };
    for (i, draft, map) in &drafts {
        let (glsl, origins) = assemble(draft, &ubo, &locations);
        let output = OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: map,
        };
        out.stages[*i].result = compile(out.stages[*i].stage, &output);
    }
    if !out.diagnostics.is_empty() || out.stages.iter().any(|s| s.result.is_err()) {
        return out;
    }
    let inputs: Vec<(&CompiledStage, StageNames<'_>)> = drafts
        .iter()
        .filter_map(|(i, draft, _)| {
            let compiled = out.stages[*i].result.as_ref().ok()?;
            Some((
                compiled,
                StageNames {
                    vertex_inputs: &draft.vertex_inputs,
                    frag_outputs: &draft.frag_outputs,
                    resources: &draft.resource_names,
                },
            ))
        })
        .collect();
    match reflect(program.kind, &inputs, &out.draw_buffers) {
        Ok(r) => out.reflection = Some(r),
        Err(message) => out.diagnostics.push(Diagnostic::new(Phase::Link, message)),
    }
    out
}
