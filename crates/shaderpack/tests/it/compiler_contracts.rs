//! Translation preserves source bindings and rejects incomplete program contracts.

use crate::{program, reflection};
use shaderpack::{PackSource, PackTranslation, Phase, ShaderPack, StageMask, TranslateConfig};

const VERTEX: &str = "#version 330\nvoid main() { gl_Position = vec4(0.0); }";
const FRAGMENT: &str = "#version 330\nout vec4 color; void main() { color = vec4(1.0); }";

/// Translates an original in-memory render program without external fixtures.
fn translate(vertex: &str, fragment: &str) -> PackTranslation {
    ShaderPack::new(PackSource::from_files([
        ("composite.vsh", vertex),
        ("composite.fsh", fragment),
    ]))
    .translate(&TranslateConfig::default())
}

/// Requires a usable program and returns its complete translation for module inspection.
fn success(vertex: &str, fragment: &str) -> PackTranslation {
    let translation = translate(vertex, fragment);
    reflection(program(&translation, "composite"));
    translation
}

/// Collects named interface locations from direct values and result structures.
fn interface_locations(module: &naga::Module, output: bool) -> Vec<(&str, u32)> {
    let function = &module.entry_points[0].function;
    let values: Vec<_> = if output {
        function
            .result
            .iter()
            .map(|value| (None, value.ty, value.binding.as_ref()))
            .collect()
    } else {
        function
            .arguments
            .iter()
            .map(|value| (value.name.as_deref(), value.ty, value.binding.as_ref()))
            .collect()
    };
    let mut locations = Vec::new();
    for (name, ty, binding) in values {
        if let Some(naga::Binding::Location { location, .. }) = binding {
            locations.push((name.unwrap_or_default(), *location));
        } else if let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner {
            locations.extend(members.iter().filter_map(|member| match member.binding {
                Some(naga::Binding::Location { location, .. }) => {
                    Some((member.name.as_deref().unwrap_or_default(), location))
                }
                _ => None,
            }));
        }
    }
    locations.sort_unstable();
    locations
}

#[test]
fn resource_overloads_keep_opaque_types_and_nonopaque_signatures() {
    let translation = success(
        VERTEX,
        "#version 330
        uniform sampler2D plane;
        uniform sampler3D volume;
        vec4 read(sampler2D source, vec2 uv) { return texture(source, uv); }
        vec4 read(sampler3D source, vec2 uv) { return texture(source, vec3(uv, 0.5)); }
        vec4 read(sampler3D source, float u) { return texture(source, vec3(u)); }
        out vec4 color;
        void main() { color = read(plane, vec2(0.5)) + read(volume, vec2(0.5)) + read(volume, 0.5); }",
    );
    let reflected = reflection(program(&translation, "composite"));
    assert_eq!(reflected.textures.len(), 2);
    assert!(
        reflected
            .textures
            .iter()
            .any(|t| t.name == "volume" && t.dimension == shaderpack::TextureDimension::D3)
    );
}

#[test]
fn stage_interface_requires_compatible_vertex_outputs() {
    for (vertex, fragment) in [
        (
            "#version 330\nout vec2 uv; void main() { uv = vec2(0.0); gl_Position = vec4(0.0); }",
            "#version 330\nin vec3 uv; out vec4 color; void main() { color = vec4(uv, 1.0); }",
        ),
        (
            VERTEX,
            "#version 330\nin vec2 missing; out vec4 color; void main() { color = vec4(missing, 0.0, 1.0); }",
        ),
    ] {
        let translation = translate(vertex, fragment);
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok());
        assert!(failed.diagnostics.iter().any(|d| d.phase == Phase::Link));
    }
}

