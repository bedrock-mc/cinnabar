use std::collections::BTreeMap;

use super::*;

/// Parses test properties without predefined macros.
fn parse(text: &str) -> ShaderProperties {
    ShaderProperties::parse(text, &BTreeMap::new())
}

/// Parses test properties with the supplied macro definitions.
fn parse_with(text: &str, defines: &[(&str, &str)]) -> ShaderProperties {
    let defines = defines
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    ShaderProperties::parse(text, &defines)
}

/// Returns property keys in source order for assertions.
fn keys(properties: &ShaderProperties) -> Vec<&str> {
    properties
        .entries
        .iter()
        .map(|(key, _)| key.as_str())
        .collect()
}

/// Returns the source lines of property diagnostics in report order.
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
    assert_eq!(properties.get("after"), Some("1"));
    // An even number of trailing backslashes is an escaped backslash, not a continuation.
    assert_eq!(properties.get("path"), Some("dir\\"));
    assert_eq!(properties.get("next"), Some("2"));
}

#[test]
fn comment_lines_end_a_continuation_and_content_loses_hashes() {
    let properties = parse("list=one \\\n#two\nnext=a#b\n");
    assert_eq!(properties.get("list"), Some("one"));
    assert_eq!(properties.get("next"), Some("ab"));
}

#[test]
fn directives_apply_on_every_physical_line() {
    // An inactive continued entry cannot hide the `#endif` that closes its block.
    let properties = parse("#ifdef MISSING\nhidden=a \\\n#endif\nshown=1\n");
    assert_eq!(keys(&properties), ["shown"]);
    // A continuation backslash on a directive is part of its condition, as in Iris.
    let properties = parse_with("#if FLAG \\\n&& FLAG\n#endif\nafter=1\n", &[("FLAG", "")]);
    assert_eq!(keys(&properties), ["after"]);
    assert_eq!(diagnostic_lines(&properties), [1]);
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
    assert_eq!(properties.program_rules.len(), 3);
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
        "size.buffer.colortex4 = 64 64\nsize.buffer.colortex6 = 0.5 0.25\n\
         size.buffer.colortex7 = 64\nsize.buffer.colortex8 = wide 2\nsize.buffer.colortex9 = 0 4\n\
         size.buffer.colortex4 = 256 128\n",
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
    assert_eq!(properties.entries.len(), 6);
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
         texture.composite = textures/none.png\ntexture.deferred.colortex2 =\n\
         texture.composite.colortex5 = textures/dust2.png\n",
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
                source: TextureSource::File("textures/dust2.png".to_owned()),
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
fn custom_textures_bind_their_unit_uniform_in_their_stage_programs() {
    let properties = parse(
        "texture.noise = noise.png\ntexture.composite.colortex6.1 = lut.png\n\
         texture.gbuffers.gaux2 = water.png\ntexture.setup.colortex7 = seed.png\n",
    );
    let [noise, composite, gbuffers, setup] = properties.custom_textures.as_slice() else {
        panic!("{:?}", properties.custom_textures);
    };
    assert_eq!(noise.uniform(), "noisetex");
    assert_eq!(composite.uniform(), "colortex6");
    assert_eq!(gbuffers.uniform(), "gaux2");
    assert!(noise.applies_to(ProgramGroup::Shadow) && noise.applies_to(ProgramGroup::Final));
    assert!(composite.applies_to(ProgramGroup::Composite));
    assert!(composite.applies_to(ProgramGroup::Final));
    assert!(!composite.applies_to(ProgramGroup::Deferred));
    assert!(gbuffers.applies_to(ProgramGroup::Gbuffers));
    assert!(gbuffers.applies_to(ProgramGroup::Shadow));
    assert!(!gbuffers.applies_to(ProgramGroup::ShadowComposite));
    assert!(setup.applies_to(ProgramGroup::Setup));
    assert!(!setup.applies_to(ProgramGroup::Begin));
}

#[test]
fn program_rules_match_nested_dimension_folders() {
    let properties = parse("program.regions/world0/final.enabled=false\n");
    let rule = properties
        .program_rule("regions/world0", "final")
        .expect("nested dimension rule");
    assert_eq!(rule.dimension.as_deref(), Some("regions/world0"));
}

#[test]
fn carriage_return_line_endings_split_entries() {
    let properties = parse("alpha=1\rbeta=2\r\ngamma=3\r");
    assert_eq!(properties.get("alpha"), Some("1"));
    assert_eq!(properties.get("beta"), Some("2"));
    assert_eq!(properties.get("gamma"), Some("3"));
}

#[test]
fn blank_lines_end_continued_entries() {
    let properties = parse(
        "#define QUALITY 2\nscreen=A \\\n  B \\\n\nsliders=QUALITY\nvalue=1 \\\n\nafter=QUALITY\n",
    );
    assert_eq!(properties.get("sliders"), Some("QUALITY"));
    assert_eq!(properties.get("after"), Some("2"));
    assert_eq!(properties.get("value"), Some("1"));
}

#[test]
fn a_prefix_cut_off_by_the_work_limit_is_discarded() {
    use crate::source::MAX_PROPERTIES_PREPROCESSING_BYTES;

    let chunk = "m".repeat(64 * 1024);
    // Leaves less than one chunk of budget, so the last entry runs out after its prefix.
    let references = MAX_PROPERTIES_PREPROCESSING_BYTES / (chunk.len() + 1);
    let text = format!(
        "#define BIG {chunk}\nfiller={}\nprogram.final.enabled=false BIG\n",
        vec!["BIG"; references].join(" ")
    );
    let properties = parse(&text);
    assert!(properties.program_rule("world0", "final").is_none());
    assert!(
        properties
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("work limit"))
    );
}

