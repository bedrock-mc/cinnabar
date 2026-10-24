//! Samplers: per-texture sampler classes (render targets clamp, depth is nearest, custom
//! textures follow their `.mcmeta`), locals shadowing a sampler, and pack helpers that take
//! a sampler getting a copy per global texture.

use crate::{program, reflection};
use shaderpack::{
    CompiledStage, PackSource, SHADOW_SAMPLER_BINDING, SamplerClass, ShaderPack, StageKind,
    TEXTURE_GROUP, TranslateConfig,
};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; }\n";

/// Builds an in-memory program with properties and optional texture metadata.
fn pack(fragment: &str, properties: &str, extra: &[(&str, &str)]) -> ShaderPack {
    let mut files = vec![
        ("composite.vsh", VERTEX.to_owned()),
        ("composite.fsh", fragment.to_owned()),
        ("shaders.properties", properties.to_owned()),
    ];
    files.extend(extra.iter().map(|(k, v)| (*k, (*v).to_owned())));
    ShaderPack::new(PackSource::from_files(files))
}

/// Returns the validated fragment stage.
fn fragment_stage(t: &shaderpack::ProgramTranslation) -> &CompiledStage {
    let stage: &CompiledStage = t
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Fragment)
        .and_then(|s| s.result.as_ref().ok())
        .unwrap_or_else(|| {
            panic!(
                "fragment compiles: {:#?}",
                t.all_diagnostics().collect::<Vec<_>>()
            )
        });
    stage
}

/// Returns translated fragment source for sampler-construction checks.
fn fragment_glsl(t: &shaderpack::ProgramTranslation) -> &str {
    &fragment_stage(t).glsl
}

#[test]
fn a_helper_taking_a_sampler_gets_a_copy_per_global_texture() {
    let fragment = "#version 120\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D colortex1;\n\
        varying vec2 uv;\n\
        vec4 tap(sampler2D s) { return texture2D(s, uv); }\n\
        void main() { gl_FragData[0] = tap(colortex0) + tap(colortex1); }\n";
    let t = pack(fragment, "", &[]).translate(&TranslateConfig::default());
    let translated = program(&t, "composite");
    let stage = fragment_stage(translated);
    let sampling_functions = stage
        .module
        .functions
        .iter()
        .filter(|(_, function)| {
            function
                .expressions
                .iter()
                .any(|(_, expression)| matches!(expression, naga::Expression::ImageSample { .. }))
        })
        .count();
    assert_eq!(sampling_functions, 2);
    assert_eq!(crate::reflection(translated).textures.len(), 2);
}

#[test]
fn each_texture_reads_through_the_class_iris_gives_it() {
    let fragment = "#version 400 compatibility\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D depthtex0;\n\
        uniform sampler2D noisetex;\n\
        uniform sampler2D gaux1;\n\
        uniform sampler2D dust;\n\
        uniform sampler2D grain;\n\
        uniform sampler2D lut;\n\
        uniform sampler2D unknownTex;\n\
        uniform usampler2D integerTex;\n\
        uniform sampler2DShadow shadowtex0;\n\
        varying vec2 uv;\n\
        vec4 tap(sampler2D s) { return texture2D(s, uv); }\n\
        void main() {\n\
            gl_FragData[0] = texture2D(colortex0, uv) + texture2D(depthtex0, uv) + texture2D(noisetex, uv)\n\
                + tap(gaux1) + texture2D(dust, uv) + texture2D(grain, uv) + texture2D(lut, uv)\n\
                + texture2D(unknownTex, uv) + vec4(shadow2D(shadowtex0, vec3(uv, 0.5)).r)\n\
                + vec4(textureGather(integerTex, uv));\n\
        }\n";
    let properties = "texture.composite.dust = tex/dust.png\n\
        texture.composite.grain = tex/grain.png\n\
        texture.composite.lut = tex/lut.dat TEXTURE_2D RGBA8 16 16 RGBA UNSIGNED_BYTE\n\
        texture.deferred.colortex0 = tex/ignored.png\n";
    let mcmeta = r#"{ "texture": { "blur": true, "clamp" : false } }"#;
    let p = pack(fragment, properties, &[("tex/grain.png.mcmeta", mcmeta)]);
    let t = p.translate(&TranslateConfig::default());
    let composite = program(&t, "composite");
    let r = reflection(composite);
    let classes: Vec<(String, SamplerClass)> = r
        .samplers
        .iter()
        .filter_map(|s| s.class().map(|c| (s.name.clone(), c)))
        .collect();
    assert!(
        classes
            .iter()
            .all(|(name, class)| name == class.glsl_name())
    );
    let mut bound: Vec<SamplerClass> = classes.iter().map(|(_, c)| *c).collect();
    bound.sort();
    assert_eq!(bound, SamplerClass::ALL, "every class is used once");
    assert!(r.samplers.iter().any(|s| s.comparison
        && s.group == TEXTURE_GROUP
        && s.binding == SHADOW_SAMPLER_BINDING
        && s.class().is_none()));

    let glsl = fragment_glsl(composite);
    let read = |texture: &str, class: SamplerClass| {
        let pair = format!("sampler2D({texture}, {})", class.glsl_name());
        assert!(glsl.contains(&pair), "missing `{pair}` in\n{glsl}");
    };
    read("colortex0", SamplerClass::ClampLinear);
    // The helper was specialized for the global, so the colour target keeps its class.
    read("gaux1", SamplerClass::ClampLinear);
    read("depthtex0", SamplerClass::ClampNearest);
    read("noisetex", SamplerClass::RepeatLinear);
    read("unknownTex", SamplerClass::RepeatLinear);
    // PNG without .mcmeta: nearest, repeat. With blur and no clamp: linear, repeat.
    read("dust", SamplerClass::RepeatNearest);
    read("grain", SamplerClass::RepeatLinear);
    // Raw textures default to blur and clamp.
    read("lut", SamplerClass::ClampLinear);
}