#[test]
fn explicit_varying_locations_link_different_names_and_skip_implicit_slots() {
    let translation = success(
        "#version 450
        out vec2 automaticUv;
        layout(location=0) out vec2 sourceUv;
        void main() { automaticUv = vec2(0.5); sourceUv = vec2(1.0); gl_Position = vec4(0.0); }",
        "#version 450
        layout(location=0) in vec2 uv;
        in vec2 automaticUv;
        out vec4 color;
        void main() { color = vec4(uv + automaticUv, 0.0, 1.0); }",
    );
    let translated = program(&translation, "composite");
    let vertex = &translated.stages[0].result.as_ref().unwrap().module;
    let fragment = &translated.stages[1].result.as_ref().unwrap().module;
    let locations = |module: &naga::Module, output: bool| {
        let mut locations: Vec<_> = interface_locations(module, output)
            .into_iter()
            .map(|(_, location)| location)
            .collect();
        locations.sort_unstable();
        locations
    };
    assert_eq!(locations(vertex, true), locations(fragment, false));
    assert!(locations(vertex, true).contains(&0));
}

#[test]
fn explicit_locations_override_crossed_names() {
    let translation = success(
        "#version 450
        layout(location=0) out vec2 a; layout(location=1) out vec2 b;
        void main() { a = vec2(0.25); b = vec2(0.75); gl_Position = vec4(0.0); }",
        "#version 450
        layout(location=1) in vec2 a; layout(location=0) in vec2 b;
        out vec4 color; void main() { color = vec4(a, b); }",
    );
    let translated = program(&translation, "composite");
    let vertex = &translated.stages[0].result.as_ref().unwrap().module;
    let fragment = &translated.stages[1].result.as_ref().unwrap().module;
    assert_eq!(interface_locations(vertex, true), [("a", 0), ("b", 1)]);
    assert_eq!(interface_locations(fragment, false), [("a", 1), ("b", 0)]);
}

