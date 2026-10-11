use std::{fs, path::PathBuf};

use crate::shader_source;

/// Loads the same generated biome module used by the renderer.
fn shader(name: &str) -> String {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("read {name}: {error}"));
    if name == "biome_tint.wesl" {
        crate::material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(&source))
    } else {
        source
    }
}

#[test]
fn shared_shader_uses_lattice_kernel_and_uniform_fast_path() {
    let source = shader("biome_tint.wesl");

    assert!(source.contains("BIOME_DISTANCE_EPSILON"));
    assert!(source.contains("lattice_point_index(position) * BIOME_POINT_WORDS"));
    assert!(source.contains("if (uniform_tint != 0xffffffffu)"));
    assert!(source.contains("offset.y"));
}

#[test]
fn seasonal_shader_uses_bounded_species_cells_and_direct_biome_lookup() {
    let source = shader("biome_tint.wesl");
    assert!(source.contains("seasonal_foliage: array<vec4<f32>, SEASONAL_FOLIAGE_COUNT>"));
    assert!(!source.contains("unpack_linear_rgb10(tint.seasonal_foliage"));
    assert!(source.contains("min(species + exposed, SEASONAL_FOLIAGE_COUNT - 1u)"));
    let direct = source
        .find("safe_tint_index(packed_biome_tint_index(record, coordinate))")
        .unwrap();
    assert!(direct < source.find("let lattice_words").unwrap());
    assert!(source.contains(&format!(
        "const SEASONAL_FOLIAGE_COUNT: u32 = {}u;",
        assets::SEASONAL_FOLIAGE_COUNT
    )));
    for (name, flags) in [
        (
            "SEASONAL_EVERGREEN_CELL",
            assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        ),
        ("SEASONAL_BIRCH_CELL", assets::MATERIAL_FLAG_BIRCH_FOLIAGE),
        ("SEASONAL_DEFAULT_CELL", 0),
    ] {
        assert!(source.contains(&format!(
            "const {name}: u32 = {}u;",
            assets::seasonal_foliage_palette_index(flags, false),
        )));
    }
    let standalone = shader_source::standalone(&source, &[]);
    let module = naga::front::wgsl::parse_str(&standalone).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
fn every_tinted_pipeline_calls_the_shared_blender() {
    for name in ["chunk.wesl", "model.wesl", "liquid.wesl"] {
        let source = shader(name);
        assert!(
            source.contains("import render::biome_tint"),
            "{name} must import the common biome contract"
        );
        assert!(
            source.contains("blended_biome_tint("),
            "{name} must apply the same blending kernel"
        );
    }
}

#[test]
fn foliage_variants_select_their_palette_inside_the_shared_average() {
    let source = shader("biome_tint.wesl");
    assert!(source.contains("fn special_foliage_tint("));
    assert!(
        source.contains("case 0x200u: { return unpack_linear_rgb10(biome_tints[tint].birch); }")
    );
    assert!(
        source
            .contains("case 0x400u: { return unpack_linear_rgb10(biome_tints[tint].evergreen); }")
    );
    assert!(
        source.contains(
            "case 0x600u: { return unpack_linear_rgb10(biome_tints[tint].dry_foliage); }"
        )
    );
    assert!(source.contains(
        "tint_domain_colour(tint, tint_kind, material_flags, position + vec3<i32>(world_origin))"
    ));
    assert!(!source.contains("if (tint_kind == 0x20u"));
}

#[test]
fn model_tints_use_the_block_position_for_every_vertex() {
    let source = shader("model.wesl");
    assert!(source.contains("out.local_position = block_position;"));
    assert!(source.contains("@interpolate(flat) local_position"));
}

#[test]
fn gpu_lattice_count_is_bounded_by_the_cpu_format_even_for_corrupt_words() {
    let source = shader("biome_tint.wesl");
    let standalone = shader_source::standalone(&source, &[]);
    let module = naga::front::wgsl::parse_str(&standalone).unwrap();
    let (_, count) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("lattice_biome_count"))
        .unwrap();
    let returned = count
        .body
        .iter()
        .find_map(|statement| match statement {
            naga::Statement::Return { value: Some(value) } => Some(*value),
            _ => None,
        })
        .expect("count helper must return its bounded value");
    let naga::Expression::Math {
        fun: naga::MathFunction::Min,
        arg,
        arg1: Some(bound),
        ..
    } = count.expressions[returned]
    else {
        panic!("GPU-loaded counts must return min, not an unchecked loop limit");
    };
    let naga::Expression::Load { pointer } = count.expressions[arg] else {
        panic!("count must be loaded from GPU storage");
    };
    let naga::Expression::Access { base, .. } = count.expressions[pointer] else {
        panic!("count must index the packed biome array");
    };
    let naga::Expression::GlobalVariable(records) = count.expressions[base] else {
        panic!("count must use the storage-buffer binding");
    };
    assert_eq!(
        module.global_variables[records].name.as_deref(),
        Some("biome_records")
    );
    let naga::Expression::Constant(constant) = count.expressions[bound] else {
        panic!("count cap must be a generated format constant");
    };
    assert_eq!(
        module.constants[constant].name.as_deref(),
        Some("BIOME_BIOME_LIMIT")
    );
    let naga::Expression::Literal(naga::Literal::U32(limit)) =
        module.global_expressions[module.constants[constant].init]
    else {
        panic!("format limit must resolve to u32");
    };
    assert_eq!(limit as usize, meshing::biome_lattice::LATTICE_BIOME_LIMIT);
    for corrupt_word in [0, 1, limit, limit + 1, 1.0_f32.to_bits(), u32::MAX] {
        assert!(corrupt_word.min(limit) <= limit);
    }
    assert!(source.contains("let count = lattice_biome_count(start);"));
    assert!(source.contains("i < count;"));
    assert!(!source.contains("i < biome_records[start]"));
}

#[test]
fn biome_reads_check_spans_before_address_addition_and_reject_bad_weights() {
    let source = shader("biome_tint.wesl");
    assert!(source.contains("if (start > length) { return false; }"));
    assert!(source.contains("return words <= length - start;"));
    assert!(
        source.contains("biome_record_span_valid(record, BIOME_DESCRIPTOR_WORDS + lattice_words)")
    );
    assert!(source.contains("relative >= arrayLength(&biome_records) - record"));
    assert!(source.contains("any(residue >= vec3(BIOME_RESIDUE_SIDE))"));
    assert!(source.contains("fraction >= 0.0 && fraction <= 1.0"));
    assert!(source.contains("bits > 32u"));
}

/// naga's HLSL builds each const array through a by-value constructor; FXC holds its arguments,
/// result and static copy as temps, and rejects a shader over 4096 temp registers.
#[test]
fn tinted_pipelines_keep_const_tables_within_the_fxc_temp_budget() {
    for name in ["chunk.wesl", "model.wesl", "liquid.wesl"] {
        let standalone = shader_source::standalone(&shader(name), &[]);
        let module = naga::front::wgsl::parse_str(&standalone).unwrap();
        let elements: u32 = module
            .constants
            .iter()
            .filter_map(|(_, constant)| match module.types[constant.ty].inner {
                naga::TypeInner::Array {
                    size: naga::ArraySize::Constant(size),
                    ..
                } => Some(size.get()),
                _ => None,
            })
            .sum();
        assert!(
            elements * 3 < 4096,
            "{name} const arrays need {elements} x 3 FXC temp registers"
        );
    }
}
