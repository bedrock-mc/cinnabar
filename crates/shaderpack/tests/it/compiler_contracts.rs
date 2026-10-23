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
    assert_eq!(locations(vertex, true), [0, 1]);
    assert_eq!(locations(fragment, false), [0, 1]);
    assert_eq!(
        interface_locations(vertex, true),
        [("automaticUv", 1), ("sourceUv", 0)]
    );
    assert_eq!(
        interface_locations(fragment, false),
        [("automaticUv", 1), ("uv", 0)]
    );
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

#[test]
fn compiler_calls_with_unmatched_delimiters_report_diagnostics() {
    for body in [
        "void main() { gl_Position = texture2D(",
        "void main() { gl_Position = shadow2D(",
        "void main() { float x[] = float[](",
    ] {
        let translation = translate(&format!("#version 120\n{body}"), FRAGMENT);
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok());
        assert!(failed.all_diagnostics().next().is_some());
    }
}

#[test]
fn compiler_reserved_members_follow_declaration_names() {
    success(
        VERTEX,
        "#version 330\nstruct S { vec4 sampler; }; out vec4 color; void main() { S s; s.sampler = vec4(1.0); color = s.sampler; }",
    );
    success(
        VERTEX,
        "#version 120\nstruct S { vec4 sample; vec4 texture; }; void main() { S s; s.sample = vec4(1.0); s.texture = s.sample; gl_FragColor = s.texture; }",
    );
}

#[test]
fn compiler_boolean_uniform_elements_convert_after_access() {
    success(
        VERTEX,
        "#version 330\nuniform bool flags[2]; uniform bvec2 masks[2]; out vec4 color; void main() { int i = 1; color = vec4(flags[i] && masks[i].x ? 1.0 : 0.0); }",
    );
}

#[test]
fn compiler_boolean_uniforms_respect_lexical_bindings() {
    success(
        VERTEX,
        "#version 330\nuniform bool enabled; out vec4 color; bool choose(bool enabled) { return enabled; } void main() { bool result = choose(true); { bool enabled = false; result = result && enabled; } color = vec4(result && enabled ? 1.0 : 0.0); }",
    );
    success(
        VERTEX,
        "#version 330\nuniform bool enabled; out vec4 color; void main() { bool unused = false, enabled = true; color = vec4(enabled ? 1.0 : 0.0); }",
    );
}

#[test]
fn compiler_specialized_helpers_have_forward_declarations() {
    success(
        VERTEX,
        "#version 330\nuniform sampler2D source; vec4 read(sampler2D s, vec2 uv); out vec4 color; void main() { color = read(source, vec2(0.5)); } vec4 read(sampler2D s, vec2 uv) { return texture(s, uv); }",
    );
}

#[test]
fn compiler_uniforms_share_struct_and_constant_dependencies() {
    success(
        VERTEX,
        "#version 330\nconst int COUNT = 2; struct Light { vec4 color; }; uniform Light lights[COUNT]; out vec4 color; void main() { color = lights[1].color; }",
    );
}

#[test]
fn compiler_rejects_missing_declaration_separators() {
    let translation = translate(
        VERTEX,
        "#version 330\nout vec4 color; void main() { float a b c d; color = vec4(1.0); }",
    );
    let failed = program(&translation, "composite");
    assert!(!failed.is_ok());
    assert!(
        failed
            .all_diagnostics()
            .any(|diagnostic| diagnostic.phase == Phase::Parse)
    );
}

#[test]
fn compiler_rejects_imported_dependency_collisions() {
    for count in [3, 2] {
        let translation = translate(
            &format!(
                "#version 330\nconst int COUNT = {count}; void main() {{ gl_Position = vec4(float(COUNT)); }}"
            ),
            "#version 330\nconst int COUNT = 2; uniform float weights[COUNT]; out vec4 color; void main() { color = vec4(weights[1]); }",
        );
        let translated = program(&translation, "composite");
        if count == 2 {
            reflection(translated);
        } else {
            assert!(!translated.is_ok());
            assert!(translated.all_diagnostics().any(|diagnostic| {
                diagnostic.phase == Phase::Link && diagnostic.message.contains("COUNT")
            }));
        }
    }
}

#[test]
fn compiler_loop_bindings_end_after_the_loop_statement() {
    success(
        VERTEX,
        "#version 330\nuniform bool enabled; out vec4 color; void main() { float value = 0.0; for (int enabled = 0; enabled < 2; enabled++) { value += float(enabled); } color = vec4(enabled ? value : 0.0); }",
    );
}