#[test]
fn custom_images_relative_absolute_and_malformed() {
    let properties = parse(
        "image.halfImg = halfSampler rgba rgba16f half_float true true 0.5 0.5\n\
         image.flatImg = flatSampler red r32f float false false 32 8\n\
         image.cubeImg = cubeSampler red_integer r8ui unsigned_byte true false 16 16 16\n\
         image.badBool = s rgba rgba8 unsigned_byte sometimes false 4 4\n\
         image.badRelative = s rgba rgba8 unsigned_byte true true 1 1 1\n\
         image.tooShort = s rgba rgba8 unsigned_byte true false\n\
         image.zero = s rgba rgba8 unsigned_byte true false 0 4\n\
         image.lineImg = lineSampler red r32f float false false 256\n\
         image.cubeImg = cubeSampler red r32f float false false 8 8 8\n",
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
                    height: Some(8),
                    depth: None,
                },
            ),
            (
                image("cubeImg", "cubeSampler", "red", "r32f", "float"),
                false,
                ImageSize::Absolute {
                    width: 8,
                    height: Some(8),
                    depth: Some(8),
                },
            ),
            (
                image("lineImg", "lineSampler", "red", "r32f", "float"),
                false,
                ImageSize::Absolute {
                    width: 256,
                    height: None,
                    depth: None,
                },
            ),
        ]
    );
    assert_eq!(diagnostic_lines(&properties), [4, 5, 6, 7]);
}

#[test]
fn custom_uniforms_and_variables_keep_file_order() {
    let properties = parse(
        "variable.float.half = frameTime * 0.5\nuniform.vec2.pair = vec2(0.0, 1.0)\n\
         uniform.float.broken = 1 +\nuniform.float = 1\nuniform.vec2.pair = vec2(half, 1.0)\n",
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
            (CustomUniformKind::Uniform, "vec2", "pair", 5),
        ]
    );
    assert_eq!(properties.custom_uniforms[1].expr.identifiers(), ["half"]);
    assert_eq!(diagnostic_lines(&properties), [3, 4]);
}

