//! Public metadata inspection without a shader compiler or render host.

use shaderpack::{
    Environment, GpuVendor, HostOs, OptionKind, OptionValues, PackOrigin, PackSource, ProgramGroup,
    ProgramKind, ShaderPack, StageKind,
};

const VERTEX: &str = "#version 450\nvoid main() { gl_Position = vec4(0.0); }";
const FRAGMENT: &str = "#version 450\nout vec4 color; void main() { color = vec4(1.0); }";

/// Builds the pack's small, original metadata inputs.
fn pack() -> ShaderPack {
    ShaderPack::new(PackSource::from_files([
        (
            "lib/settings.glsl",
            "//#define GLOW\n#define QUALITY 2 // [1 2 3]\nconst bool generateShadowMipmap = true;\nconst int shadowMapResolution = 2048; // [1024 2048]\n#ifdef GLOW\n#endif\n",
        ),
        ("notes.txt", "#define IGNORE 1 // [1 2]\n"),
        (
            "shaders.properties",
            "#if defined(GLOW) && QUALITY >= 3\nprofile=bright\n#else\nprofile=plain\n#endif\nprogram.composite.enabled=GLOW\nprogram.world0/composite.enabled=QUALITY >= 3\n",
        ),
        ("composite.vsh", "root vertex"),
        ("composite.fsh", "root fragment"),
        ("world0/composite.vsh", "dimension vertex"),
        ("world0/composite.fsh", "dimension fragment"),
        ("world0/composite.csh", "dimension compute"),
        ("world0/composite2.fsh", ""),
        ("world0/composite10.fsh", ""),
        ("world0/dh_terrain.vsh", ""),
        ("world0/dh_terrain.fsh", ""),
        ("world0/lib/final.fsh", "nested source"),
        ("world-1/final.fsh", "nether fragment"),
    ]))
}

#[test]
fn options_and_properties_share_effective_macro_values() {
    let pack = pack();
    let environment = Environment::default();
    assert_eq!(pack.source().origin(), &PackOrigin::Memory);
    assert_eq!(pack.source().text("composite.fsh"), Some("root fragment"));
    let options = pack.options();
    assert_eq!(
        options
            .iter()
            .map(|option| option.name.as_str())
            .collect::<Vec<_>>(),
        [
            "GLOW",
            "QUALITY",
            "generateShadowMipmap",
            "shadowMapResolution"
        ]
    );
    assert_eq!(options[0].kind, OptionKind::Bool { default: false });
    assert_eq!(
        (options[1].file.as_str(), options[1].line),
        ("lib/settings.glsl", 2)
    );
    let defaults = OptionValues::new();
    assert_eq!(options[0].value(&defaults), "false");
    assert_eq!(options[1].value(&defaults), "2");
    assert_eq!(options[2].value(&defaults), "true");
    let defines = pack.defines(&environment, &defaults);
    assert!(!defines.contains_key("GLOW"));
    // Const options reach property conditions, as flags or by value.
    assert_eq!(
        defines.get("generateShadowMipmap").map(String::as_str),
        Some("")
    );
    assert_eq!(
        defines.get("shadowMapResolution").map(String::as_str),
        Some("2048")
    );
    assert_eq!(defines.get("QUALITY").map(String::as_str), Some("2"));
    let properties = pack.properties(&environment, &defaults);
    assert!(properties.diagnostics.is_empty());
    assert_eq!(properties.get("profile"), Some("plain"));
    let rule = properties.program_rule("world0", "composite").unwrap();
    assert!(!rule.is_enabled(&defines));

    let chosen = OptionValues::from([
        ("GLOW".into(), "true".into()),
        ("QUALITY".into(), "3".into()),
        ("UNKNOWN".into(), "1".into()),
    ]);
    let defines = pack.defines(&environment, &chosen);
    assert_eq!(defines.get("GLOW").map(String::as_str), Some(""));
    assert!(!defines.contains_key("UNKNOWN"));
    let properties = pack.properties(&environment, &chosen);
    assert_eq!(properties.get("profile"), Some("bright"));
    assert!(
        properties
            .program_rule("world0", "composite")
            .unwrap()
            .is_enabled(&defines)
    );
    assert!(
        properties
            .program_rule("", "composite")
            .unwrap()
            .is_enabled(&defines)
    );
}