#[test]
fn compiler_specialization_respects_local_parameter_shadows() {
    success(
        VERTEX,
        "#version 330\nuniform sampler2D plane; out vec4 color; vec4 read(sampler2D source, vec2 uv) { vec4 result = texture(source, uv); { float source = 1.0; result += vec4(source); } return result; } void main() { color = read(plane, vec2(0.5)); }",
    );
}

#[test]
fn compiler_specialization_distinguishes_shadowed_resource_arguments() {
    let translation = success(
        VERTEX,
        "#version 330\nuniform sampler2D plane; out vec4 color; vec4 read(sampler2D source, vec2 uv) { return texture(source, uv); } vec4 read(float value, vec2 uv) { return vec4(value + uv.x); } void main() { color = read(plane, vec2(0.5)); { float plane = 1.0; color += read(plane, vec2(0.5)); } }",
    );
    let module = &program(&translation, "composite")
        .stages
        .iter()
        .find(|stage| stage.stage == shaderpack::StageKind::Fragment)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .module;
    let (value_helper, _) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("read"))
        .expect("value overload remains");
    assert!(
        module
            .functions
            .iter()
            .map(|(_, function)| function)
            .chain(module.entry_points.iter().map(|entry| &entry.function))
            .any(|function| function.expressions.iter().any(|(_, expression)| {
                matches!(expression, naga::Expression::CallResult(callee) if *callee == value_helper)
            })),
        "the shadowed float argument must still call the value overload"
    );
}

#[test]
fn compiler_specialization_avoids_capturing_resource_names() {
    for helper in [
        "vec4 tap(sampler2D s, vec2 source) { return texture(s, source); }",
        "vec4 tap(sampler2D s, vec2 uv) { vec2 source = uv; return texture(s, source); }",
        "vec4 tap(sampler2D s, vec2 uv) { vec4 result = texture(s, uv); { vec2 source = uv; result += texture(s, source); } return result; }",
        "vec4 tap(sampler2D s, vec2 uv) { vec4 result = texture(s, uv); for (int source = 0; source < 2; source++) result += texture(s, uv) * float(source); return result + texture(source, uv); }",
        "vec4 tap(sampler2D s, vec2 uv) { vec2 unused = uv, source = uv; return texture(s, source); }",
        "vec4 tap(sampler2D s, vec2 uv) { struct Data { vec2 source; }; Data value; value.source = uv; return texture(s, value.source); }",
        "vec4 tap(sampler2D s, vec2 uv) { struct Data { vec2 uv; } source; source.uv = uv; return texture(s, source.uv); }",
    ] {
        success(
            VERTEX,
            &format!(
                "#version 330\nuniform sampler2D source;\n{helper}\nout vec4 color; void main() {{ color = tap(source, vec2(0.5)); }}"
            ),
        );
    }
}

#[test]
fn compiler_specializes_nested_calls_without_overlapping_edits() {
    success(
        VERTEX,
        "#version 330
        uniform sampler2D source;
        vec4 tap(sampler2D s, vec2 uv) { return texture(s, uv); }
        vec4 join(vec4 a, sampler2D s, vec4 b, sampler2D t) {
            return a + texture(s, b.xy) + texture(t, b.zw);
        }
        out vec4 color;
        void main() {
            color = join(tap(source, tap(source, vec2(0.5)).xy), source,
                tap(source, vec2(0.25)), source);
        }",
    );
}

#[test]
fn compiler_attribute_location_overflow_reports_diagnostics() {
    for declaration in [
        "vec4 huge[4294967295u]",
        "mat4 huge[4294967295u]",
        "dvec4 huge[4294967295u]",
        "vec4[4294967295u] huge",
        "vec4 huge[65536][65536]",
    ] {
        let translation = translate(
            &format!("#version 450\nin {declaration};\nvoid main() {{ gl_Position = vec4(0.0); }}"),
            FRAGMENT,
        );
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok());
        assert!(failed.all_diagnostics().any(|diagnostic| {
            diagnostic.phase == Phase::Transform
                && diagnostic.message.contains("attribute location overflow")
                && diagnostic
                    .location
                    .as_ref()
                    .is_some_and(|location| location.file == "composite.vsh" && location.line == 2)
        }));
    }
}

