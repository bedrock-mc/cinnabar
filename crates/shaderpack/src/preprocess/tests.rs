use super::*;

/// Expands and preprocesses in-memory files with the supplied macros.
fn run(files: &[(&str, &str)], entry: &str, defines: &[(&str, &str)]) -> Preprocessed {
    let source = PackSource::from_files(files.iter().copied());
    let expanded = expand(&source, entry, &[], &OptionValues::new()).expect("expands");
    let defines: Vec<(String, String)> = defines
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    preprocess(expanded, &defines).expect("preprocesses")
}

#[test]
fn frontend_version_condition() {
    for (version, text) in [(450, "#version 450\n"), (110, "")] {
        let src = format!(
            "{text}#if __VERSION__ == {version}\nint selected;\n#else\n#error wrong version\n#endif"
        );
        assert!(
            run(&[("a.fsh", &src)], "a.fsh", &[])
                .tokens
                .iter()
                .any(|t| t.ident() == Some("selected"))
        );
    }
}

#[test]
fn frontend_marker_scope() {
    for control in ["#endif", "#else\n#endif", "#elif 1\n#endif"] {
        let src = format!("#if 0\n#define X 1 /* RENDERTARGETS:3 */\n{control}\nvoid main() {{}}");
        assert_eq!(run(&[("a.fsh", &src)], "a.fsh", &[]).render_targets, None);
    }
    let pp = run(
        &[("a.fsh", "#if 1\n#define X 1 /* RENDERTARGETS:4 */\n#endif")],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![4]));
}

#[test]
fn frontend_multiline_macro_comment() {
    let pp = run(
        &[(
            "a.fsh",
            "#define X /* first\nsecond */ 1\n#if X != 1\n#error wrong macro\n#endif\nint visible;",
        )],
        "a.fsh",
        &[],
    );
    assert!(pp.tokens.iter().any(|t| t.ident() == Some("visible")));
    assert!(!pp.tokens.iter().any(|t| t.text() == "1"));
}

#[test]
fn frontend_splices_before_line_comments() {
    let pp = run(
        &[("a.fsh", "// ignored \\\n#error hidden\nint visible;")],
        "a.fsh",
        &[],
    );
    assert!(pp.tokens.iter().any(|t| t.ident() == Some("visible")));
}

#[test]
fn frontend_original_line_numbers_survive_metadata_and_comments() {
    let pp = run(
        &[(
            "a.fsh",
            "#define X 1 /* DRAWBUFFERS:0 */\nint a = __LINE__;\n#define Y /* one\ntwo */ 2\nint b = __LINE__;\n#line 80\nint c = __LINE__;",
        )],
        "a.fsh",
        &[],
    );
    let values: Vec<_> = pp
        .tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokKind::Int(n) => Some(n.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(values, ["2", "5", "81"]);
}

#[test]
fn frontend_empty_drawbuffers() {
    let pp = run(
        &[("a.fsh", "/* DRAWBUFFERS:01 */\n/* DRAWBUFFERS: */")],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![]));
}

#[test]
fn frontend_const_whitespace() {
    let pp = run(
        &[("a.fsh", "const\tint shadowMapResolution = 2048;")],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.consts.len(), 1);
}

#[test]
fn include_filenames_end_before_trailing_comments() {
    for include in [
        "#include \"/lib/common.glsl\" // shared",
        "#include \"/lib/common.glsl\" /* shared */",
        "/* leading */ #include \"/lib/common.glsl\"",
        "/* previous\n*/ #include \"/lib/common.glsl\"",
    ] {
        let pp = run(
            &[("a.fsh", include), ("lib/common.glsl", "float shared;")],
            "a.fsh",
            &[],
        );
        let token = pp
            .tokens
            .iter()
            .find(|t| t.ident() == Some("shared"))
            .unwrap();
        assert_eq!(
            pp.map.locate(token.line).unwrap().to_string(),
            "lib/common.glsl:1"
        );
    }
}

#[test]
fn includes_inside_comments_are_ignored() {
    let pp = run(
        &[("a.fsh", "/*\n#include \"missing.glsl\"\n*/\nfloat present;")],
        "a.fsh",
        &[],
    );
    assert!(pp.tokens.iter().any(|t| t.ident() == Some("present")));
}

