//! naga GLSL parsing and validation, with errors mapped back to pack files.

use crate::diagnostics::{Diagnostic, Phase};
use crate::preprocess::LineMap;
use crate::programs::StageKind;
use naga::valid::ModuleInfo;

mod inputs;
mod order;

#[cfg(test)]
mod tests;

/// A validated stage module.
#[derive(Clone, Debug)]
pub struct CompiledStage {
    pub stage: StageKind,
    /// The translated GLSL 4.50 that naga parsed.
    pub glsl: String,
    pub module: naga::Module,
    /// Validation info for `module`, validated with every capability; the host checks the
    /// capabilities and limits its device actually has.
    pub info: ModuleInfo,
}

/// naga's stage for a translatable stage kind.
pub(crate) fn naga_stage(stage: StageKind) -> Option<naga::ShaderStage> {
    match stage {
        StageKind::Vertex => Some(naga::ShaderStage::Vertex),
        StageKind::Fragment => Some(naga::ShaderStage::Fragment),
        StageKind::Compute => Some(naga::ShaderStage::Compute),
        _ => None,
    }
}

/// Maps lines of translated GLSL to pack locations.
pub(crate) struct OutputMap<'a> {
    pub glsl: &'a str,
    /// Include-expanded source line of every GLSL line (0 = generated).
    pub origins: &'a [u32],
    pub lines: &'a LineMap,
}

impl OutputMap<'_> {
    /// A diagnostic at 1-based GLSL line `line`: located in the pack when the line came
    /// from it, otherwise carrying the generated line as its snippet.
    pub fn diagnostic(&self, phase: Phase, message: String, line: Option<u32>) -> Diagnostic {
        let mut d = Diagnostic::new(phase, message);
        let Some(line) = line else {
            return d;
        };
        let origin = self.origins.get(line as usize - 1).copied().unwrap_or(0);
        d.location = self.lines.locate(origin);
        if d.location.is_none() {
            d.snippet = self.glsl.lines().nth(line as usize - 1).map(str::to_string);
        }
        d
    }
}

/// Parses translated GLSL with naga's GLSL frontend and validates the module.
pub(crate) fn compile(
    stage: StageKind,
    map: &OutputMap<'_>,
) -> Result<CompiledStage, Vec<Diagnostic>> {
    let Some(naga_stage) = naga_stage(stage) else {
        return Err(vec![Diagnostic::new(
            Phase::Parse,
            format!("{} shaders are not supported", stage.extension()),
        )]);
    };
    let mut frontend = naga::front::glsl::Frontend::default();
    let mut module = frontend
        .parse(&naga::front::glsl::Options::from(naga_stage), map.glsl)
        .map_err(|e| {
            e.errors
                .iter()
                .map(|err| {
                    let line = err
                        .meta
                        .is_defined()
                        .then(|| err.meta.location(map.glsl).line_number);
                    map.diagnostic(Phase::Parse, err.kind.to_string(), line)
                })
                .collect::<Vec<_>>()
        })?;
    order::functions(&mut module).map_err(|span| {
        let line = span
            .is_defined()
            .then(|| span.location(map.glsl).line_number);
        vec![map.diagnostic(
            Phase::Validate,
            "recursive function calls are not supported".to_string(),
            line,
        )]
    })?;
    let info = inputs::validate(&mut module).map_err(|e| {
        let line = e
            .spans()
            .find(|(span, _)| span.is_defined())
            .map(|(span, _)| span.location(map.glsl).line_number);
        vec![map.diagnostic(Phase::Validate, error_chain(&e), line)]
    })?;
    Ok(CompiledStage {
        stage,
        glsl: map.glsl.to_string(),
        module,
        info,
    })
}

/// `error: cause: cause …`.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    s
}