#[test]
fn compiler_resource_specializations_have_distinct_names() {
    let translation = success(
        VERTEX,
        "#version 330\nuniform sampler2D a_b, c, a, b_c; out vec4 color; vec4 combine(sampler2D x, sampler2D y) { return texture(x, vec2(0.5)) + texture(y, vec2(0.5)); } void main() { color = combine(a_b, c) + combine(a, b_c); }",
    );
    assert_eq!(
        reflection(program(&translation, "composite"))
            .textures
            .len(),
        4
    );
}

#[test]
fn compiler_specializations_distinguish_opaque_parameter_positions() {
    success(
        VERTEX,
        "#version 330
        uniform sampler2D source;
        vec4 tap(sampler2D s, vec2 uv) { return texture(s, uv); }
        vec4 tap(vec2 uv, sampler2D s) { return 2.0 * texture(s, uv); }
        out vec4 color;
        void main() { color = tap(source, vec2(0.25)) + tap(vec2(0.75), source); }",
    );
}

#[test]
fn compiler_cyclic_varyings_report_transform_diagnostics() {
    for declarations in [
        "struct S { S child; };\nout S value;",
        "struct S { S child[2]; };\nout S value[2];",
        "struct S { T child; }; struct T { S child; };\nout T value;",
    ] {
        let translation = translate(
            &format!("#version 330\n{declarations}\nvoid main() {{ gl_Position = vec4(0.0); }}"),
            FRAGMENT,
        );
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok());
        assert!(
            failed.all_diagnostics().any(|diagnostic| {
                diagnostic.phase == Phase::Transform
                    && diagnostic.message.contains("cyclic struct")
                    && diagnostic.location.as_ref().is_some_and(|location| {
                        location.file == "composite.vsh" && location.line == 3
                    })
            }),
            "{:#?}",
            failed.all_diagnostics().collect::<Vec<_>>()
        );
    }
}

#[test]
fn compiler_cyclic_declarations_report_diagnostics() {
    for declaration in [
        "S value;",
        "uniform S value;",
        "uniform Values { S value; };",
        "layout(std430) buffer Values { S value; };",
    ] {
        let translation = translate(
            VERTEX,
            &format!(
                "#version 450\nstruct S {{ S child; }};\n{declaration}\nout vec4 color; void main() {{ color = vec4(1.0); }}"
            ),
        );
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok());
        assert!(failed.all_diagnostics().next().is_some());
    }
}

#[test]
fn boolean_array_lengths_keep_the_array_receiver() {
    success(
        VERTEX,
        "#version 330\nuniform bool flags[2]; out vec4 color; void main() { color = vec4(float(flags.length())); }",
    );
}
#[test]
fn atomic_access_is_merged_across_render_stages() {
    let translation = success(
        "#version 450\nlayout(r32ui, binding=0) uniform uimage2D counter; void main() { gl_Position = vec4(float(imageLoad(counter, ivec2(0)).r)); }",
        "#version 450\nlayout(r32ui, binding=0) uniform uimage2D counter; out vec4 color; void main() { imageAtomicAdd(counter, ivec2(0), 1u); color = vec4(float(imageLoad(counter, ivec2(0)).r)); }",
    );
    let image = &reflection(program(&translation, "composite")).storage_images[0];
    assert!(image.read && image.write && image.atomic);
    assert!(image.stages.vertex && image.stages.fragment);
}

/// Checks that every stage retains distinct global variable bindings.
fn assert_unique_globals(translation: &PackTranslation) {
    for stage in &program(translation, "composite").stages {
        let module = &stage.result.as_ref().unwrap().module;
        let names: Vec<_> = module
            .global_variables
            .iter()
            .filter_map(|(_, variable)| variable.name.as_deref())
            .collect();
        let unique: std::collections::BTreeSet<_> = names.iter().copied().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "duplicate global bindings: {names:?}"
        );
    }
}

#[test]
fn declaration_lists_preserve_alias_names_after_arrays_and_initializers() {
    for declarations in [
        "uniform mat4 view[2], projectionMatrix;",
        "mat4 view[2], projectionMatrix = mat4(1.0);",
    ] {
        success(
            &format!(
                "#version 450\n{declarations} void main() {{ gl_Position = projectionMatrix * view[0][0]; }}"
            ),
            FRAGMENT,
        );
    }
}

#[test]
fn declaration_aliases_respect_parameters_locals_and_members() {
    success(
        "#version 450\nstruct S { float projectionMatrix; }; float choose(float projectionMatrix) { return projectionMatrix; } void main() { S value; value.projectionMatrix = 1.0; float amount = choose(value.projectionMatrix); { float projectionMatrix = 2.0; amount += projectionMatrix; } gl_Position = projectionMatrix * vec4(amount); }",
        FRAGMENT,
    );
}