#[test]
fn deep_includes_expand_without_call_stack_recursion() {
    let mut files = Vec::new();
    for i in 0..8192 {
        files.push((
            format!("lib/{i}.glsl"),
            if i == 8191 {
                "float deepest;".to_string()
            } else {
                format!("#include \"{}.glsl\"", i + 1)
            },
        ));
    }
    let source = PackSource::from_files(files);
    let expanded = expand(&source, "lib/0.glsl", &[], &OptionValues::new()).unwrap();
    let pp = preprocess(expanded, &[]).unwrap();
    let token = pp
        .tokens
        .iter()
        .find(|t| t.ident() == Some("deepest"))
        .unwrap();
    assert_eq!(
        pp.map.locate(token.line).unwrap().to_string(),
        "lib/8191.glsl:1"
    );
}

#[test]
fn includes_resolve_absolute_and_relative_paths() {
    assert_eq!(
        resolve_include("world0/a.fsh", " \"/lib/x.glsl\"").unwrap(),
        "lib/x.glsl"
    );
    assert_eq!(
        resolve_include("world0/a.fsh", " \"../lib/x.glsl\"").unwrap(),
        "lib/x.glsl"
    );
    assert_eq!(
        resolve_include("program/a.glsl", " \"b.glsl\"").unwrap(),
        "program/b.glsl"
    );
}

#[test]
fn only_directives_in_active_code_survive() {
    let src = "#version 450\n\
        #ifdef A\n/* RENDERTARGETS: 3 */\n#else\n/* RENDERTARGETS: 1,2 */\n#endif\n\
        /*\nconst int colortex1Format = RGBA16F;\n*/\n\
        #if 0\n/*\nconst int colortex2Format = R8;\n*/\n#endif\n\
        void main() {}\n";
    let pp = run(&[("a.fsh", src)], "a.fsh", &[]);
    assert_eq!(pp.render_targets, Some(vec![1, 2]));
    assert_eq!(pp.consts.len(), 1);
    assert_eq!(pp.consts[0].name, "colortex1Format");
    assert_eq!(pp.consts[0].value, "RGBA16F");
    let pp = run(&[("a.fsh", src)], "a.fsh", &[("A", "")]);
    assert_eq!(pp.render_targets, Some(vec![3]));
}

#[test]
fn token_lines_map_back_through_includes() {
    let pp = run(
        &[
            (
                "world0/a.fsh",
                "#version 450\n#include \"/lib/b.glsl\"\nfloat y;\n",
            ),
            ("lib/b.glsl", "\nfloat x;\n"),
        ],
        "world0/a.fsh",
        &[],
    );
    let x = pp
        .tokens
        .iter()
        .find(|t| t.ident() == Some("x"))
        .expect("x");
    let y = pp
        .tokens
        .iter()
        .find(|t| t.ident() == Some("y"))
        .expect("y");
    let at = |t: &Tok| pp.map.locate(t.line).expect("mapped");
    assert_eq!(at(x).to_string(), "lib/b.glsl:2");
    assert_eq!(at(y).to_string(), "world0/a.fsh:3");
}

#[test]
fn missing_include_reports_the_including_line() {
    let source = PackSource::from_files([("a.fsh", "\n#include \"/nope.glsl\"\n")]);
    let err = expand(&source, "a.fsh", &[], &OptionValues::new())
        .err()
        .expect("fails");
    assert_eq!(err.phase, Phase::Include);
    assert_eq!(err.location.expect("located").to_string(), "a.fsh:2");
}
#[test]
fn frontend_include_expansion_is_bounded_even_for_empty_leaves() {
    for leaf in ["", "float leaf;\n"] {
        let files: Vec<_> = (0..=12)
            .map(|level| {
                (
                    format!("{level}.glsl"),
                    if level == 12 {
                        leaf.to_string()
                    } else {
                        format!(
                            "#include \"{}.glsl\"\n#include \"{}.glsl\"\n",
                            level + 1,
                            level + 1
                        )
                    },
                )
            })
            .collect();
        let source = PackSource::from_files(files);
        let error = expand_limited(&source, "0.glsl", &[], &OptionValues::new(), 128)
            .err()
            .expect("expansion budget");
        assert_eq!(error.phase, Phase::Include);
        assert!(error.message.contains("expanded GLSL"));
        assert!(error.location.is_some());
    }
}

