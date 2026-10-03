use std::{fs, path::PathBuf};

/// Loads the same generated biome module used by the renderer.
fn shader(name: &str) -> String {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("read {name}: {error}"));
    if name == "biome_tint.wgsl" {
        meshing::biome_lattice::shader_source(&source)
    } else {
        source
    }
}

#[test]
fn shared_shader_uses_lattice_kernel_and_uniform_fast_path() {
    let source = shader("biome_tint.wgsl");

    assert!(source.contains("BIOME_DISTANCE_EPSILON"));
    assert!(source.contains("lattice_point_index(position) * BIOME_POINT_WORDS"));
    assert!(source.contains("if (uniform_tint != 0xffffffffu)"));
    assert!(source.contains("offset.y"));
}

#[test]
fn every_tinted_pipeline_calls_the_shared_blender() {
    for name in ["chunk.wgsl", "model.wgsl", "liquid.wgsl"] {
        let source = shader(name);
        assert!(
            source.contains("#import cinnabar::biome_tint"),
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
    let source = shader("biome_tint.wgsl");
    assert!(source.contains("fn special_foliage_tint("));
    assert!(source.contains("case 0x200u: { return unpack_linear_rgb10(tint.birch); }"));
    assert!(source.contains("case 0x400u: { return unpack_linear_rgb10(tint.evergreen); }"));
    assert!(source.contains("case 0x600u: { return unpack_linear_rgb10(tint.dry_foliage); }"));
    assert!(source.contains(
        "tint_domain_colour(tint, tint_kind, material_flags, position + vec3<i32>(world_origin))"
    ));
    assert!(!source.contains("if (tint_kind == 0x20u"));
}

#[test]
fn model_tints_use_the_block_position_for_every_vertex() {
    let source = shader("model.wgsl");
    assert!(source.contains("out.local_position = block_position;"));
    assert!(source.contains("@interpolate(flat) local_position"));
}