#[test]
fn discovery_selects_complete_sets_and_keeps_disabled_programs_inspectable() {
    let pack = pack();
    let mut environment = Environment::default();
    assert_eq!(
        pack.dimension_folder("overworld", &Environment::default()),
        "world0"
    );
    assert_eq!(
        pack.dimension_folder("minecraft:the_nether", &Environment::default()),
        "world-1"
    );
    assert_eq!(
        pack.dimension_folder("minecraft:the_end", &Environment::default()),
        "world0"
    );
    let programs = pack.programs("world0", &environment);
    assert_eq!(
        programs
            .iter()
            .map(|program| (program.name.as_str(), program.kind))
            .collect::<Vec<_>>(),
        [
            ("composite", ProgramKind::Compute),
            ("composite", ProgramKind::Render),
            ("composite2", ProgramKind::Render),
            ("composite10", ProgramKind::Render),
        ]
    );
    assert!(programs.iter().all(|program| program.folder == "world0"));
    assert_eq!(programs[0].group, ProgramGroup::Composite);
    assert_eq!(
        programs[1].stages,
        [
            (StageKind::Vertex, "world0/composite.vsh".into()),
            (StageKind::Fragment, "world0/composite.fsh".into()),
        ]
    );
    assert_eq!(
        programs[0].stages,
        [(StageKind::Compute, "world0/composite.csh".into())]
    );
    assert_eq!(pack.programs("absent", &environment), programs);
    let root = pack.programs("", &environment);
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].folder, "");
    assert_eq!(root[0].stages[1].1, "composite.fsh");
    let nether = pack.programs(
        &pack.dimension_folder("the_nether", &Environment::default()),
        &environment,
    );
    assert_eq!(nether.len(), 1);
    assert_eq!(nether[0].name, "final");

    environment.distant_horizons = true;
    let programs = pack.programs("world0", &environment);
    assert_eq!(programs[0].name, "dh_terrain");
    assert_eq!(programs[0].group, ProgramGroup::DistantHorizons);
}

#[test]
fn dimension_mappings_preserve_duplicate_order_wildcard_and_root() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "dimension.z = overworld\ndimension.a = the_end *\ndimension.z = the_end *\ndimension.root = custom:root\ndimension.missing = custom:absent\n",
        ),
        ("z/final.fsh", ""),
        ("a/composite.fsh", ""),
        ("final.fsh", ""),
    ]));
    let environment = Environment::default();
    assert_eq!(
        pack.dimension_folder("the_end", &Environment::default()),
        "a"
    );
    assert_eq!(
        pack.dimension_folder("custom:moon", &Environment::default()),
        "a"
    );
    assert_eq!(
        pack.dimension_folder("custom:absent", &Environment::default()),
        "a"
    );
    assert_eq!(
        pack.dimension_folder("overworld", &Environment::default()),
        "a"
    );
    assert_eq!(pack.programs("", &environment)[0].name, "final");

    let root_mapping = ShaderPack::new(PackSource::from_files([
        ("dimension.properties", "dimension. = custom:root *\n"),
        ("world0/composite.fsh", ""),
        ("final.fsh", ""),
    ]));
    assert_eq!(
        root_mapping.dimension_folder("custom:root", &Environment::default()),
        ""
    );
    assert_eq!(
        root_mapping.dimension_folder("custom:unknown", &Environment::default()),
        ""
    );
    assert_eq!(
        root_mapping.programs("missing", &environment)[0].name,
        "final"
    );
}

#[test]
fn enable_rules_use_numeric_macros_and_retain_enabled_on_evaluation_errors() {
    let pack = ShaderPack::new(PackSource::from_files([(
        "shaders.properties",
        "program.composite.enabled=false\nprogram.world0/composite.enabled=FLAG && HEX == 16 && NEG < 0 && !TEXT && !MISSING\nprogram.final.enabled=false\nprogram.final.enabled=unknown(1)\nprogram.composite2.enabled=false && unknown(1)\n",
    )]));
    let properties = pack.properties(&Environment::default(), &OptionValues::new());
    assert!(properties.diagnostics.is_empty());
    let defines = [
        ("FLAG".into(), "".into()),
        ("HEX".into(), "0x10u".into()),
        ("NEG".into(), " -2.0f ".into()),
        ("TEXT".into(), "some_name".into()),
    ]
    .into();
    assert!(
        properties
            .program_rule("world0", "composite")
            .unwrap()
            .is_enabled(&defines)
    );
    assert!(
        !properties
            .program_rule("world-1", "composite")
            .unwrap()
            .is_enabled(&defines)
    );
    assert!(
        properties
            .program_rule("", "final")
            .unwrap()
            .is_enabled(&defines)
    );
    assert!(
        !properties
            .program_rule("", "composite2")
            .unwrap()
            .is_enabled(&defines)
    );
    assert!(properties.program_rule("", "absent").is_none());
}