#[test]
fn frontend_macro_work_includes_condition_evaluation() {
    let mut definitions = "#define A0 1\n".to_string();
    for level in 1..=8 {
        definitions.push_str(&format!(
            "#define A{level} (A{} + A{})\n",
            level - 1,
            level - 1
        ));
    }
    for tail in ["A8", "#if A8\nint selected;\n#endif"] {
        let source = PackSource::from_files([("a.fsh", format!("{definitions}{tail}"))]);
        let expanded = expand(&source, "a.fsh", &[], &OptionValues::new()).unwrap();
        let error = preprocess_limited(expanded, &[], 128, usize::MAX)
            .err()
            .expect("work budget");
        assert_eq!(error.phase, Phase::Preprocess);
        assert!(error.message.contains("work limit"));
        assert!(error.location.is_some());
    }
}

#[test]
fn frontend_macro_output_has_a_separate_storage_budget() {
    let source =
        PackSource::from_files([("a.fsh", "#define NAME repeated_identifier\nNAME NAME NAME")]);
    let expanded = expand(&source, "a.fsh", &[], &OptionValues::new()).unwrap();
    let error = preprocess_limited(expanded, &[], u64::MAX, 1)
        .err()
        .expect("retained storage budget");
    assert_eq!(error.phase, Phase::Preprocess);
    assert!(error.message.contains("storage limit"));
    assert_eq!(error.location.unwrap().to_string(), "a.fsh:2");
}

#[test]
fn frontend_newlines_expand_includes_and_keep_source_locations() {
    for newline in ["\r", "\r\n", "\n"] {
        let main = [
            "#version 450",
            "#include \"/lib/value.glsl\"",
            "int after = __LINE__;",
        ]
        .join(newline);
        let included = [r"// ignored \", "#error hidden", "int included = __LINE__;"].join(newline);
        let pp = run(
            &[("a.fsh", &main), ("lib/value.glsl", &included)],
            "a.fsh",
            &[],
        );
        let values: Vec<_> = pp
            .tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Int(value) => Some(value.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(values, ["3", "3"]);
        for (name, expected) in [("included", "lib/value.glsl:3"), ("after", "a.fsh:3")] {
            let token = pp
                .tokens
                .iter()
                .find(|token| token.ident() == Some(name))
                .unwrap();
            assert_eq!(pp.map.locate(token.line).unwrap().to_string(), expected);
        }
    }
}

#[test]
fn frontend_boolean_overrides_preserve_block_metadata() {
    for prefix in ["#define", "//#define"] {
        let text = format!(
            "{prefix} FEATURE /* DRAWBUFFERS:01 */ /* RENDERTARGETS: 2 */ // note\n#ifdef FEATURE\n#endif"
        );
        let options = crate::options::discover_options([("a.fsh", text.as_str())]);
        assert_eq!(options.len(), 1);
        let source = PackSource::from_files([("a.fsh", text.as_str())]);
        for value in ["true", "false"] {
            let values = OptionValues::from([("FEATURE".into(), value.into())]);
            let expanded = expand(&source, "a.fsh", &options, &values).unwrap();
            let pp = preprocess(expanded, &[]).unwrap();
            assert_eq!(pp.render_targets, (value == "true").then(|| vec![2]));
        }
    }
}

#[test]
fn frontend_include_budget_accepts_the_exact_boundary() {
    let text = "float value;\n";
    let source = PackSource::from_files([("a.fsh", text)]);
    let expanded = expand_limited(&source, "a.fsh", &[], &OptionValues::new(), text.len()).unwrap();
    assert_eq!(expanded.text, text);
    assert!(expand_limited(&source, "a.fsh", &[], &OptionValues::new(), text.len() - 1).is_err());
}

#[test]
fn frontend_metadata_copies_spend_the_storage_budget() {
    let expanded = Expanded {
        text: format!("/* const float value = {}; */", "x".repeat(512)),
        map: LineMap::default(),
    };
    let error = preprocess_limited(expanded, &[], u64::MAX, 256)
        .err()
        .expect("metadata storage budget");
    assert!(error.message.contains("storage limit"));
}

#[test]
fn frontend_macro_setup_exhaustion_is_recoverable() {
    let source = PackSource::from_files([("a.fsh", "int value;")]);
    let expanded = expand(&source, "a.fsh", &[], &OptionValues::new()).unwrap();
    let error = preprocess_limited(expanded, &[], 0, usize::MAX)
        .err()
        .expect("setup work budget");
    assert!(error.message.contains("work limit"));
}

#[test]
fn frontend_inactive_errors_accept_arbitrary_payloads() {
    for error in [
        "# error \"x\"",
        "#error It's x",
        "# error \"unclosed",
        "#error `@$",
    ] {
        let source = format!("#if 0\n{error}\n#endif\nint visible;");
        let pp = run(&[("a.fsh", &source)], "a.fsh", &[]);
        assert!(
            pp.tokens
                .iter()
                .any(|token| token.ident() == Some("visible"))
        );
    }
}

#[test]
fn frontend_comment_consts_follow_preprocessed_source_order() {
    let pp = run(
        &[(
            "a.fsh",
            "/* const int colortex0Format = RGBA16F; */\n#if 0\n/* const int colortex1Format = R8; */\n#endif\n// const int colortex2Format = RGBA8;\nconst int shadowMapResolution = 1024;",
        )],
        "a.fsh",
        &[],
    );
    assert_eq!(
        pp.consts
            .iter()
            .map(|directive| (directive.name.as_str(), directive.value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("colortex0Format", "RGBA16F"),
            ("colortex2Format", "RGBA8"),
            ("shadowMapResolution", "1024")
        ]
    );
}

#[test]
fn frontend_include_line_comments_survive_expansion() {
    let pp = run(
        &[
            (
                "a.fsh",
                "#include \"/lib/common.glsl\" /* RENDERTARGETS: 2 */",
            ),
            ("lib/common.glsl", "int included;"),
        ],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![2]));
    assert!(
        pp.tokens
            .iter()
            .any(|token| token.ident() == Some("included"))
    );
}

#[test]
fn frontend_include_leading_comments_precede_included_metadata() {
    let pp = run(
        &[
            ("a.fsh", "/* RENDERTARGETS: 1 */ #include \"lib.glsl\""),
            ("lib.glsl", "/* RENDERTARGETS: 2 */"),
        ],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![2]));
}

#[test]
fn frontend_include_trailing_comments_follow_included_metadata() {
    let pp = run(
        &[
            ("a.fsh", "#include \"lib.glsl\" /* RENDERTARGETS: 1 */"),
            ("lib.glsl", "/* RENDERTARGETS: 2 */"),
        ],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![1]));
}