#[test]
fn declaration_aliases_preserve_local_struct_fields() {
    success(
        "#version 450\nvoid main() { struct S { float projectionMatrix; }; S value; value.projectionMatrix = 0.5; gl_Position = projectionMatrix * vec4(value.projectionMatrix); }",
        FRAGMENT,
    );
}

#[test]
fn declaration_aliases_preserve_block_fields() {
    success(
        "#version 450\nout Data { float projectionMatrix; } data; void main() { data.projectionMatrix = 0.5; gl_Position = projectionMatrix * vec4(data.projectionMatrix); }",
        "#version 450\nin Data { float projectionMatrix; } data; out vec4 color; void main() { color = vec4(data.projectionMatrix); }",
    );
}

#[test]
fn declaration_block_members_with_underscores_keep_distinct_bindings() {
    let translated = success(
        "#version 450\nout A_B { vec4 c; } first; out A { vec4 B_c; } second; void main() { first.c = vec4(0.25); second.B_c = vec4(0.75); gl_Position = vec4(0.0); }",
        "#version 450\nin A_B { vec4 c; } first; in A { vec4 B_c; } second; out vec4 color; void main() { color = first.c + second.B_c; }",
    );
    assert_unique_globals(&translated);
}

#[test]
fn declaration_generated_interfaces_do_not_hide_pack_globals() {
    let translated = success(
        "#version 450\nvec4 iris_b_DATA_color_0; out DATA { vec4 color; } data; void main() { iris_b_DATA_color_0 = vec4(0.25); data.color = vec4(0.75); gl_Position = iris_b_DATA_color_0; }",
        "#version 450\nvec4 iris_out_color; in DATA { vec4 color; } data; out vec3 color; void main() { iris_out_color = data.color; color = iris_out_color.rgb; }",
    );
    assert_unique_globals(&translated);
}

#[test]
fn declaration_converted_inputs_do_not_hide_pack_globals() {
    let translated = success(
        "#version 450\nin vec4 vaPosition; vec3 iris_float_vaPosition = vec3(0.25); void main() { gl_Position = vaPosition + vec4(iris_float_vaPosition, 0.0); }",
        FRAGMENT,
    );
    assert!(
        reflection(program(&translated, "composite"))
            .vertex_inputs
            .iter()
            .any(|input| input.attribute == shaderpack::VertexAttribute::Position)
    );
    assert_unique_globals(&translated);
}

#[test]
fn numeric_returns_preserve_comma_expressions() {
    success(
        VERTEX,
        "#version 450\nout vec4 color; float choose(float a, float b) { return a, b; } void main() { color = vec4(choose(1.0, 2.0)); }",
    );
}

#[test]
fn boolean_storage_assignments_keep_boolean_values_and_boundaries() {
    for expression in [
        "if (flag = true) { mask.x = false; }",
        "bool result = true ? flag = true : false; mask.x = result;",
        "bool result = true ? flag = false, flag = true : false; mask.x = result;",
        "if (flag = mask.x = true) { mask.y = flag; }",
        "if (mask[0] = flag = true) { mask.xy = bvec2(flag); }",
        "if (all(mask.xy = bvec2(true))) { flag = mask.x; }",
        "flag = true ? false : true;",
    ] {
        let source = format!(
            "#version 450\nlayout(local_size_x=1) in; layout(std430, binding=0) buffer State {{ bool flag; bvec2 mask; }}; void main() {{ {expression} }}"
        );
        let translation = ShaderPack::new(PackSource::from_files([("begin.csh", source)]))
            .translate(&TranslateConfig::default());
        assert!(
            program(&translation, "begin").is_ok(),
            "{expression}: {:?}",
            program(&translation, "begin")
                .all_diagnostics()
                .collect::<Vec<_>>()
        );
        reflection(program(&translation, "begin"));
    }
}

#[test]
fn dot_calls_preserve_required_constant_contexts() {
    success(
        VERTEX,
        "#version 450\nconst float globalScale = dot(vec2(1.0), vec2(2.0)); out vec4 color; void main() { float values[int(dot(vec2(1.0), vec2(2.0)))]; const float scale = dot(vec2(1.0), vec2(2.0)); int n = 4; switch(n) { case int(dot(vec2(1.0), vec2(2.0))): values[0] = scale; break; default: values[0] = 0.0; } color = vec4(values[0]); }",
    );
}

