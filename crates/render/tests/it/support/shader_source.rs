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
    let biome = meshing::biome_lattice::shader_source(include_str!("../../../src/biome_tint.wgsl"));
    let material = material_shader::source(include_str!("../../../src/material.wgsl"));
    let lighting = material_shader::source(include_str!("../../../src/lighting.wgsl"));
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
            } else if module.starts_with("cinnabar::enhanced_common") {
                ("common", include_str!("../../../src/enhanced/common.wgsl"))
            } else if module.starts_with("cinnabar::enhanced_environment") {
                (
                    "environment",
                    include_str!("../../../src/enhanced/environment.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_temporal") {
                (
                    "temporal",
                    include_str!("../../../src/enhanced/temporal.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_local_lights") {
                (
                    "local_lights",
                    include_str!("../../../src/enhanced/local_lights.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_actor_motion") {
                (
                    "actor_motion",
                    include_str!("../../../src/enhanced/actor_motion.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_shadow") {
                ("shadows", include_str!("../../../src/enhanced/shadow.wgsl"))
            } else if module.starts_with("cinnabar::enhanced_sun_shadow_temporal") {
                (
                    "sun_shadow_temporal",
                    include_str!("../../../src/enhanced/sun_shadow_temporal.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_radiance") {
                (
                    "radiance",
                    include_str!("../../../src/enhanced/radiance.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_water") {
                ("water", include_str!("../../../src/enhanced/water.wgsl"))
            } else if module.starts_with("cinnabar::enhanced_atmosphere") {
                (
                    "physical_atmosphere",
                    include_str!("../../../src/enhanced/atmosphere.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_clouds") {
                (
                    "volume_clouds",
                    include_str!("../../../src/enhanced/clouds.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_ao") {
                ("horizon_ao", include_str!("../../../src/enhanced/ao.wgsl"))
            } else if module.starts_with("cinnabar::enhanced_indirect_trace") {
                (
                    "indirect_trace",
                    include_str!("../../../src/enhanced/indirect_trace.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_indirect") {
                (
                    "indirect",
                    include_str!("../../../src/enhanced/indirect.wgsl"),
                )
            } else if module.starts_with("cinnabar::enhanced_pbr") {
                ("pbr", include_str!("../../../src/enhanced/pbr.wgsl"))
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
    let module = composed_module(source, definitions);
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("composed module validates");
    naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
        .expect("write composed WGSL")
}

/// Retains the composed IR used by Bevy's native shader compilation path.
pub fn composed_module(source: &str, definitions: &[&str]) -> naga::Module {
    use naga_oil::compose::{
        ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
    };
    let resolved = material_shader::source(source);
    let source = resolved.as_str();
    let mut composer = Composer::default();
    for (name, body) in composable_sources() {
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
    composer
        .make_naga_module(NagaModuleDescriptor {
            source,
            file_path: "enhanced_validation.wgsl",
            shader_defs: definitions
                .iter()
                .map(|name| ((*name).to_owned(), ShaderDefValue::Bool(true)))
                .collect(),
            ..Default::default()
        })
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&composer)))
}

pub fn fullscreen_vertex_source() -> String {
    format!(
        "#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput\n{FULLSCREEN_VERTEX}"
    )
}

pub fn composable_sources() -> Vec<(&'static str, String)> {
    vec![
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
            meshing::biome_lattice::shader_source(include_str!("../../../src/biome_tint.wgsl")),
        ),
        (
            "cinnabar::enhanced_common",
            include_str!("../../../src/enhanced/common.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_environment",
            include_str!("../../../src/enhanced/environment.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_temporal",
            include_str!("../../../src/enhanced/temporal.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_local_lights",
            include_str!("../../../src/enhanced/local_lights.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_actor_motion",
            include_str!("../../../src/enhanced/actor_motion.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_atmosphere",
            include_str!("../../../src/enhanced/atmosphere.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_clouds",
            include_str!("../../../src/enhanced/clouds.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_ao",
            include_str!("../../../src/enhanced/ao.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_shadow",
            include_str!("../../../src/enhanced/shadow.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_sun_shadow_temporal",
            include_str!("../../../src/enhanced/sun_shadow_temporal.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_radiance",
            include_str!("../../../src/enhanced/radiance.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_water",
            include_str!("../../../src/enhanced/water.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_indirect_trace",
            include_str!("../../../src/enhanced/indirect_trace.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_indirect",
            include_str!("../../../src/enhanced/indirect.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_pbr",
            material_shader::source(include_str!("../../../src/enhanced/pbr.wgsl")),
        ),
        (
            "cinnabar::enhanced_view",
            include_str!("../../../src/enhanced/view.wgsl").to_owned(),
        ),
        (
            "cinnabar::enhanced_caster",
            include_str!("../../../src/enhanced/caster.wgsl").to_owned(),
        ),
    ]
}