#[test]
fn frontend_lfcr_continuations_keep_source_lines() {
    let source = [
        "#define VALUE 1 + \\",
        "2",
        "int sum = VALUE;",
        "int line = __LINE__;",
    ]
    .join("\n\r");
    let pp = run(&[("a.fsh", &source)], "a.fsh", &[]);
    let mut emitter = crate::token::Emitter::default();
    emitter.tokens(&pp.tokens);
    assert_eq!(emitter.finish().0, "int sum = 1 + 2;\nint line = 4;\n");
    let line = pp
        .tokens
        .iter()
        .find(|token| token.ident() == Some("line"))
        .unwrap();
    assert_eq!(pp.map.locate(line.line).unwrap().to_string(), "a.fsh:4");
}

#[test]
fn frontend_float_literals_keep_expanded_spelling() {
    let pp = run(
        &[(
            "a.fsh",
            "#define PRECISE 0.1234567890123456789\nfloat values[] = float[](PRECISE, 1e999, 1e-999, 4.1e+10f, .01e12F, 1.000000);",
        )],
        "a.fsh",
        &[],
    );
    let literals: Vec<_> = pp
        .tokens
        .iter()
        .filter_map(|token| match &token.kind {
            TokKind::Float(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        literals,
        [
            "0.1234567890123456789",
            "1e999",
            "1e-999",
            "4.1e+10f",
            ".01e12F",
            "1.000000"
        ]
    );
}

#[test]
fn frontend_header_line_macro_preserves_spelling() {
    let pp = run(
        &[("a.fsh", "\n\nint line = __LINE__;\nuint literal = 0x2Au;")],
        "a.fsh",
        &[],
    );
    let mut emitter = crate::token::Emitter::default();
    emitter.tokens(&pp.tokens);
    assert_eq!(emitter.finish().0, "int line = 3;\nuint literal = 0x2Au;\n");
}

#[test]
fn frontend_header_directive_comments_keep_metadata_lines() {
    for header in [
        "#version 450 // header",
        "#extension GL_ARB_shader_texture_lod : enable // x",
        "#pragma optimize(on) // x",
    ] {
        for tail in [
            "const int shadowMapResolution = 2048;",
            "/* RENDERTARGETS: 2 */",
        ] {
            let source = format!("{header}\n{tail}");
            let pp = run(&[("a.fsh", &source)], "a.fsh", &[]);
            if tail.starts_with("const") {
                assert_eq!(
                    pp.consts,
                    [ConstDirective {
                        ty: "int".into(),
                        name: "shadowMapResolution".into(),
                        value: "2048".into(),
                    }],
                    "{header}"
                );
            } else {
                assert_eq!(pp.render_targets, Some(vec![2]), "{header}");
            }
        }
    }
}

#[test]
fn frontend_header_combines_options_includes_and_conditionals() {
    let header = "#version 450 // shader header\n\
        /* DRAWBUFFERS:01 */\n\
        #extension GL_ARB_shader_texture_lod : enable // texture lookup\n\
        const int shadowMapResolution = 2048; // [1024 2048]\n\
        #extension GL_ARB_draw_buffers : enable\n\
        /* RENDERTARGETS: 3 */ #include \"lib.glsl\" /* RENDERTARGETS: 2 */\n\
        #ifdef HIGH_QUALITY\n\
        #define SHADOW_SAMPLES 16 // [8 16]\n\
        /* RENDERTARGETS: 1 */\n\
        #else\n\
        #define SHADOW_SAMPLES 8 // [8 16]\n\
        /* RENDERTARGETS: 0 */\n\
        #endif\n\
        const int samples = SHADOW_SAMPLES;\n\
        int sourceLine = __LINE__;";
    for (defines, samples, target) in [(&[][..], "8", 0), (&[("HIGH_QUALITY", "")][..], "16", 1)] {
        let pp = run(
            &[
                ("a.fsh", header),
                (
                    "lib.glsl",
                    "/* DRAWBUFFERS:04 */\nconst float shadowDistance = 128.0;",
                ),
            ],
            "a.fsh",
            defines,
        );
        let mut emitter = crate::token::Emitter::default();
        emitter.tokens(&pp.tokens);
        assert_eq!(
            emitter.finish().0,
            format!(
                "const int shadowMapResolution = 2048;\nconst float shadowDistance = 128.0;\nconst int samples = {samples};\nint sourceLine = 15;\n"
            )
        );
        assert_eq!(
            pp.consts
                .iter()
                .map(|value| (value.ty.as_str(), value.name.as_str(), value.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("int", "shadowMapResolution", "2048"),
                ("float", "shadowDistance", "128.0"),
                ("int", "samples", samples)
            ]
        );
        assert_eq!(pp.render_targets, Some(vec![target]));
        assert_eq!(pp.version, 450);
        assert_eq!(
            pp.extensions,
            [
                "GL_ARB_shader_texture_lod : enable",
                "GL_ARB_draw_buffers : enable"
            ]
        );
    }
}

#[test]
fn frontend_metadata_uses_expanded_values_and_comment_whitespace() {
    let pp = run(
        &[(
            "a.fsh",
            "#define SIZE 2048\nconst/* gap */ int shadowMapResolution = SIZE;\n#if 0 /* RENDERTARGETS: 9 */\n#endif /* RENDERTARGETS: 8 */\n#if 1 /* RENDERTARGETS: 2 */\n#endif",
        )],
        "a.fsh",
        &[],
    );
    assert_eq!(pp.render_targets, Some(vec![2]));
    assert_eq!(
        pp.consts,
        [ConstDirective {
            ty: "int".into(),
            name: "shadowMapResolution".into(),
            value: "2048".into()
        }]
    );
}

#[test]
fn unknown_directives_are_skipped_like_banner_comments() {
    let pp = run(
        &[(
            "a.fsh",
            "#version 330\n#==== Copyright banner ====\nfloat x;\n/*\n#==== in a comment\n*/\n",
        )],
        "a.fsh",
        &[],
    );
    let names: Vec<_> = pp.tokens.iter().filter_map(|t| t.ident()).collect();
    assert_eq!(names, ["float", "x"]);
    assert_eq!(
        pp.map.locate(pp.tokens[0].line).unwrap().to_string(),
        "a.fsh:3"
    );
    assert!(is_unknown_directive("#====") && is_unknown_directive("# banner"));
    assert!(!is_unknown_directive("#") && !is_unknown_directive("#  // note"));
    assert!(!is_unknown_directive("#extension GL_ARB_foo : enable"));
}