#[test]
fn custom_texture_classes_follow_the_program_stage_and_noise_override() {
    let fragment = "#version 120\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D noisetex;\n\
        varying vec2 uv;\n\
        void main() { gl_FragData[0] = texture2D(colortex0, uv) + texture2D(noisetex, uv); }\n";
    // `colortex0` is only replaced in deferred programs; `texture.noise` applies everywhere
    // and its clamp-only .mcmeta makes it nearest.
    let properties = "texture.deferred.colortex0 = tex/a.png\ntexture.noise = tex/noise.png\n";
    let mcmeta = r#"{"texture":{"clamp":true}}"#;
    let p = pack(fragment, properties, &[("tex/noise.png.mcmeta", mcmeta)]);
    let t = p.translate(&TranslateConfig::default());
    let glsl = fragment_glsl(program(&t, "composite"));
    assert!(glsl.contains("sampler2D(colortex0, iris_samplerClampLinear)"));
    assert!(glsl.contains("sampler2D(noisetex, iris_samplerClampNearest)"));
}

#[test]
fn a_custom_texture_only_reclasses_samplers_of_its_shape() {
    // A 3D volume under `colortex0`: the 2D buffer read keeps its clamp/linear class, the
    // 3D read takes the volume's .mcmeta (no blur, no clamp).
    let fragment = "#version 120\n\
        uniform sampler2D colortex0;\n\
        varying vec2 uv;\n\
        void main() { gl_FragData[0] = texture2D(colortex0, uv); }\n";
    let volume = "#version 120\n\
        uniform sampler3D colortex0;\n\
        varying vec2 uv;\n\
        void main() { gl_FragData[0] = texture3D(colortex0, vec3(uv, 0.5)); }\n";
    let properties = "texture.composite.colortex0 = v.dat TEXTURE_3D R8 2 2 2 RED UNSIGNED_BYTE\n";
    let mcmeta = r#"{"texture":{"blur":false,"clamp":false}}"#;
    let p = pack(
        fragment,
        properties,
        &[
            ("v.dat.mcmeta", mcmeta),
            ("composite1.vsh", VERTEX),
            ("composite1.fsh", volume),
        ],
    );
    let t = p.translate(&TranslateConfig::default());
    let flat = fragment_glsl(program(&t, "composite"));
    assert!(flat.contains("sampler2D(colortex0, iris_samplerClampLinear)"));
    let deep = fragment_glsl(program(&t, "composite1"));
    assert!(
        deep.contains("sampler3D(colortex0, iris_samplerRepeatNearest)"),
        "{deep}"
    );
}