#[test]
fn dot_calls_preserve_user_overloads() {
    success(
        VERTEX,
        "#version 450\nout vec4 color; float dot(vec2 a) { return a.x; } float dot(float a, vec2 b) { return a + b.x; } void main() { vec2 p = vec2(0.5); color = vec4(dot(p) + dot(1.0, p) + dot(p, p)); }",
    );
}

#[test]
fn const_parameters_do_not_suppress_runtime_dot_rounding() {
    let translation = success(
        VERTEX,
        "#version 450\nout vec4 color; float hash(const vec2 p) { return dot(p, p); } void main() { color = vec4(hash(gl_FragCoord.xy)); }",
    );
    let module = &program(&translation, "composite")
        .stages
        .iter()
        .find(|stage| stage.stage == shaderpack::StageKind::Fragment)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .module;
    assert!(
        module
            .functions
            .iter()
            .any(
                |(_, function)| function.expressions.iter().any(|(_, expression)| matches!(
                    expression,
                    naga::Expression::Math {
                        fun: naga::MathFunction::Fma,
                        ..
                    }
                ))
            )
    );
}

#[test]
fn integer_packing_preserves_constant_expressions() {
    let translated = success(
        VERTEX,
        "#version 450\nout vec4 color; const uint packed = packUint2x16(u16vec2(1, 2)); const uvec2 unpacked = unpackUint2x16(packed); const int signedPacked = packInt2x16(i16vec2(-1, -2)); const ivec2 signedParts = unpackInt2x16(signedPacked); void main() { const uint local = packUint2x16(u16vec2(unpacked)); float values[int(unpackUint2x16(packed).x)]; values[0] = float(signedParts.x); color = vec4(values[0] + float(local)); }",
    );
    let module = &program(&translated, "composite")
        .stages
        .iter()
        .find(|stage| stage.stage == shaderpack::StageKind::Fragment)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .module;
    for (name, expected) in [
        ("packed", naga::Literal::U32(131073)),
        ("signedPacked", naga::Literal::I32(-65537)),
    ] {
        let (_, constant) = module
            .constants
            .iter()
            .find(|(_, constant)| constant.name.as_deref() == Some(name))
            .unwrap();
        assert!(
            matches!(module.global_expressions[constant.init], naga::Expression::Literal(value) if value == expected)
        );
    }
}

#[test]
fn boolean_assignment_ternary_separator_keeps_both_operands_boolean() {
    let source = "#version 450\nlayout(local_size_x=1) in; layout(std430, binding=0) buffer State { bool flag; }; void main() { bool result = true ? flag = true : false; flag = result; }";
    let translation = ShaderPack::new(PackSource::from_files([("begin.csh", source)]))
        .translate(&TranslateConfig::default());
    reflection(program(&translation, "begin"));
}

#[test]
fn numeric_helpers_do_not_hide_pack_functions() {
    success(
        VERTEX,
        "#version 450\nout vec4 color; float iris_dot(vec2 a, vec2 b) { return a.x; } uint iris_packUint2x16(uvec2 v) { return v.x; } void main() { vec2 p = gl_FragCoord.xy; uvec2 bits = uvec2(p); color = vec4(dot(p, p) + iris_dot(p, p) + float(packUint2x16(bits)) + float(iris_packUint2x16(bits))); }",
    );
}

#[test]
fn dot_overloads_keep_prototypes_and_builtin_replacements() {
    success(
        VERTEX,
        "#version 450\nout vec4 color; float dot(vec2); float dot(float, float); void main() { color = vec4(dot(vec2(1.0)) + dot(2.0, 3.0)); } float dot(vec2 a) { return a.x; } float dot(float a, float b) { return a - b; }",
    );
}

#[test]
fn numeric_calls_with_unmatched_delimiters_report_diagnostics() {
    for body in [
        "void main() { float x = dot(",
        "void main() { uint x = packUint2x16(",
    ] {
        let translation = translate(&format!("#version 450\n{body}"), FRAGMENT);
        assert!(!program(&translation, "composite").is_ok());
        assert!(
            program(&translation, "composite")
                .all_diagnostics()
                .next()
                .is_some()
        );
    }
}

#[test]
fn integer_packing_in_case_labels_is_not_a_function_declaration() {
    success(
        VERTEX,
        "#version 450\nout vec4 color; void main() { switch(int(gl_FragCoord.x)) { case int(packUint2x16(uvec2(1u, 0u))): color = vec4(1.0); break; default: color = vec4(0.0); } }",
    );
}
