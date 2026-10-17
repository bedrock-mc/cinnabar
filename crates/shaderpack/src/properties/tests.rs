use std::collections::BTreeMap;

use super::*;

fn parse(text: &str) -> ShaderProperties {
    ShaderProperties::parse(text, &BTreeMap::new())
}

fn parse_with(text: &str, defines: &[(&str, &str)]) -> ShaderProperties {
    let defines = defines
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    ShaderProperties::parse(text, &defines)
}

fn keys(properties: &ShaderProperties) -> Vec<&str> {
    properties
        .entries
        .iter()
        .map(|(key, _)| key.as_str())
        .collect()
}

fn diagnostic_lines(properties: &ShaderProperties) -> Vec<u32> {
    properties
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.line)
        .collect()
}

#[test]
fn java_properties_separators_comments_and_escapes() {
    let properties = parse(
        "alpha=1\n  beta : two words  \ngamma 3\ndelta=\n! bang comment\n# hash comment\n\
         \n spaced.key   =   x = y\nesc\\=key=a\\tb\\u0041\n",
    );
    assert_eq!(
        properties.entries,
        [
            ("alpha".to_owned(), "1".to_owned()),
            ("beta".to_owned(), "two words".to_owned()),
            ("gamma".to_owned(), "3".to_owned()),
            ("delta".to_owned(), String::new()),
            ("spaced.key".to_owned(), "x = y".to_owned()),
            ("esc=key".to_owned(), "a\tbA".to_owned()),
        ]
    );
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn duplicates_are_kept_and_get_returns_the_last() {
    let properties = parse("mode=first\nother=1\nmode = second \n");
    assert_eq!(keys(&properties), ["mode", "other", "mode"]);
    assert_eq!(properties.get("mode"), Some("second"));
    assert_eq!(properties.get("missing"), None);
}

#[test]
fn continuation_lines_join_without_leading_whitespace() {
    let properties = parse(
        "uniform.float.total = alpha + \\\n        beta + \\\n   gamma\nafter=1\n\
         path=dir\\\\\nnext=2\n",
    );
    assert_eq!(
        properties.get("uniform.float.total"),
        Some("alpha + beta + gamma")
    );
    assert_eq!(properties.custom_uniforms[0].line, 1);
    assert_eq!(properties.get("after"), Some("1"));
    // An even number of trailing backslashes is an escaped backslash, not a continuation.
    assert_eq!(properties.get("path"), Some("dir\\"));
    assert_eq!(properties.get("next"), Some("2"));
}

#[test]
fn continuation_lines_starting_with_hash_are_content() {
    let properties = parse("list=one \\\n#two\n");
    assert_eq!(properties.get("list"), Some("one #two"));
}

#[test]
fn if_elif_else_select_one_branch() {
    let text = "#if LEVEL == 1\npick=one\n#elif LEVEL == 2\npick=two\n#else\npick=other\n#endif\n";
    assert_eq!(parse_with(text, &[("LEVEL", "1")]).get("pick"), Some("one"));
    assert_eq!(parse_with(text, &[("LEVEL", "2")]).get("pick"), Some("two"));
    assert_eq!(
        parse_with(text, &[("LEVEL", "7")]).get("pick"),
        Some("other")
    );
    assert_eq!(parse(text).get("pick"), Some("other"));
    let properties = parse_with(text, &[("LEVEL", "2")]);
    assert_eq!(keys(&properties), ["pick"]);
}

#[test]
fn ifdef_ifndef_and_defined_use_the_define_table() {
    let text = "#ifdef FEATURE\na=1\n#endif\n#ifndef FEATURE\nb=1\n#endif\n\
                # if defined(FEATURE) && !defined MISSING\nc=1\n#endif\n\
                \t#if FEATURE\nd=1\n#endif\n#if WORD\ne=1\n#endif\n#if HEX == 16\nf=1\n#endif\n";
    let properties = parse_with(text, &[("FEATURE", ""), ("WORD", "abc"), ("HEX", "0x10")]);
    assert_eq!(keys(&properties), ["a", "c", "d", "f"]);
    assert!(properties.diagnostics.is_empty());
    let properties = parse(text);
    assert_eq!(keys(&properties), ["b"]);
}

#[test]
fn define_and_undef_affect_later_conditionals_only_when_active() {
    let text = "#define LOCAL 4\n#if LOCAL > 3\na=1\n#endif\n#undef LOCAL\n#ifdef LOCAL\nb=1\n#endif\n\
                #if 0\n#define HIDDEN\n#endif\n#ifdef HIDDEN\nc=1\n#endif\n";
    assert_eq!(keys(&parse(text)), ["a"]);
}

#[test]
fn nested_blocks_inside_inactive_regions_stay_inactive() {
    let text = "#if 0\n#if 1\na=1\n#else\nb=1\n#endif\nc=1\n#elif 1\nd=1\n#endif\ne=1\n";
    assert_eq!(keys(&parse(text)), ["d", "e"]);
}

#[test]
fn commented_keys_and_prose_are_plain_comments() {
    let properties = parse("#uniform.float.hidden=1\n#ifdefined stuff\n# Notes about it\nreal=1\n");
    assert_eq!(keys(&properties), ["real"]);
    assert!(properties.custom_uniforms.is_empty());
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn unbalanced_directives_are_diagnosed_and_parsing_continues() {
    let properties =
        parse("#endif\na=1\n#else\nb=1\n#if (\nc=1\n#endif\n#ifdef\nd=1\n#endif\n#if 1\ne=1\n");
    assert_eq!(keys(&properties), ["a", "b", "e"]);
    assert_eq!(diagnostic_lines(&properties), [1, 3, 5, 8, 11]);
}

#[test]
fn duplicate_else_and_elif_after_else_are_diagnosed() {
    let properties = parse("#if 0\n#else\na=1\n#else\nb=1\n#elif 1\nc=1\n#endif\n");
    assert_eq!(keys(&properties), ["a"]);
    assert_eq!(diagnostic_lines(&properties), [4, 6]);
}

#[test]
fn program_rules_prefer_dimension_and_last_occurrence() {
    let properties = parse(
        "program.composite1.enabled = BLOOM\nprogram.world0/composite1.enabled = BLOOM && !FAST\n\
         program.world0/composite1.enabled = TAA\nprogram.shadow.enabled=SHADOW\n\
         program.world0/shadow.enabled = (\n",
    );
    assert_eq!(properties.program_rules.len(), 4);
    let rule = properties
        .program_rule("world0", "composite1")
        .expect("rule");
    assert_eq!(rule.dimension.as_deref(), Some("world0"));
    assert_eq!(rule.expr, Expr::Ident("TAA".to_owned()));
    assert_eq!(rule.line, 3);
    let rule = properties
        .program_rule("world-1", "composite1")
        .expect("rule");
    assert_eq!(rule.dimension, None);
    assert_eq!(rule.line, 1);
    assert_eq!(
        properties
            .program_rule("world0", "shadow")
            .map(|rule| rule.line),
        Some(4)
    );
    assert!(properties.program_rule("world0", "deferred").is_none());
    assert_eq!(diagnostic_lines(&properties), [5]);
}

#[test]
fn buffer_sizes_absolute_relative_and_malformed() {
    let properties = parse(
        "size.buffer.colortex4 = 256 128\nsize.buffer.colortex6 = 0.5 0.25\n\
         size.buffer.colortex7 = 64\nsize.buffer.colortex8 = wide 2\nsize.buffer.colortex9 = 0 4\n",
    );
    assert_eq!(
        properties.buffer_sizes,
        [
            BufferSize {
                buffer: "colortex4".to_owned(),
                width: SizeValue::Absolute(256),
                height: SizeValue::Absolute(128),
            },
            BufferSize {
                buffer: "colortex6".to_owned(),
                width: SizeValue::Relative(0.5),
                height: SizeValue::Relative(0.25),
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [3, 4, 5]);
    assert_eq!(properties.entries.len(), 5);
}

#[test]
fn blend_rules_off_factors_per_buffer_and_malformed() {
    let properties = parse(
        "blend.gbuffers_water = off\n\
         blend.gbuffers_water.colortex2 = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO\n\
         blend.gbuffers_hand = SRC_ALPHA ONE PLUS ONE\nblend.gbuffers_basic = ONE ONE ONE\n",
    );
    assert_eq!(
        properties.blends,
        [
            BlendRule {
                program: "gbuffers_water".to_owned(),
                buffer: None,
                mode: BlendMode::Off,
            },
            BlendRule {
                program: "gbuffers_water".to_owned(),
                buffer: Some("colortex2".to_owned()),
                mode: BlendMode::Factors {
                    src_color: BlendFactor::SrcAlpha,
                    dst_color: BlendFactor::OneMinusSrcAlpha,
                    src_alpha: BlendFactor::One,
                    dst_alpha: BlendFactor::Zero,
                },
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [3, 4]);
}

#[test]
fn alpha_tests_off_function_and_malformed() {
    let properties = parse(
        "alphaTest.gbuffers_terrain = off\nalphaTest.gbuffers_hand = GEQUAL 0.25\n\
         alphaTest.gbuffers_block = ABOVE 0.1\nalphaTest.gbuffers_line = LESS half\n\
         alphaTest.gbuffers_weather = GREATER\n",
    );
    assert_eq!(
        properties.alpha_tests,
        [
            AlphaTestRule {
                program: "gbuffers_terrain".to_owned(),
                test: None,
            },
            AlphaTestRule {
                program: "gbuffers_hand".to_owned(),
                test: Some(AlphaTest {
                    function: CompareFunction::GreaterEqual,
                    reference: 0.25,
                }),
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [3, 4, 5]);
}

#[test]
fn flip_rules_and_malformed() {
    let properties = parse(
        "flip.composite2.colortex3 = true\nflip.deferred.colortex1=false\n\
         flip.composite3.colortex4 = maybe\nflip.composite4 = true\n",
    );
    assert_eq!(
        properties.flips,
        [
            FlipRule {
                program: "composite2".to_owned(),
                buffer: "colortex3".to_owned(),
                flip: true,
            },
            FlipRule {
                program: "deferred".to_owned(),
                buffer: "colortex1".to_owned(),
                flip: false,
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [3, 4]);
}

#[test]
fn scale_rules_with_and_without_offset() {
    let properties = parse(
        "scale.composite5 = 0.5\nscale.composite6 = 0.25 0.5 0.75\nscale.composite7 = 0.5 0.1\n\
         scale.composite8 = big\nscale.composite9 = 0\n",
    );
    assert_eq!(
        properties.scales,
        [
            ScaleRule {
                program: "composite5".to_owned(),
                scale: 0.5,
                offset: [0.0, 0.0],
            },
            ScaleRule {
                program: "composite6".to_owned(),
                scale: 0.25,
                offset: [0.5, 0.75],
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [3, 4, 5]);
}

#[test]
fn custom_textures_file_resource_raw_noise_and_malformed() {
    let properties = parse(
        "texture.noise = /textures/grain.png\ntexture.composite.colortex5 = textures/dust.png\n\
         texture.gbuffers.gaux1 = example:textures/sky/stars.png\n\
         texture.deferred.colortex9.1 = data/lut.bin TEXTURE_3D RGBA8 16 16 16 RGBA UNSIGNED_BYTE\n\
         texture.composite = textures/none.png\ntexture.deferred.colortex2 =\n",
    );
    assert_eq!(
        properties.custom_textures,
        [
            CustomTexture {
                stage: "noise".to_owned(),
                sampler: "noisetex".to_owned(),
                source: TextureSource::File("textures/grain.png".to_owned()),
            },
            CustomTexture {
                stage: "composite".to_owned(),
                sampler: "colortex5".to_owned(),
                source: TextureSource::File("textures/dust.png".to_owned()),
            },
            CustomTexture {
                stage: "gbuffers".to_owned(),
                sampler: "gaux1".to_owned(),
                source: TextureSource::Resource("example:textures/sky/stars.png".to_owned()),
            },
            CustomTexture {
                stage: "deferred".to_owned(),
                sampler: "colortex9.1".to_owned(),
                source: TextureSource::Raw {
                    path: "data/lut.bin".to_owned(),
                    spec: [
                        "TEXTURE_3D",
                        "RGBA8",
                        "16",
                        "16",
                        "16",
                        "RGBA",
                        "UNSIGNED_BYTE"
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                },
            },
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [5, 6]);
}

#[test]
fn custom_images_relative_absolute_and_malformed() {
    let properties = parse(
        "image.halfImg = halfSampler rgba rgba16f half_float true true 0.5 0.5\n\
         image.flatImg = flatSampler red r32f float false false 32 8\n\
         image.cubeImg = cubeSampler red_integer r8ui unsigned_byte true false 16 16 16\n\
         image.badBool = s rgba rgba8 unsigned_byte sometimes false 4 4\n\
         image.badRelative = s rgba rgba8 unsigned_byte true true 1 1 1\n\
         image.tooShort = s rgba rgba8 unsigned_byte true false 4\n\
         image.zero = s rgba rgba8 unsigned_byte true false 0 4\n",
    );
    let image = |name: &str, sampler: &str, format: &str, internal: &str, pixel: &str| {
        (
            name.to_owned(),
            sampler.to_owned(),
            format.to_owned(),
            internal.to_owned(),
            pixel.to_owned(),
        )
    };
    let summary: Vec<_> = properties
        .images
        .iter()
        .map(|img| {
            (
                (
                    img.name.clone(),
                    img.sampler.clone(),
                    img.format.clone(),
                    img.internal_format.clone(),
                    img.pixel_type.clone(),
                ),
                img.clear,
                img.size,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                image("halfImg", "halfSampler", "rgba", "rgba16f", "half_float"),
                true,
                ImageSize::Relative {
                    width: 0.5,
                    height: 0.5,
                },
            ),
            (
                image("flatImg", "flatSampler", "red", "r32f", "float"),
                false,
                ImageSize::Absolute {
                    width: 32,
                    height: 8,
                    depth: None,
                },
            ),
            (
                image(
                    "cubeImg",
                    "cubeSampler",
                    "red_integer",
                    "r8ui",
                    "unsigned_byte"
                ),
                true,
                ImageSize::Absolute {
                    width: 16,
                    height: 16,
                    depth: Some(16),
                },
            ),
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [4, 5, 6, 7]);
}

#[test]
fn custom_uniforms_and_variables_keep_file_order() {
    let properties = parse(
        "variable.float.half = frameTime * 0.5\nuniform.vec2.pair = vec2(half, 1.0)\n\
         uniform.float.broken = 1 +\nuniform.float = 1\n",
    );
    let summary: Vec<_> = properties
        .custom_uniforms
        .iter()
        .map(|uniform| {
            (
                uniform.kind,
                uniform.ty.as_str(),
                uniform.name.as_str(),
                uniform.line,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (CustomUniformKind::Variable, "float", "half", 1),
            (CustomUniformKind::Uniform, "vec2", "pair", 2),
        ]
    );
    assert_eq!(properties.custom_uniforms[1].expr.identifiers(), ["half"]);
    assert_eq!(diagnostic_lines(&properties), [3, 4]);
}

#[test]
fn unknown_keys_stay_only_in_entries() {
    let properties = parse("sun=true\nshadow.culling=reversed\nscreen=[MAIN] SPEED\n");
    assert_eq!(keys(&properties), ["sun", "shadow.culling", "screen"]);
    assert_eq!(properties.get("screen"), Some("[MAIN] SPEED"));
    assert!(properties.program_rules.is_empty());
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn malformed_entries_stay_in_entries() {
    let properties = parse("blend.gbuffers_water = SOMETIMES\n");
    assert_eq!(properties.get("blend.gbuffers_water"), Some("SOMETIMES"));
    assert!(properties.blends.is_empty());
    assert_eq!(diagnostic_lines(&properties), [1]);
}