#[test]
fn a_flipped_buffer_reads_through_its_own_class_after_the_stage_binding_releases() {
    // Iris's `flippedAtLeastOnce`: once a pass of the stage wrote `colortex0`, later passes
    // (and `final` after composite) read the buffer, not `texture.<stage>.colortex0`. Each
    // stage starts over, so `composite` still reads the image after `deferred` wrote it.
    // Raw textures rename the stage's samplers instead, so flips never release them.
    let copy = "#version 120\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D colortex2;\n\
        varying vec2 uv;\n\
        void main() {\n\
        /* DRAWBUFFERS:02 */\n\
            gl_FragData[0] = texture2D(colortex0, uv);\n\
            gl_FragData[1] = texture2D(colortex2, uv);\n\
        }\n";
    let show = "#version 120\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D colortex2;\n\
        varying vec2 uv;\n\
        void main() { gl_FragColor = texture2D(colortex0, uv) + texture2D(colortex2, uv); }\n";
    let properties = "texture.composite.colortex0 = tex/clouds.png\n\
        texture.deferred.colortex0 = tex/clouds.png\n\
        texture.composite.colortex2 = tex/lut.dat TEXTURE_2D RGBA8 1 1 RGBA UNSIGNED_BYTE\n";
    let mcmeta = r#"{"texture":{"blur":false,"clamp":false}}"#;
    let p = pack(
        copy,
        properties,
        &[
            ("tex/lut.dat.mcmeta", mcmeta),
            ("deferred.vsh", VERTEX),
            ("deferred.fsh", copy),
            ("composite1.vsh", VERTEX),
            ("composite1.fsh", copy),
            ("final.vsh", VERTEX),
            ("final.fsh", show),
        ],
    );
    let t = p.translate(&TranslateConfig::default());
    for (name, released, image) in [
        ("deferred", 0, SamplerClass::RepeatNearest),
        ("composite", 0, SamplerClass::RepeatNearest),
        ("composite1", 0b101, SamplerClass::ClampLinear),
        ("final", 0b101, SamplerClass::ClampLinear),
    ] {
        let translated = program(&t, name);
        assert_eq!(translated.released_overrides, released, "{name}");
        let glsl = fragment_glsl(translated);
        let raw = if name == "deferred" {
            SamplerClass::ClampLinear
        } else {
            SamplerClass::RepeatNearest
        };
        for (texture, class) in [("colortex0", image), ("colortex2", raw)] {
            let pair = format!("sampler2D({texture}, {})", class.glsl_name());
            assert!(glsl.contains(&pair), "{name}: missing `{pair}` in\n{glsl}");
        }
    }
}

#[test]
fn locals_and_parameters_shadowing_a_sampler_stay_plain() {
    // Complementary's GGX declares `float specular` while `specular` is the global sampler;
    // SEUS's `vec4 tex = texture2D(tex, …)` reads the global in the initializer.
    let fragment = "#version 120\n\
        uniform sampler2D specular;\n\
        varying vec2 uv;\n\
        float ggx(float x) { float specular = x * 2.0; specular = specular / (specular + 1.0); return specular; }\n\
        float scale(float specular) { return specular * 0.5; }\n\
        void main() {\n\
            float s = ggx(uv.x);\n\
            { vec4 specular = texture2D(specular, uv) + vec4(s); s += specular.x; }\n\
            gl_FragData[0] = texture2D(specular, uv) * scale(s);\n\
        }\n";
    let p = pack(fragment, "", &[]);
    let t = p.translate(&TranslateConfig::default());
    let module = &fragment_stage(program(&t, "composite")).module;
    let ggx = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("ggx"))
        .unwrap()
        .1;
    let local = ggx
        .local_variables
        .iter()
        .find(|(_, variable)| variable.name.as_deref() == Some("specular"))
        .unwrap()
        .1;
    assert!(matches!(
        module.types[local.ty].inner,
        naga::TypeInner::Scalar(naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 4
        })
    ));
    let scale = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("scale"))
        .unwrap()
        .1;
    assert!(matches!(
        module.types[scale.arguments[0].ty].inner,
        naga::TypeInner::Scalar(naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 4
        })
    ));
    assert_eq!(
        module
            .functions
            .iter()
            .flat_map(|(_, function)| function.expressions.iter())
            .filter(|(_, expression)| matches!(expression, naga::Expression::ImageSample { .. }))
            .count(),
        2
    );
}