#[test]
fn environment_macros_follow_the_selected_description() {
    let environment = Environment {
        mc_version: 12000,
        gl_version: 450,
        iris_version: 10800,
        os: HostOs::Linux,
        vendor: GpuVendor::Amd,
        render_quality: 0.5,
        shadow_quality: 2.0,
        hand_depth: 0.25,
        normal_map: false,
        specular_map: false,
        lab_pbr: false,
        anisotropic_filtering: 0,
        distant_horizons: true,
    };
    let pack = ShaderPack::new(PackSource::from_files([("notes.txt", "")]));
    let defines = pack.defines(&environment, &OptionValues::new());
    for (name, value) in [
        ("MC_VERSION", "12000"),
        ("MC_GL_VERSION", "450"),
        ("MC_GLSL_VERSION", "450"),
        ("IRIS_VERSION", "10800"),
        ("MC_RENDER_QUALITY", "0.5"),
        ("MC_SHADOW_QUALITY", "2.0"),
        ("MC_HAND_DEPTH", "0.25"),
        ("MC_OS_LINUX", ""),
        ("MC_GL_VENDOR_AMD", ""),
        ("MC_GL_VENDOR_ATI", ""),
        ("MC_GL_RENDERER_RADEON", ""),
        ("DISTANT_HORIZONS", ""),
    ] {
        assert_eq!(defines.get(name).map(String::as_str), Some(value), "{name}");
    }
    for name in [
        "MC_OS_WINDOWS",
        "MC_NORMAL_MAP",
        "MC_SPECULAR_MAP",
        "MC_TEXTURE_FORMAT_LAB_PBR",
        "MC_ANISOTROPIC_FILTERING",
    ] {
        assert!(!defines.contains_key(name), "{name}");
    }
    assert!(pack.options().is_empty());
    assert!(
        pack.properties(&environment, &OptionValues::new())
            .entries
            .is_empty()
    );
    assert!(pack.programs("", &environment).is_empty());
}

#[test]
fn fallback_programs_use_enable_rules_from_the_selected_directory() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shaders.properties",
            "program.final.enabled = true\nprogram.world0/final.enabled = false",
        ),
        ("final.vsh", ""),
        ("final.fsh", ""),
    ]));
    let environment = Environment::default();
    let values = OptionValues::new();
    let defines = pack.defines(&environment, &values);
    let properties = pack.properties(&environment, &values);
    let selected = pack.programs("world0", &environment);
    assert_eq!(selected.len(), 1);
    let program = &selected[0];
    assert_eq!(program.folder, "");
    assert!(
        properties
            .program_rule(&program.folder, &program.name)
            .unwrap()
            .is_enabled(&defines)
    );
}

#[test]
fn dimension_program_sets_replace_the_root_as_a_whole() {
    let pack = ShaderPack::new(PackSource::from_files([
        ("composite.vsh", VERTEX),
        ("composite.fsh", FRAGMENT),
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
        ("world0/composite.vsh", VERTEX),
        (
            "world0/begin.csh",
            "#version 450\nlayout(local_size_x=1) in; void main() {}",
        ),
    ]));
    let selected = pack.programs("world0", &Environment::default());
    assert_eq!(
        selected.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["begin", "composite"]
    );
    let composite = selected.iter().find(|p| p.name == "composite").unwrap();
    assert_eq!(
        composite.stages,
        [(StageKind::Vertex, "world0/composite.vsh".to_string())]
    );
}

#[test]
fn explicit_dimension_mappings_use_root_for_unmapped_ids() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "dimension.custom = minecraft:the_nether",
        ),
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
        ("world0/composite.vsh", VERTEX),
        ("world0/composite.fsh", FRAGMENT),
    ]));
    assert_eq!(
        pack.dimension_folder("minecraft:overworld", &Environment::default()),
        ""
    );
    let folder = pack.dimension_folder("minecraft:overworld", &Environment::default());
    let selected = pack.programs(&folder, &Environment::default());
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "final");
}