#[test]
fn custom_uniform_expressions_see_macros_expanded_like_iris() {
    let properties = parse_with(
        "#define LOCAL_SCALE 2.0\n\
         uniform.float.swampy = if(biome_category == CAT_SWAMP, 1.0, 0.0)\n\
         variable.float.scaled = LOCAL_SCALE * frameTime\n",
        &[("CAT_SWAMP", "14")],
    );
    assert_eq!(
        properties.get("uniform.float.swampy"),
        Some("if(biome_category == 14, 1.0, 0.0)")
    );
    assert_eq!(
        properties.get("variable.float.scaled"),
        Some("2.0 * frameTime")
    );
    assert_eq!(
        properties.custom_uniforms[0].expr.identifiers(),
        ["biome_category"]
    );
    assert_eq!(
        properties.custom_uniforms[1].expr.identifiers(),
        ["frameTime"]
    );
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn a_directive_ends_a_continued_uniform_definition_as_in_iris() {
    let text = "#ifdef BRIGHT\nuniform.float.b = if( \\\n  x > 0, 1.0, \\\n  0.5) \\\n\
                #else\nuniform.float.b = 0.25\n#endif\nafter=1\n";
    let off = parse(text);
    assert_eq!(off.get("uniform.float.b"), Some("0.25"));
    assert_eq!(off.custom_uniforms.len(), 1);
    assert!(off.diagnostics.is_empty(), "{:?}", off.diagnostics);
    let on = parse_with(text, &[("BRIGHT", "")]);
    assert_eq!(on.get("uniform.float.b"), Some("if( x > 0, 1.0, 0.5)"));
    assert_eq!(on.custom_uniforms[0].expr.identifiers(), ["x"]);
    assert_eq!(on.get("after"), Some("1"));
    assert!(on.diagnostics.is_empty(), "{:?}", on.diagnostics);
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
fn a_malformed_last_declaration_leaves_no_rule_for_its_key() {
    let properties = parse(
        "image.lut = s red r32f float false false 16\nimage.lut = s red r32f float false false 0\n",
    );
    assert!(properties.images.is_empty());
    assert_eq!(diagnostic_lines(&properties), [2]);
}

#[test]
fn malformed_entries_stay_in_entries() {
    let properties = parse("image.cimage1 = sampler rgba rgba8 unsigned_byte maybe true 1.0 1.0\n");
    assert_eq!(
        properties.get("image.cimage1"),
        Some("sampler rgba rgba8 unsigned_byte maybe true 1.0 1.0")
    );
    assert!(properties.images.is_empty());
    assert_eq!(diagnostic_lines(&properties), [1]);
}

#[test]
fn conditions_use_c_integer_arithmetic_and_macro_aliases() {
    let defines = [
        ("MC_VERSION", "12111"),
        ("ALIAS", "QUALITY"),
        ("QUALITY", "2"),
    ];
    let properties = parse_with(
        "#if MC_VERSION / 10000 == 1\ndivided=1\n#endif\n\
         #if ALIAS >= 2\naliased=1\n#endif\n#if UNDEFINED\nhidden=1\n#endif\n",
        &defines,
    );
    assert_eq!(keys(&properties), ["divided", "aliased"]);
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn float_literals_in_conditions_read_as_their_integer_part_as_in_jcpp() {
    let defines = [
        ("MC_RENDER_QUALITY", "1.0"),
        ("STRENGTH", "0.75"),
        ("colortex4", ""),
    ];
    let properties = parse_with(
        "#if MC_RENDER_QUALITY >= 1\nquality=1\n#endif\n\
         #if STRENGTH > 0.5\nstrong=1\n#else\nweak=1\n#endif\n\
         #if 1.5f == 1 && 0x1F == 31 && defined colortex4\nliterals=1\n#endif\n",
        &defines,
    );
    assert_eq!(keys(&properties), ["quality", "weak", "literals"]);
    assert!(
        properties.diagnostics.is_empty(),
        "{:?}",
        properties.diagnostics
    );
}

#[test]
fn values_expand_object_like_macros() {
    let properties = parse_with(
        "#define LUT_SIZE SIZE\nimage.lut = s red r32f float false false LUT_SIZE\nflag=FLAG 0.5\n",
        &[("SIZE", "256"), ("FLAG", "")],
    );
    assert_eq!(
        properties.images[0].size,
        ImageSize::Absolute {
            width: 256,
            height: None,
            depth: None,
        }
    );
    assert_eq!(properties.get("flag"), Some("1 0.5"));
}

#[test]
fn a_macro_is_not_expanded_inside_its_own_expansion() {
    let properties = parse_with(
        "#define WIDTH WIDTH * 2\nwidth = WIDTH\nmutual = SELF + 1\n\
         #if SELF\nself_condition=1\n#endif\n#if SELF == 0 && defined SELF\nzero=1\n#endif\n",
        &[("SELF", "SELF + ALIAS"), ("ALIAS", "SELF")],
    );
    assert_eq!(properties.get("width"), Some("WIDTH * 2"));
    assert_eq!(properties.get("mutual"), Some("SELF + SELF + 1"));
    assert_eq!(keys(&properties), ["width", "mutual", "zero"]);
    assert!(
        properties.diagnostics.is_empty(),
        "{:?}",
        properties.diagnostics
    );
}

#[test]
fn layout_and_profile_values_keep_option_names() {
    let properties = parse_with(
        "screen=QUALITY\nscreen.details=QUALITY \\\n QUALITY\nsliders=QUALITY\nprofile.low=QUALITY:1\nother=QUALITY\n",
        &[("QUALITY", "2")],
    );
    assert_eq!(properties.get("screen"), Some("QUALITY"));
    assert_eq!(properties.get("screen.details"), Some("QUALITY QUALITY"));
    assert_eq!(properties.get("sliders"), Some("QUALITY"));
    assert_eq!(properties.get("profile.low"), Some("QUALITY:1"));
    assert_eq!(properties.get("other"), Some("2"));
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn expanded_properties_bytes_are_bounded_across_lines() {
    use crate::source::{MAX_EXPANDED_PROPERTIES_BYTES, MAX_TEXT_FILE_BYTES};

    // One identifier per reference keeps this about bytes, not the separate token budget.
    let chunk = "m".repeat(64 * 1024);
    let repetitions = MAX_EXPANDED_PROPERTIES_BYTES / chunk.len() / 2 + 1;
    let references = vec!["BIG"; repetitions].join(" ");
    let text = format!("#define BIG {chunk}\na={references}\nb={references}\nafter=ok\n");
    assert!(text.len() < MAX_TEXT_FILE_BYTES);
    let properties = parse(&text);
    assert_eq!(
        properties.get("a").unwrap().len(),
        (chunk.len() + 1) * repetitions - 1
    );
    assert!(properties.get("b").is_none());
    assert_eq!(properties.get("after"), Some("ok"));
    assert_eq!(diagnostic_lines(&properties), [3]);
}

#[test]
fn recursively_doubled_macros_stop_at_the_preprocessing_work_limit() {
    let mut text = format!("#define M0 {}\n", "m".repeat(64 * 1024));
    for level in 1..=24 {
        text += &format!("#define M{level} M{p} M{p}\n", p = level - 1);
    }
    text += "value=M24\n";
    let properties = parse(&text);
    assert!(properties.get("value").is_none());
    assert!(
        properties
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("work limit"))
    );
}

#[test]
fn doubled_macros_in_conditions_stop_at_the_preprocessing_work_limit() {
    let mut text = String::from("#define M0 0\n");
    for level in 1..=40 {
        text += &format!("#define M{level} M{p} + M{p}\n", p = level - 1);
    }
    text += "#if M40\nhidden=1\n#endif\n";
    let properties = parse(&text);
    assert_eq!(properties.get("hidden"), None);
    assert!(
        properties
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("work limit"))
    );
}

#[test]
fn an_entry_with_a_preprocessing_error_is_discarded_whole() {
    let properties = parse(
        "#define ID(x) x\nprogram.final.enabled=false ID(1,2)\nsplit=a \\\n  ID(1,2)\nafter=ok\n",
    );
    assert_eq!(properties.get("program.final.enabled"), None);
    assert_eq!(properties.get("split"), None);
    assert_eq!(properties.get("after"), Some("ok"));
    assert_eq!(diagnostic_lines(&properties), [2, 4]);
}

#[test]
fn a_nested_expansion_error_discards_the_invoking_entry() {
    let properties =
        parse("#define ID(x) x\n#define BAD ID(1,2)\nprogram.final.enabled=false BAD\nafter=ok\n");
    assert_eq!(properties.get("program.final.enabled"), None);
    assert_eq!(properties.get("after"), Some("ok"));
    assert_eq!(diagnostic_lines(&properties), [3]);
}

#[test]
fn deep_conditions_return_a_recoverable_diagnostic() {
    let nested = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
    let properties = parse(&format!("#if {nested}\nhidden=1\n#endif\nafter=ok\n"));
    assert_eq!(properties.get("hidden"), None);
    assert_eq!(properties.get("after"), Some("ok"));
    assert_eq!(diagnostic_lines(&properties), [1]);
}

#[test]
fn property_conditions_accept_ternaries() {
    let text = "#if MC_VERSION >= 12100 ? 1 : 0\npick=new\n#else\npick=old\n#endif\n";
    let properties = parse_with(text, &[("MC_VERSION", "12111")]);
    assert_eq!(properties.get("pick"), Some("new"));
    assert!(properties.diagnostics.is_empty());
    assert_eq!(
        parse_with(text, &[("MC_VERSION", "12000")]).get("pick"),
        Some("old")
    );
}

#[test]
fn comments_in_directives_are_whitespace_and_property_literals_stay_literal() {
    let properties = parse_with(
        "#define/* name */ LOCAL\n#ifndef/* absent */ MISSING\n#if defined/* gap\n */ LOCAL && defined ENV\nselected=ok\n#endif\n#endif\n#if 0\nhidden=1\n#elif/* choice */ defined LOCAL\nbranch=ok\n#endif\nliteral=\"QUALITY\" QUALITY /* QUALITY */\n",
        &[("ENV", ""), ("QUALITY", "2")],
    );
    assert_eq!(properties.get("selected"), Some("ok"));
    assert_eq!(properties.get("branch"), Some("ok"));
    assert_eq!(
        properties.get("literal"),
        Some("\"QUALITY\" 2 /* QUALITY */")
    );
    assert!(
        properties.diagnostics.is_empty(),
        "{:?}",
        properties.diagnostics
    );
}

#[test]
fn conditional_reference_forms_crlf_and_empty_values() {
    let properties = parse_with(
        "#define LOCAL\r\n#ifndef MISSING\r\n#if defined LOCAL && defined(ENV)\r\nempty=\r\nselected=ok\r\n#endif\r\n#endif\r\n#if 0\r\nhidden=1\r\n#elif defined ENV\r\nbranch=ok\r\n#else\r\nhidden=1\r\n#endif\r\n",
        &[("ENV", "")],
    );
    assert_eq!(keys(&properties), ["empty", "selected", "branch"]);
    assert_eq!(properties.get("empty"), Some(""));
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn hash_comments_do_not_open_directive_block_comments() {
    let properties = parse("# Notes mentioning /* without a matching end\nafter=ok\n");
    assert_eq!(properties.get("after"), Some("ok"));
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn macro_expansion_preserves_condition_token_boundaries() {
    let properties = parse("#define NEG -1\n#if -NEG > 0\nselected=yes\n#endif\n");
    assert_eq!(properties.get("selected"), Some("yes"));
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn content_block_comments_hide_conditional_directives() {
    let properties = parse("/*\n#if 0\n*/\nprogram.final.enabled=false\n");
    assert_eq!(properties.get("program.final.enabled"), Some("false"));
    assert!(properties.diagnostics.is_empty());
}

#[test]
fn flip_numeric_booleans_include_macro_expansions() {
    let properties = parse_with(
        "flip.composite.colortex0=0\nflip.composite.colortex1=1\nflip.composite.colortex2=OFF\nflip.composite.colortex3=ON\n",
        &[("OFF", "0"), ("ON", "1")],
    );
    assert!(
        properties.diagnostics.is_empty(),
        "{:?}",
        properties.diagnostics
    );
    assert_eq!(
        properties
            .flips
            .iter()
            .map(|rule| rule.flip)
            .collect::<Vec<_>>(),
        [false, true, false, true]
    );
}

#[test]
fn custom_expression_operator_groups_follow_iris() {
    for (expression, expected) in [
        ("true || false && false", 0.0),
        ("true || (false && false)", 1.0),
        ("1 == 2 < 3", 1.0),
        ("8 - 2 * 3", 2.0),
        ("if(true || false && false, 1, 2)", 2.0),
    ] {
        let properties = parse(&format!(
            "uniform.float.result={expression}\nvariable.float.result2={expression}\n"
        ));
        assert!(
            properties.diagnostics.is_empty(),
            "{:?}",
            properties.diagnostics
        );
        for uniform in &properties.custom_uniforms {
            assert_eq!(
                uniform.expr.eval(&|_| None).unwrap(),
                expected,
                "{expression}"
            );
        }
    }
}

#[test]
fn custom_comparison_aliases_match_ascii_operators() {
    for expression in ["1 ≠ 2", "1 ≤ 2", "2 ≥ 1"] {
        let properties = parse(&format!("uniform.bool.result={expression}\n"));
        assert!(
            properties.diagnostics.is_empty(),
            "{:?}",
            properties.diagnostics
        );
        assert_eq!(
            properties.custom_uniforms[0].expr.eval(&|_| None).unwrap(),
            1.0
        );
    }
}
