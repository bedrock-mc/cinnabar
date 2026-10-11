//! Resolve the project's small WGSL import graph for standalone validation.
#![allow(
    dead_code,
    reason = "shared helpers serve different shader test targets"
)]
use crate::material_shader;
use std::collections::BTreeSet;

const VIEW: &str = "struct View { clip_from_world: mat4x4<f32>, unjittered_clip_from_world: mat4x4<f32>, view_from_world: mat4x4<f32>, world_from_view: mat4x4<f32>, clip_from_view: mat4x4<f32>, view_from_clip: mat4x4<f32>, world_position: vec3<f32>, exposure: f32, viewport: vec4<f32>, }";
const FULLSCREEN: &str = "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, }";
const FULLSCREEN_VERTEX: &str = "@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> FullscreenVertexOutput { var out: FullscreenVertexOutput; out.uv = vec2(f32((index << 1u) & 2u), f32(index & 2u)); out.position = vec4(out.uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0); return out; }";

/// Reads the alpha cutoff guarding a fragment discard, including named WGSL constants.
pub fn alpha_discard_threshold(source: &str, function: &str) -> Option<f32> {
    let module = naga::front::wgsl::parse_str(&standalone(source, &[])).expect("shader parses");
    // An entry point, or the shading function an entry point delegates to.
    let function = module
        .entry_points
        .iter()
        .find(|point| point.name == function)
        .map(|point| &point.function)
        .or_else(|| {
            module
                .functions
                .iter()
                .map(|(_, candidate)| candidate)
                .find(|candidate| candidate.name.as_deref() == Some(function))
        })
        .expect("fragment function exists");
    discard_threshold(&module, function, &function.body)
}

/// Follows nested fragment guards to the comparison that actually discards its alpha.
fn discard_threshold(
    module: &naga::Module,
    function: &naga::Function,
    block: &naga::Block,
) -> Option<f32> {
    block.iter().find_map(|statement| match statement {
        naga::Statement::If {
            condition,
            accept,
            reject,
        } => {
            let threshold = accept
                .iter()
                .any(|statement| matches!(statement, naga::Statement::Kill))
                .then(|| alpha_comparison(module, function, *condition))
                .flatten();
            threshold
                .or_else(|| discard_threshold(module, function, accept))
                .or_else(|| discard_threshold(module, function, reject))
        }
        naga::Statement::Block(inner) => discard_threshold(module, function, inner),
        _ => None,
    })
}

/// Resolves the cutoff of an alpha-channel comparison inside a combined material guard.
fn alpha_comparison(
    module: &naga::Module,
    function: &naga::Function,
    condition: naga::Handle<naga::Expression>,
) -> Option<f32> {
    let naga::Expression::Binary { op, left, right } = function.expressions[condition] else {
        return None;
    };
    let alpha = match function.expressions[left] {
        naga::Expression::Load { pointer } => &function.expressions[pointer],
        _ => &function.expressions[left],
    };
    if op == naga::BinaryOperator::Less
        && matches!(alpha, naga::Expression::AccessIndex { index: 3, .. })
    {
        let expression = match function.expressions[right] {
            naga::Expression::Constant(constant) => {
                &module.global_expressions[module.constants[constant].init]
            }
            _ => &function.expressions[right],
        };
        return match expression {
            naga::Expression::Literal(naga::Literal::F32(value)) => Some(*value),
            _ => None,
        };
    }
    alpha_comparison(module, function, left).or_else(|| alpha_comparison(module, function, right))
}

/// Keep precisely the active Enhanced branches, including the depth caster variant.
pub fn preprocess(source: &str, definitions: &[&str]) -> String {
    let source = material_shader::source(source);
    let mut active = vec![true];
    let mut output = String::new();
    for line in source.split_inclusive('\n') {
        let directive = line.trim();
        if let Some(name) = directive.strip_prefix("#ifdef ") {
            active.push(definitions.contains(&name));
        } else if let Some(name) = directive.strip_prefix("#ifndef ") {
            active.push(!definitions.contains(&name));
        } else if directive == "#else" {
            let enabled = active.last_mut().expect("matching conditional");
            *enabled = !*enabled;
        } else if directive == "#endif" {
            assert!(active.len() > 1, "unmatched endif");
            active.pop();
        } else if active.iter().all(|value| *value) {
            output.push_str(line);
        }
    }
    assert_eq!(active.len(), 1, "unterminated conditional");
    output
}

/// Inline imported modules once, matching Bevy's shared WGSL definitions.
pub fn standalone(source: &str, definitions: &[&str]) -> String {
    imports(
        &preprocess(&meshing::cloud_viewport::shader_source(source), definitions),
        &mut BTreeSet::new(),
    )
}