#[test]
fn dimension_ids_normalize_namespaces_and_last_mapping_wins() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "dimension.first = the_nether\ndimension.last = minecraft:the_nether\ndimension.other = the_end *",
        ),
        ("first/final.vsh", VERTEX),
        ("last/final.vsh", VERTEX),
        ("other/final.vsh", VERTEX),
    ]));
    assert_eq!(
        pack.dimension_folder("minecraft:the_nether", &Environment::default()),
        "last"
    );
    assert_eq!(
        pack.dimension_folder("the_nether", &Environment::default()),
        "last"
    );
    assert_eq!(
        pack.dimension_folder("minecraft:the_end", &Environment::default()),
        "other"
    );
    assert_eq!(
        pack.dimension_folder("custom:moon", &Environment::default()),
        "other"
    );
}

#[test]
fn dimension_properties_replace_keys_without_reordering_them() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "dimension.a = overworld\ndimension.b = the_end\ndimension.a = the_end",
        ),
        ("a/final.vsh", VERTEX),
        ("b/final.vsh", VERTEX),
    ]));
    assert_eq!(
        pack.dimension_folder("overworld", &Environment::default()),
        ""
    );
    assert_eq!(
        pack.dimension_folder("the_end", &Environment::default()),
        "b"
    );
}

#[test]
fn legacy_and_absent_override_folders_choose_one_base_set() {
    for properties in ["", "# only comments"] {
        let pack = ShaderPack::new(PackSource::from_files([
            ("dimension.properties", properties),
            ("world0/final.vsh", VERTEX),
            ("world-1/composite.vsh", VERTEX),
            ("final.vsh", VERTEX),
        ]));
        assert_eq!(
            pack.dimension_folder("the_nether", &Environment::default()),
            "world-1"
        );
        assert_eq!(
            pack.dimension_folder("the_end", &Environment::default()),
            "world0"
        );
        assert_eq!(
            pack.dimension_folder("custom:moon", &Environment::default()),
            "world0"
        );
        let selected = pack.programs("missing", &Environment::default());
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].folder, "world0");
    }
}

#[test]
fn unqualified_dimension_mappings_match_namespaced_queries() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "dimension.customNether = the_nether",
        ),
        ("customNether/final.vsh", ""),
        ("customNether/final.fsh", ""),
        ("world-1/composite.vsh", ""),
        ("world-1/composite.fsh", ""),
    ]));
    let folder = pack.dimension_folder("minecraft:the_nether", &Environment::default());
    assert_eq!(folder, "customNether");
    let selected = pack.programs(&folder, &Environment::default());
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "final");
    assert_eq!(selected[0].folder, "customNether");
}

#[test]
fn option_locations_preserve_distinct_exact_source_keys() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            r"a\b.glsl",
            "const bool shadowtex0Mipmap=true;\nconst bool shadowtex1Mipmap=false;",
        ),
        (
            "a/b.glsl",
            "\nconst bool shadowtex0Nearest=true;\nconst bool shadowtex1Nearest=false;",
        ),
    ]));
    let options = pack.options();
    assert_eq!(options.len(), 4);
    for (name, key, line, declaration) in [
        (
            "shadowtex0Mipmap",
            r"a\b.glsl",
            1,
            "const bool shadowtex0Mipmap=true;",
        ),
        (
            "shadowtex1Mipmap",
            r"a\b.glsl",
            2,
            "const bool shadowtex1Mipmap=false;",
        ),
        (
            "shadowtex0Nearest",
            "a/b.glsl",
            2,
            "const bool shadowtex0Nearest=true;",
        ),
        (
            "shadowtex1Nearest",
            "a/b.glsl",
            3,
            "const bool shadowtex1Nearest=false;",
        ),
    ] {
        let option = options.iter().find(|option| option.name == name).unwrap();
        let defining_text = pack.source().text(&option.file).unwrap();
        assert_eq!(
            defining_text.lines().nth((option.line - 1) as usize),
            Some(declaration),
            "{name} must point to its defining source line"
        );
        assert_eq!((option.file.as_str(), option.line), (key, line));
        assert_eq!(
            pack.source().read_bytes(&option.file).unwrap(),
            defining_text.as_bytes()
        );
    }
}

#[test]
fn dimension_properties_see_environment_macros() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "dimension.properties",
            "#ifdef IS_IRIS\ndimension.iris = overworld\n#else\ndimension.other = overworld\n#endif\n",
        ),
        ("iris/final.fsh", FRAGMENT),
        ("other/final.fsh", FRAGMENT),
    ]));
    assert_eq!(
        pack.dimension_folder("overworld", &Environment::default()),
        "iris"
    );
}
