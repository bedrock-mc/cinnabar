use crate::shader_safety;
use crate::shader_source;
// Every shader validates, not just parses: naga's parser accepts colliding varying locations
// and reserved identifiers that fail pipeline creation at runtime and silently skip the pass.

use crate::motion_blur_shader;
use crate::nametag_shader;
use crate::ui_shader;

/// Resolve the vanilla shader for standalone validation.
fn standalone(source: &str) -> String {
    let shader = shader_safety::from_actor_wesl(
        source,
        "standalone.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("checked constructor must produce WESL");
    };
    shader_source::standalone(&source, &[])
}

/// Checks emitted sampling and gradient order for divergent fragments.
fn assert_explicit_fragment_samples(source: &str, entry_name: &str) {
    let module = naga::front::wgsl::parse_str(source).expect("fragment shader parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("fragment shader validates");
    let function = module
        .entry_points
        .iter()
        .find(|entry| entry.name == entry_name)
        .map(|entry| &entry.function)
        .or_else(|| {
            module
                .functions
                .iter()
                .find(|(_, function)| function.name.as_deref() == Some(entry_name))
                .map(|(_, function)| function)
        })
        .expect("fragment sampling function exists");
    let mut samples = 0;
    for (_, expression) in function.expressions.iter() {
        if let naga::Expression::ImageSample { level, .. } = expression {
            assert!(
                matches!(level, naga::SampleLevel::Gradient { .. }),
                "{entry_name} must supply the original sample footprint explicitly"
            );
            samples += 1;
        }
    }
    assert!(samples > 0, "the fragment's sampling contract is exercised");
    let mut branched = false;
    let mut derivatives = 0;
    for statement in function.body.iter() {
        match statement {
            naga::Statement::Emit(range) => {
                for handle in range.clone() {
                    if matches!(
                        function.expressions[handle],
                        naga::Expression::Derivative { .. }
                    ) {
                        assert!(
                            !branched,
                            "{entry_name} computes gradients before rejection"
                        );
                        derivatives += 1;
                    }
                }
            }
            naga::Statement::If { .. }
            | naga::Statement::Switch { .. }
            | naga::Statement::Kill
            | naga::Statement::Return { .. } => branched = true,
            _ => {}
        }
    }
    assert!(
        derivatives >= 2,
        "the fragment computes both screen-space gradients"
    );
}

#[test]
fn divergent_fragments_keep_explicit_gradients_before_rejection() {
    for (raw, entry_name) in [
        (include_str!("../../src/actor.wesl"), "actor_colour"),
        (include_str!("../../src/hand_rig.wesl"), "hand_fragment"),
    ] {
        assert_explicit_fragment_samples(&standalone(raw), entry_name);
    }
    let shader = shader_safety::from_block_entity_wesl(
        include_str!("../../src/block_entity/block_entity.wesl"),
        "block entity gradients",
        render_model::BLOCK_ENTITY_VERTEX_WORDS,
        render_model::BLOCK_SELECTION_VERTICES_PER_EDGE,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("block entity constructor produces WESL");
    };
    let source = shader_source::standalone(&source, &[]);
    for entry in [
        "block_entity_overlay",
        "block_entity_crack",
        "portal_fragment",
    ] {
        assert_explicit_fragment_samples(&source, entry);
    }
}

/// Lists calls that pass a function's own matrix parameter on to another function.
///
/// Built-in shaders avoid this valid pattern for Adreno compiler compatibility.
fn forwarded_matrix_arguments(module: &naga::Module) -> Vec<String> {
    /// Visits calls in nested control flow without following calls into other functions.
    fn visit(
        module: &naga::Module,
        function: &naga::Function,
        block: &naga::Block,
        found: &mut Vec<String>,
    ) {
        for statement in block.iter() {
            match statement {
                naga::Statement::Call {
                    function: callee,
                    arguments,
                    ..
                } => {
                    for argument in arguments {
                        let naga::Expression::FunctionArgument(index) =
                            function.expressions[*argument]
                        else {
                            continue;
                        };
                        let parameter = &function.arguments[index as usize];
                        if matches!(
                            module.types[parameter.ty].inner,
                            naga::TypeInner::Matrix { .. }
                        ) {
                            found.push(format!(
                                "{} forwards {} to {}",
                                function.name.as_deref().unwrap_or("?"),
                                parameter.name.as_deref().unwrap_or("?"),
                                module.functions[*callee].name.as_deref().unwrap_or("?"),
                            ));
                        }
                    }
                }
                naga::Statement::Block(inner) => visit(module, function, inner, found),
                naga::Statement::If { accept, reject, .. } => {
                    visit(module, function, accept, found);
                    visit(module, function, reject, found);
                }
                naga::Statement::Switch { cases, .. } => {
                    for case in cases {
                        visit(module, function, &case.body, found);
                    }
                }
                naga::Statement::Loop {
                    body, continuing, ..
                } => {
                    visit(module, function, body, found);
                    visit(module, function, continuing, found);
                }
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for (_, function) in module.functions.iter() {
        visit(module, function, &function.body, &mut found);
    }
    for entry in &module.entry_points {
        visit(module, &entry.function, &entry.function.body, &mut found);
    }
    found
}

#[test]
fn block_entity_stages_never_forward_matrix_parameters() {
    let shader = shader_safety::from_block_entity_wesl(
        include_str!("../../src/block_entity/block_entity.wesl"),
        "block entity driver calls",
        render_model::BLOCK_ENTITY_VERTEX_WORDS,
        render_model::BLOCK_SELECTION_VERTICES_PER_EDGE,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("block entity constructor produces WESL");
    };
    let module = naga::front::wgsl::parse_str(&shader_source::standalone(&source, &[]))
        .expect("block entity shader parses");
    for entry in ["portal_vertex", "selection_line_vertex"] {
        assert!(
            module.entry_points.iter().any(|point| point.name == entry),
            "{entry} remains in the checked module"
        );
    }
    assert_eq!(forwarded_matrix_arguments(&module), Vec::<String>::new());
}

#[test]
fn dragon_dissolve_passes_preserve_identical_vertex_depths() {
    let module = naga::front::wgsl::parse_str(&standalone(include_str!("../../src/actor.wesl")))
        .expect("production actor shader parses");
    let vertex = module
        .entry_points
        .iter()
        .find(|entry| entry.stage == naga::ShaderStage::Vertex)
        .expect("actor vertex entry point");
    let result = vertex.function.result.as_ref().expect("vertex output");
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("actor vertex output is a struct");
    };
    let position = members.iter().find_map(|member| match member.binding {
        Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { invariant })) => Some(invariant),
        _ => None,
    });
    assert_eq!(
        position,
        Some(true),
        "the Equal-depth dissolve color pass requires invariant vertex positions"
    );
}

#[test]
fn every_shader_parses_and_validates() {
    let mut failures = Vec::new();
    let mut validated = 0;
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.ends_with(".wgsl") || name.ends_with(".wesl"))
            || name == "lighting.wesl"
            || name == "biome_tint.wesl"
            || name == "material.wesl"
            || name == "chunk_bindings.wesl"
        {
            continue;
        }
        let raw = std::fs::read_to_string(&path).unwrap();
        let source = if name == "ui.wgsl" {
            // Exercise the exact production constructor, including its renderer-owned style
            // constant injection, without publishing a test-only runtime API.
            let shader = ui_shader::from_wgsl(&raw, path.to_string_lossy());
            let bevy::shader::Source::Wgsl(source) = shader.source else {
                panic!("UI shader constructor must produce WGSL");
            };
            standalone(&source)
        } else if name == "motion_blur.wgsl" {
            // The production constructor injects the shared motion tile size.
            standalone(&motion_blur_shader::source(&raw, false))
        } else if name == "transparent_terrain.wesl" {
            // Composes the liquid and model modules, whose shared names only the composer resolves.
            shader_source::composed(&raw, &[])
        } else if name == "nametag.wesl" {
            let shader = nametag_shader::from_wesl(&raw, path.to_string_lossy());
            let bevy::shader::Source::Wesl(source) = shader.source else {
                panic!("nametag shader constructor must produce WESL");
            };
            // Validate the tested-glyph specialization; Bevy preprocesses this define at runtime.
            shader_source::standalone(&source, &["NAMETAG_ALPHA_TEST"])
        } else if matches!(name.as_str(), "actor.wesl" | "hand_rig.wesl") {
            let shader = shader_safety::from_actor_wesl(
                &raw,
                path.to_string_lossy(),
                render_model::ACTOR_GPU_INSTANCE_WORDS,
                render_model::ACTOR_RIG_VERTEX_WORDS,
            );
            let bevy::shader::Source::Wesl(source) = shader.source else {
                panic!("packed actor constructor must produce WESL");
            };
            standalone(&source)
        } else {
            standalone(&raw)
        };
        match naga::front::wgsl::parse_str(&source) {
            Err(error) => failures.push(format!("{name}: {error}")),
            Ok(module) => {
                if name == "ui.wgsl" {
                    let viewport_bytes = module.types.iter().find_map(|(_, ty)| {
                        if ty.name.as_deref() == Some("UiViewport")
                            && let naga::TypeInner::Struct { span, .. } = ty.inner
                        {
                            Some(span as usize)
                        } else {
                            None
                        }
                    });
                    assert_eq!(
                        viewport_bytes,
                        Some(std::mem::size_of::<ui_shader::UiViewportUniform>())
                    );
                }
                if let Err(error) = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                {
                    failures.push(format!("{name}: {error:?}"));
                }
                for call in forwarded_matrix_arguments(&module) {
                    failures.push(format!("{name}: {call}"));
                }
            }
        }
        validated += 1;
    }
    assert!(validated >= 10, "shader sources were not found");
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn review_render_world_actor_fragments_read_shared_fog() {
    for raw in [
        include_str!("../../src/actor.wesl"),
        include_str!("../../src/dropped_item.wesl"),
    ] {
        let module = naga::front::wgsl::parse_str(&standalone(raw)).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let fog = module
            .global_variables
            .iter()
            .find(|(_, global)| global.name.as_deref() == Some("world_atmosphere"))
            .expect("world fragments need the shared atmosphere uniform")
            .0;
        let fragment = module
            .entry_points
            .iter()
            .position(|entry| entry.stage == naga::ShaderStage::Fragment)
            .unwrap();
        assert!(!info.get_entry_point(fragment)[fog].is_empty());
    }
}

/// Validates every batched entry point, including those not covered by the cube pixel fixture.
#[test]
fn direct_terrain_unit_shaders_validate_with_and_without_msaa_cutout() {
    for raw in [
        include_str!("../../src/chunk.wesl"),
        include_str!("../../src/model.wesl"),
        include_str!("../../src/liquid.wesl"),
    ] {
        for definitions in [
            &["TERRAIN_UNITS"][..],
            &["TERRAIN_UNITS", "ALPHA_TO_COVERAGE"][..],
        ] {
            let source = shader_source::standalone(raw, definitions);
            let module = naga::front::wgsl::parse_str(&source).expect("unit shader parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("unit shader validates");
            assert!(
                module
                    .entry_points
                    .iter()
                    .any(|entry| entry.name == "vertex_unit")
            );
        }
    }
}