#[test]
fn mixed_locations_link_by_name_and_reserve_explicit_slots() {
    for (vertex_location, fragment_location) in
        [("layout(location=2)", ""), ("", "layout(location=2)")]
    {
        success(
            &format!("#version 450\n{vertex_location} out vec3 sharedUv; out vec2 automaticUv;
            void main() {{ sharedUv = vec3(0.5); automaticUv = vec2(0.25); gl_Position = vec4(0.0); }}"),
            &format!("#version 450\n{fragment_location} in vec3 sharedUv; in vec2 automaticUv;
            out vec4 color; void main() {{ color = vec4(sharedUv + automaticUv.x, 1.0); }}"),
        );
    }
}

/// Translates a storage program alongside an independent, healthy render program.
fn storage_translate(vertex: &str, fragment: &str) -> PackTranslation {
    let translated = ShaderPack::new(PackSource::from_files([
        ("composite.vsh", vertex),
        ("composite.fsh", fragment),
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
    ]))
    .translate(&TranslateConfig::default());
    reflection(program(&translated, "final"));
    translated
}

/// Requires each named storage resource to retain its binding without aliases.
fn bindings(translated: &PackTranslation, expected: &[(&str, u32)]) {
    let reflected = reflection(program(translated, "composite"));
    let actual: Vec<_> = reflected
        .storage_buffers
        .iter()
        .map(|buffer| (buffer.name.as_str(), buffer.binding))
        .collect();
    for binding in expected {
        assert!(
            actual.contains(binding),
            "missing {binding:?} in {actual:?}"
        );
    }
    assert_eq!(actual.len(), expected.len());
}

#[test]
fn storage_bindings_allow_an_explicit_block_after_the_upper_boundary() {
    let translated = storage_translate(
        VERTEX,
        "#version 450
        layout(binding=4294967295u) readonly buffer High { vec4 value; } high;
        layout(binding=7) readonly buffer Next { vec4 value; } next;
        out vec4 color; void main() { color = high.value + next.value; }",
    );
    bindings(&translated, &[("high", u32::MAX), ("next", 7)]);
}

#[test]
fn storage_bindings_find_a_gap_after_the_upper_boundary() {
    let translated = storage_translate(
        VERTEX,
        "#version 450
        layout(binding=0) readonly buffer Low { vec4 value; } low;
        layout(binding=4294967295u) readonly buffer High { vec4 value; } high;
        readonly buffer Next { vec4 value; } next;
        out vec4 color; void main() { color = high.value + low.value + next.value; }",
    );
    bindings(&translated, &[("high", u32::MAX), ("low", 0), ("next", 1)]);
}

#[test]
fn storage_bindings_reuse_the_upper_boundary_across_stages() {
    for qualifier in ["layout(binding=4294967295u)", ""] {
        let fragment = format!(
            "#version 450
            {qualifier} readonly buffer High {{ vec4 value; }} high;
            out vec4 color; void main() {{ color = high.value; }}"
        );
        let translated = storage_translate(
            "#version 450
            layout(binding=4294967295u) readonly buffer High { vec4 value; } high;
            void main() { gl_Position = high.value; }",
            &fragment,
        );
        bindings(&translated, &[("high", u32::MAX)]);
        assert_eq!(
            reflection(program(&translated, "composite")).storage_buffers[0].stages,
            StageMask {
                vertex: true,
                fragment: true,
                compute: false,
            }
        );
    }
}

#[test]
fn storage_bindings_reject_distinct_blocks_claiming_the_same_slot() {
    for (vertex, fragment, line) in [
        (
            VERTEX,
            "#version 450
            layout(binding=3) readonly buffer First { vec4 value; } first;
            layout(binding=3) readonly buffer Second { vec4 value; } second;
            out vec4 color; void main() { color = first.value + second.value; }",
            3,
        ),
        (
            "#version 450
            layout(binding=3) readonly buffer First { vec4 value; } first;
            void main() { gl_Position = first.value; }",
            "#version 450
            layout(binding=3) readonly buffer Second { vec4 value; } second;
            out vec4 color; void main() { color = second.value; }",
            2,
        ),
    ] {
        let translated = storage_translate(vertex, fragment);
        let failed = program(&translated, "composite");
        assert!(!failed.is_ok());
        assert!(failed.all_diagnostics().any(|d| {
            d.phase == Phase::Transform
                && d.location.as_ref().is_some_and(|location| {
                    location.file == "composite.fsh" && location.line == line
                })
        }));
    }
}

#[test]
fn storage_bindings_reject_cross_stage_binding_disagreement() {
    let translated = storage_translate(
        "#version 450
        layout(binding=2) readonly buffer Shared { vec4 value; } sharedData;
        void main() { gl_Position = sharedData.value; }",
        "#version 450
        layout(binding=4) readonly buffer Shared { vec4 value; } sharedData;
        out vec4 color; void main() { color = sharedData.value; }",
    );
    let failed = program(&translated, "composite");
    assert!(!failed.is_ok());
    assert!(failed.all_diagnostics().any(|d| {
        d.phase == Phase::Transform
            && d.location
                .as_ref()
                .is_some_and(|location| location.file == "composite.fsh" && location.line == 2)
    }));
}

#[test]
fn sampler_overloads_preserve_equal_arity_value_signatures() {
    let translated = success(
        VERTEX,
        "#version 450
        uniform sampler2D source;
        vec4 tap(sampler2D t, vec2 uv) { return texture(t, uv); }
        vec4 tap(sampler2D t, vec3 uv) { return texture(t, uv.xy) * uv.z; }
        out vec4 color;
        void main() { color = tap(source, vec2(0.5)) + tap(source, vec3(0.5)); }",
    );
    assert_eq!(
        reflection(program(&translated, "composite")).textures.len(),
        1
    );
}

#[test]
fn sampler_metadata_reads_flags_only_from_the_texture_object() {
    let texture = shaderpack::CustomTexture {
        stage: "composite".to_string(),
        sampler: "colortex0".to_string(),
        source: shaderpack::TextureSource::File("custom.png".to_string()),
    };
    for metadata in [
        r#"{"other":{"blur":true,"clamp":true},"texture":{"blur":false,"clamp":false}}"#,
        r#"{"blur":true,"clamp":true,"texture":{}}"#,
        r#"{"texture":{"blur":"true","clamp":1}}"#,
        r#"{"texture":{"blur":true,"clamp":true}} trailing"#,
    ] {
        let source = PackSource::from_files([("custom.png.mcmeta", metadata)]);
        assert_eq!(
            texture.sampler_class(&source),
            shaderpack::SamplerClass::RepeatNearest
        );
    }
}

#[test]
fn sampler_locals_cover_every_declarator_and_restore_outer_resources() {
    success(
        VERTEX,
        "#version 450
        uniform sampler2D specular;
        out vec4 color;
        void main() {
            float value = 0.0;
            { float unused = 0.0, specular = 1.0; value += unused + specular; }
            { float a = 0.0, specular = texture(specular, vec2(0.5)).x, b = specular; value += a + b; }
            color = texture(specular, vec2(0.5)) + vec4(value);
        }",
    );
}

#[test]
fn unused_fragment_inputs_are_removed_from_the_compiled_interface() {
    let translated = success(
        VERTEX,
        "#version 450\nin vec2 unusedUv; in mat3 unusedBasis; in float unusedValues[2];
        out vec4 color; void main() { color = vec4(1.0); }",
    );
    let stage = program(&translated, "composite")
        .stages
        .iter()
        .find(|stage| stage.stage == shaderpack::StageKind::Fragment)
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    assert!(stage.module.entry_points[0].function.arguments.is_empty());
}

#[test]
fn array_and_matrix_varyings_have_matching_compiled_locations() {
    let translated = success(
        "#version 450
        out mat2 basis; out vec2 uv[2]; flat out int index;
        void main() { basis = mat2(1.0); uv[0] = vec2(0.25); uv[1] = vec2(0.75);
            index = 1; gl_Position = vec4(0.0); }",
        "#version 450
        in mat2 basis; in vec2 uv[2]; flat in int index;
        out vec4 color; void main() { color = vec4(basis * (uv[0] + uv[1]), float(index), 1.0); }",
    );
    for stage in &program(&translated, "composite").stages {
        let locations: std::collections::BTreeSet<_> = interface_locations(
            &stage.result.as_ref().unwrap().module,
            stage.stage == shaderpack::StageKind::Vertex,
        )
        .into_iter()
        .map(|(_, location)| location)
        .collect();
        assert_eq!(locations, (0..5).collect());
    }
}

#[test]
fn explicit_block_locations_link_distinct_block_and_member_names() {
    let translated = success(
        "#version 450
        layout(location=2) out FIRST { vec2 uv; mat3 basis; } vertexData;
        out vec2 automatic;
        void main() { vertexData.uv = vec2(0.5); vertexData.basis = mat3(1.0);
        automatic = vec2(0.25); gl_Position = vec4(0.0); }",
        "#version 450
        layout(location=2) in SECOND { vec2 renamed; mat3 transform; } fragmentData;
        in vec2 automatic; out vec4 color;
        void main() { color = vec4((fragmentData.transform * vec3(fragmentData.renamed, 1.0)).xy + automatic, 0.0, 1.0); }",
    );
    for stage in &program(&translated, "composite").stages {
        let mut locations: Vec<_> = interface_locations(
            &stage.result.as_ref().unwrap().module,
            stage.stage == shaderpack::StageKind::Vertex,
        )
        .into_iter()
        .map(|(_, location)| location)
        .collect();
        locations.sort_unstable();
        assert_eq!(locations, [0, 2, 3, 4, 5]);
    }
}

#[test]
fn boolean_storage_writes_follow_array_and_swizzle_accesses() {
    for declaration in ["bool flags[2];", "bvec2 flags;", "bvec2 flags[2];"] {
        let (write, read) = match declaration {
            "bool flags[2];" => ("flags[0] = true; flags[1] = flags[0];", "flags[1]"),
            "bvec2 flags;" => ("flags.x = true; flags[1] = flags.x;", "flags.y"),
            _ => (
                "flags[0].x = true; flags[1].xy = bvec2(flags[0].x);",
                "flags[1].y",
            ),
        };
        let source = format!(
            "#version 450
            layout(local_size_x=1) in;
            layout(std430, binding=0) buffer State {{ {declaration} }};
            void main() {{ {write} if ({read}) {{ {write} }} }}"
        );
        let pack = ShaderPack::new(PackSource::from_files([("begin.csh", source)]));
        let translated = pack.translate(&TranslateConfig::default());
        let reflected = reflection(program(&translated, "begin"));
        assert!(!reflected.storage_buffers[0].read_only);
    }
}
