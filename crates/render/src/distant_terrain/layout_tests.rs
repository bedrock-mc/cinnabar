use std::collections::HashMap;

use super::*;

/// The value of every scalar constant `module` declares, by name.
fn declared_constants(module: &naga::Module) -> HashMap<String, naga::Literal> {
    module
        .constants
        .iter()
        .filter_map(|(_, constant)| {
            let naga::Expression::Literal(literal) = module.global_expressions[constant.init]
            else {
                return None;
            };
            Some((constant.name.clone()?, literal))
        })
        .collect()
}

#[test]
fn the_prelude_declares_every_shared_constant_with_its_rust_value() {
    let module = naga::front::wgsl::parse_str(&prelude()).expect("the prelude parses");
    let declared = declared_constants(&module);
    let expected = shader_constants();
    for (name, value) in &expected {
        assert_eq!(
            declared.get(name),
            Some(&naga::Literal::U32(*value)),
            "{name}"
        );
    }
    for (name, value) in shader_float_constants() {
        assert_eq!(
            declared.get(&name),
            Some(&naga::Literal::F32(value)),
            "{name}"
        );
    }
    assert_eq!(
        declared.len(),
        expected.len() + shader_float_constants().len(),
        "no constant is declared twice or by hand"
    );
}

#[test]
fn bit_field_constants_decode_the_rust_words() {
    use lod::{
        mesh::quad::Quad,
        voxel::{Light, Voxel},
    };
    let constants: HashMap<String, u32> = shader_constants().into_iter().collect();
    let read = |word: u32, name: &str| {
        (word >> constants[&format!("{name}_SHIFT")]) & constants[&format!("{name}_MASK")]
    };
    let payload = Voxel::new(MaterialId::new(1234).unwrap(), Light::new(13, 2), 5, true).bits();
    let quad = Quad::new(Face::PosZ, [3, 17, 31], [16, 2], payload);
    assert_eq!(read(quad.geometry(), "QUAD_X"), 3);
    assert_eq!(read(quad.geometry(), "QUAD_Y"), 17);
    assert_eq!(read(quad.geometry(), "QUAD_Z"), 31);
    assert_eq!(
        read(quad.geometry(), "QUAD_FACE"),
        Face::PosZ.index() as u32
    );
    assert_eq!(read(quad.geometry(), "QUAD_WIDTH"), 15);
    assert_eq!(read(quad.geometry(), "QUAD_HEIGHT"), 1);
    assert_eq!(read(quad.payload(), "VOXEL_MATERIAL"), 1234);
    assert_eq!(read(quad.payload(), "VOXEL_SKY_LIGHT"), 13);
    assert_eq!(read(quad.payload(), "VOXEL_BLOCK_LIGHT"), 2);
    let colour = FaceColour::from_srgb([10, 20, 30], TintClass::Foliage, 7);
    assert_eq!(read(colour.bits(), "COLOUR_GREEN"), 20);
    assert_eq!(
        read(colour.bits(), "COLOUR_TINT"),
        TintClass::Foliage.index() as u32
    );
    assert_eq!(read(colour.bits(), "COLOUR_TINT_WEIGHT"), 7);
}

/// Byte offset of each member of the WGSL struct `name`, and its size.
fn wgsl_layout(module: &naga::Module, name: &str) -> (Vec<(String, u32)>, u32) {
    let (_, ty) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some(name))
        .expect("declared struct");
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("{name} is a struct");
    };
    (
        members
            .iter()
            .map(|member| (member.name.clone().unwrap_or_default(), member.offset))
            .collect(),
        *span,
    )
}

#[test]
fn the_wgsl_view_structs_match_the_rust_uniforms() {
    let module = naga::front::wgsl::parse_str(&prelude()).expect("the prelude parses");
    let (members, span) = wgsl_layout(&module, "LodView");
    assert_eq!(span as usize, size_of::<LodView>());
    let view = LodView::zeroed();
    let base = std::ptr::from_ref(&view) as usize;
    let rust_offsets = [
        (
            "clip_from_rel",
            std::ptr::from_ref(&view.clip_from_rel) as usize,
        ),
        ("planes", std::ptr::from_ref(&view.planes) as usize),
        ("camera", std::ptr::from_ref(&view.camera) as usize),
        (
            "camera_frac",
            std::ptr::from_ref(&view.camera_frac) as usize,
        ),
        ("viewport", std::ptr::from_ref(&view.viewport) as usize),
        ("depth", std::ptr::from_ref(&view.depth) as usize),
        ("counts", std::ptr::from_ref(&view.counts) as usize),
        ("resident", std::ptr::from_ref(&view.resident) as usize),
        ("fog", std::ptr::from_ref(&view.fog) as usize),
        ("fog_end", std::ptr::from_ref(&view.fog_end) as usize),
        ("shade", std::ptr::from_ref(&view.shade) as usize),
        ("tables", std::ptr::from_ref(&view.tables) as usize),
        (
            "tables_extra",
            std::ptr::from_ref(&view.tables_extra) as usize,
        ),
    ]
    .map(|(name, address)| (name.to_owned(), (address - base) as u32));
    assert_eq!(members, rust_offsets);
    let (_, span) = wgsl_layout(&module, "DrawPass");
    assert_eq!(span as usize, size_of::<DrawPass>());
    assert!(size_of::<DrawPass>() as u64 <= DRAW_PASS_STRIDE);
}

#[test]
fn capacities_fit_the_adapter_and_scale_with_the_pool() {
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: u64::from(u32::MAX),
        max_buffer_size: u64::MAX,
        ..wgpu::Limits::default()
    };
    let medium = Capacity::new(256 << 20, &limits).expect("medium pool");
    assert_eq!(medium.pool_bytes(), 256 << 20);
    assert_eq!(
        u64::from(medium.clusters),
        (256 << 20) / 8 / QUADS_PER_CLUSTER_SLOT
    );
    let high = Capacity::new(512 << 20, &limits).expect("high pool");
    assert!(high.clusters > medium.clusters);
    let tight = wgpu::Limits {
        max_storage_buffer_binding_size: 128 << 20,
        max_buffer_size: 256 << 20,
        ..wgpu::Limits::default()
    };
    let capped = Capacity::new(512 << 20, &tight).expect("capped pool");
    assert_eq!(
        capped.pool_bytes(),
        128 << 20,
        "the binding limit caps the pool"
    );
    for bytes in [
        capped.flags_bytes(),
        capped.lists_bytes(),
        capped.staging_bytes(),
        capped.sections_bytes(),
        capped.clusters_bytes(),
    ] {
        assert!(bytes <= 128 << 20);
    }
    let tiny = wgpu::Limits {
        max_storage_buffer_binding_size: 1 << 10,
        ..wgpu::Limits::default()
    };
    assert_eq!(Capacity::new(256 << 20, &tiny), None);
}
