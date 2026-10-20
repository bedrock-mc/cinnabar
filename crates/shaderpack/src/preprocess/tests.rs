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
            TokKind::Int(n) => Some(n.trim_end_matches('u')),
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
    assert_eq!(error.location.unwrap().to_string(), "a.fsh:1");
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
                TokKind::Int(value) => Some(value.trim_end_matches('u')),
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
        text: format!("{MARKER}0 {MARKER}0"),
        map: LineMap::default(),
        directives: vec![Directive::Const(ConstDirective {
            ty: "float".into(),
            name: "value".into(),
            value: "x".repeat(512),
        })],
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
