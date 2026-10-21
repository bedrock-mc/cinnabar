//! Device-independent buffer usage and storage sizing for translated pack programs.

use super::super::plan::{MAX_COLOR_BUFFERS, PlannedProgram, color_buffer_of};
use super::super::uniforms::PackSettings;
use shaderpack::{
    BlendMode, BufferId, ProgramKind, ProgramReflection, ProgramTranslation, ShaderProperties,
    StageKind,
};

/// Destination buffer per fragment output location.
fn outputs_by_location(reflection: &ProgramReflection) -> Vec<Option<u32>> {
    let len = reflection
        .color_outputs
        .iter()
        .map(|o| o.location as usize + 1)
        .max()
        .unwrap_or(0);
    let mut outputs = vec![None; len];
    for output in &reflection.color_outputs {
        outputs[output.location as usize] = output.buffer;
    }
    outputs
}

/// Whether some function of `module` can discard a fragment.
fn discards(module: &naga::Module) -> bool {
    /// Finds a discard in this block or any nested control flow.
    fn kills(block: &naga::Block) -> bool {
        block.iter().any(|statement| match statement {
            naga::Statement::Kill => true,
            naga::Statement::Block(inner) => kills(inner),
            naga::Statement::If { accept, reject, .. } => kills(accept) || kills(reject),
            naga::Statement::Switch { cases, .. } => cases.iter().any(|c| kills(&c.body)),
            naga::Statement::Loop {
                body, continuing, ..
            } => kills(body) || kills(continuing),
            _ => false,
        })
    }
    module.functions.iter().any(|(_, f)| kills(&f.body))
        || module.entry_points.iter().any(|e| kills(&e.function.body))
}

/// Plans one translated program, rejecting unsupported destinations before building masks.
pub(super) fn planned(
    program: &ProgramTranslation,
    reflection: &ProgramReflection,
    properties: &ShaderProperties,
) -> Result<PlannedProgram, String> {
    let name = program.program.name.as_str();
    let mipmaps = program
        .consts
        .iter()
        .filter(|c| c.value.trim() == "true")
        .filter_map(|c| c.name.strip_suffix("MipmapEnabled"))
        .filter_map(|b| match BufferId::parse(b) {
            Some(BufferId::Color(i)) if i < MAX_COLOR_BUFFERS => Some(i),
            _ => None,
        })
        .collect();
    let outputs = if reflection.kind == ProgramKind::Render {
        outputs_by_location(reflection)
    } else {
        vec![]
    };
    if let Some(index) = outputs.iter().flatten().find(|&&b| b >= MAX_COLOR_BUFFERS) {
        return Err(format!(
            "{name} writes unsupported color buffer {index}; expected an index below {MAX_COLOR_BUFFERS}"
        ));
    }
    let whole = properties.scales.iter().all(|s| s.program != name)
        && !program
            .stages
            .iter()
            .filter_map(|s| s.result.as_ref().ok())
            .any(|c| c.stage == StageKind::Fragment && discards(&c.module));
    let blends = |buffer: u32| {
        let rule = |per_buffer: bool| {
            properties.blends.iter().rev().find(|r| {
                r.program == name
                    && match r.buffer.as_deref() {
                        Some(b) => per_buffer && color_buffer_of(b) == Some(buffer),
                        None => !per_buffer,
                    }
            })
        };
        rule(true)
            .or_else(|| rule(false))
            .is_some_and(|r| r.mode != BlendMode::Off)
    };
    let partial = outputs
        .iter()
        .flatten()
        .filter(|&&b| !whole || blends(b))
        .fold(0u32, |mask, b| mask | (1 << b));
    Ok(PlannedProgram {
        name: name.to_owned(),
        group: program.program.group,
        kind: program.program.kind,
        outputs,
        reads: reflection
            .textures
            .iter()
            .map(|t| t.name.as_str())
            .chain(reflection.storage_images.iter().map(|i| i.name.as_str()))
            .filter_map(color_buffer_of)
            .collect(),
        mipmaps,
        partial,
    })
}