/// Expand multiline imports recursively while retaining all selected module symbols.
fn imports(source: &str, seen: &mut BTreeSet<String>) -> String {
    let mut output = String::new();
    let mut lines = source.lines();
    let biome = material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
        include_str!("../../../src/biome_tint.wgsl"),
    ));
    let material = material_shader::source(include_str!("../../../src/material.wgsl"));
    let lighting = material_shader::source(include_str!("../../../src/lighting.wgsl"));
    let bindings = material_shader::source(include_str!("../../../src/chunk_bindings.wgsl"));
    while let Some(line) = lines.next() {
        let directive = line.trim();
        if directive.starts_with("#define_import_path") {
            continue;
        }
        if let Some(import) = directive.strip_prefix("#import ") {
            let module = import.split("::{").next().unwrap();
            if import.contains('{') && !import.contains('}') {
                for continuation in lines.by_ref() {
                    if continuation.contains('}') {
                        break;
                    }
                }
            }
            let (key, body) = if module.starts_with("bevy_render::view::") {
                ("view", VIEW)
            } else if module.starts_with("bevy_core_pipeline::fullscreen_vertex_shader::") {
                ("fullscreen", FULLSCREEN)
            } else if module.starts_with("cinnabar::material") {
                ("material", material.as_str())
            } else if module.starts_with("cinnabar::lighting") {
                ("lighting", lighting.as_str())
            } else if module.starts_with("cinnabar::biome_tint") {
                ("biome", biome.as_str())
            } else if module.starts_with("cinnabar::world_projection") {
                (
                    "world_projection",
                    include_str!("../../../src/world_projection.wgsl"),
                )
            } else if module.starts_with("cinnabar::chunk_bindings") {
                ("chunk_bindings", bindings.as_str())
            } else if module.starts_with("cinnabar::enhanced_common") {
                ("common", include_str!("../../../src/enhanced/common.wgsl"))
            } else if module.starts_with("cinnabar::enhanced_view") {
                (
                    "enhanced_view",
                    include_str!("../../../src/enhanced/view.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_caster") {
                ("caster", include_str!("../../../src/enhanced/caster.wgsl"))
            } else {
                panic!("unhandled shader import: {import}");
            };
            if seen.insert(key.to_owned()) {
                output.push_str(&imports(body, seen));
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

/// Compose with the same imported-symbol pruning and preprocessor Bevy uses.
pub fn composed(source: &str, definitions: &[&str]) -> String {
    use naga_oil::compose::{
        ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
    };
    let resolved = material_shader::source(source);
    let source = resolved.as_str();
    let mut composer = Composer::default();
    for (name, body) in [
        ("bevy_render::view", VIEW.to_owned()),
        (
            "bevy_core_pipeline::fullscreen_vertex_shader",
            FULLSCREEN.to_owned(),
        ),
        (
            "cinnabar::material",
            material_shader::source(include_str!("../../../src/material.wgsl")),
        ),
        (
            "cinnabar::lighting",
            material_shader::source(include_str!("../../../src/lighting.wgsl")),
        ),
        (
            "cinnabar::biome_tint",
            material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
                include_str!("../../../src/biome_tint.wgsl"),
            )),
        ),
        (
            "cinnabar::world_projection",
            include_str!("../../../src/world_projection.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_common",
            include_str!("../../../src/enhanced/common.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_view",
            include_str!("../../../src/enhanced/view.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_caster",
            include_str!("../../../src/enhanced/caster.wgsl").to_owned(),
        ),
        (
            "cinnabar::chunk_bindings",
            material_shader::source(include_str!("../../../src/chunk_bindings.wgsl")),
        ),
        (
            "cinnabar::liquid",
            material_shader::source(include_str!("../../../src/liquid.wgsl")),
        ),
        (
            "cinnabar::model",
            material_shader::source(include_str!("../../../src/model.wgsl")),
        ),
    ] {
        composer
            .add_composable_module(ComposableModuleDescriptor {
                source: &body,
                file_path: name,
                as_name: Some(name.to_owned()),
                ..Default::default()
            })
            .map(|_| ())
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&composer)));
    }
    let fullscreen_source;
    let source = if source.contains("#import bevy_core_pipeline::fullscreen_vertex_shader") {
        fullscreen_source = format!("{source}\n{FULLSCREEN_VERTEX}");
        fullscreen_source.as_str()
    } else {
        source
    };
    let module = composer
        .make_naga_module(NagaModuleDescriptor {
            source,
            file_path: "enhanced_validation.wgsl",
            shader_defs: definitions
                .iter()
                .map(|name| ((*name).to_owned(), ShaderDefValue::Bool(true)))
                .collect(),
            ..Default::default()
        })
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&composer)));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("composed module validates");
    naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
        .expect("write composed WGSL")
}