#[test]
fn overloads_taking_a_sampler_each_get_a_specialized_copy() {
    let fragment = "#version 120\n\
        #====================#\n\
        #   Copyright banner #\n\
        uniform sampler2D depthtex1;\n\
        varying vec2 uv;\n\
        vec4 trace(sampler2D depth, vec2 p, float s) { return texture2D(depth, p) * s; }\n\
        vec3 trace(sampler2D depth, vec2 p, float s, out float hit) { vec4 c = trace(depth, p, s); hit = c.a; return c.rgb; }\n\
        void main() { float hit; vec3 c = trace(depthtex1, uv, 2.0, hit); gl_FragData[0] = vec4(c, hit); }\n";
    let t = pack(fragment, "", &[]).translate(&TranslateConfig::default());
    let stage = fragment_stage(program(&t, "composite"));
    let mut arities: Vec<_> = stage
        .module
        .functions
        .iter()
        .map(|(_, function)| function.arguments.len())
        .filter(|arity| *arity > 0)
        .collect();
    arities.sort_unstable();
    assert_eq!(arities, [2, 3]);
}

#[test]
fn sampler_class_bindings_precede_textures_and_round_trip() {
    for class in SamplerClass::ALL {
        assert_eq!(SamplerClass::from_binding(class.binding()), Some(class));
        assert!(class.binding() < shaderpack::FIRST_TEXTURE_BINDING);
        assert_ne!(class.binding(), SHADOW_SAMPLER_BINDING);
    }
    assert_eq!(
        SamplerClass::for_name("shadowcolor1"),
        SamplerClass::ClampLinear
    );
    assert_eq!(
        SamplerClass::for_name("gdepthtex"),
        SamplerClass::ClampNearest
    );
    assert_eq!(SamplerClass::for_name("gnormal"), SamplerClass::ClampLinear);
    assert_eq!(
        SamplerClass::for_name("noisetex"),
        SamplerClass::RepeatLinear
    );
    for atlas in ["gtexture", "texture", "tex", "normals", "specular"] {
        assert_eq!(SamplerClass::for_name(atlas), SamplerClass::RepeatNearest);
    }
    assert_eq!(
        SamplerClass::for_name("lightmap"),
        SamplerClass::ClampLinear
    );
    assert_eq!(shaderpack::ProgramGroup::Final.texture_stage(), "composite");
    let unit = shaderpack::CustomTexture {
        stage: "composite".to_owned(),
        sampler: "colortex6.1".to_owned(),
        source: shaderpack::TextureSource::File("a.png".to_owned()),
    };
    assert_eq!(unit.uniform(), "colortex6");
}
#[test]
fn sampler_compatibility_buffer_aliases_share_custom_classes() {
    for (property, uniform) in [
        ("gaux1", "colortex4"),
        ("colortex4", "gaux1"),
        ("shadowcolor", "shadowcolor0"),
    ] {
        let fragment = format!(
            "#version 330\nuniform sampler2D {uniform}; out vec4 color; void main() {{ color = texture({uniform}, vec2(0.5)); }}"
        );
        let properties = format!("texture.composite.{property} = sample.png");
        let translated = pack(&fragment, &properties, &[]).translate(&TranslateConfig::default());
        let classes: Vec<_> = reflection(program(&translated, "composite"))
            .samplers
            .iter()
            .filter_map(|sampler| sampler.class())
            .collect();
        assert_eq!(
            classes,
            [SamplerClass::RepeatNearest],
            "{property} and {uniform}"
        );
    }
}

#[test]
fn sampler_compatibility_modern_texture_members() {
    let fragment = "#version 330\nstruct Surface { vec4 texture; }; out vec4 color; void main() { Surface surface; surface.texture = vec4(1.0); color = surface.texture; }";
    let translated = pack(fragment, "", &[]).translate(&TranslateConfig::default());
    reflection(program(&translated, "composite"));
}

#[test]
fn sampler_compatibility_renamed_texture_uses_metadata() {
    let fragment = "#version 330\nuniform sampler2D texture; out vec4 color; void main() { color = texture2D(texture, vec2(0.5)); }";
    let metadata = r#"{"texture":{"blur":true,"clamp":true}}"#;
    let translated = pack(
        fragment,
        "texture.composite.texture = sample.png",
        &[("sample.png.mcmeta", metadata)],
    )
    .translate(&TranslateConfig::default());
    let classes: Vec<_> = reflection(program(&translated, "composite"))
        .samplers
        .iter()
        .filter_map(|sampler| sampler.class())
        .collect();
    assert_eq!(classes, [SamplerClass::ClampLinear]);
}