/// Storage bindings: the reflected minimum, raised to the pack's complete requested words.
/// Resolves whole-buffer binding sizes, rejecting requests whose alignment would overflow.
pub(super) fn storage_sizes(
    reflections: &[&ProgramReflection],
    properties: &shaderpack::ShaderProperties,
) -> Result<Vec<(u32, u64)>, String> {
    let mut sizes: Vec<(u32, u64)> = vec![];
    for buffer in reflections.iter().flat_map(|r| &r.storage_buffers) {
        let declared = properties
            .get(&format!("bufferObject.{}", buffer.binding))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0);
        let size = storage_binding_size(buffer.min_size, declared)
            .map_err(|error| format!("storage binding {}: {error}", buffer.binding))?;
        match sizes.iter_mut().find(|(b, _)| *b == buffer.binding) {
            Some((_, s)) => *s = (*s).max(size),
            None => sizes.push((buffer.binding, size)),
        }
    }
    Ok(sizes)
}

/// Settings declared by the enabled programs in the pack.
pub(super) fn pack_settings(programs: &[ProgramTranslation]) -> PackSettings {
    PackSettings::read(
        programs
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.consts),
    )
}

/// Binding size in complete storage words, at least the reflected prefix.
fn storage_binding_size(minimum: u32, declared: u64) -> Result<u64, String> {
    let minimum = u64::from(minimum).max(4).next_multiple_of(4);
    let size = minimum.max(declared);
    let aligned = size.checked_next_multiple_of(4).ok_or("size overflows")?;
    Ok(aligned - if size.is_multiple_of(4) { 0 } else { 4 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shaderpack::ProgramGroup;

    #[test]
    fn storage_sizes_reject_alignment_overflow() {
        let properties = format!("bufferObject.0={}\n", u64::MAX);
        let pack = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
            ("shaders.properties", properties.as_str()),
            (
                "composite.csh",
                "#version 430\nlayout(local_size_x=1) in;\nlayout(std430, binding=0) buffer Data { uint value; } data;\nvoid main() { data.value = 1u; }\n",
            ),
        ]));
        let translation = pack.translate(&shaderpack::TranslateConfig::default());
        let reflections: Vec<_> = translation
            .programs
            .iter()
            .map(|program| {
                program
                    .reflection
                    .as_ref()
                    .expect("the storage fixture translates")
            })
            .collect();
        assert_eq!(reflections.len(), 1);
        assert!(storage_sizes(&reflections, &translation.properties).is_err());
    }
    /// A program with one pack-wide setting, independently of host execution support.
    fn setting_program(group: ProgramGroup, enabled: bool, value: &str) -> ProgramTranslation {
        ProgramTranslation {
            program: shaderpack::ProgramSource {
                name: String::new(),
                group,
                kind: ProgramKind::Render,
                folder: String::new(),
                stages: vec![],
            },
            enabled,
            forced: vec![],
            stages: vec![],
            diagnostics: vec![],
            reflection: None,
            draw_buffers: vec![],
            consts: vec![shaderpack::ConstDirective {
                ty: "float".into(),
                name: "sunPathRotation".into(),
                value: value.into(),
            }],
        }
    }

    #[test]
    fn pack_settings_include_enabled_geometry_and_shadow_programs() {
        let programs = [
            setting_program(ProgramGroup::Gbuffers, true, "30.0"),
            setting_program(ProgramGroup::Shadow, true, "45.0"),
            setting_program(ProgramGroup::Composite, false, "90.0"),
        ];
        assert_eq!(pack_settings(&programs).sun_path_rotation, 45.0);
    }

    #[test]
    fn storage_binding_lengths_keep_complete_declared_words() {
        for declared in [20, 21, 23] {
            assert_eq!(storage_binding_size(4, declared).unwrap(), 20);
        }
        assert_eq!(storage_binding_size(16, 4).unwrap(), 16);
        assert!(storage_binding_size(4, u64::MAX).is_err());
    }
}
