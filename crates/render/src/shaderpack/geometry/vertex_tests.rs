use super::*;

fn input(location: u32, name: &str, ty: ValueType) -> VertexInput {
    VertexInput {
        location,
        name: name.into(),
        attribute: VertexAttribute::from_glsl_name(name),
        ty,
    }
}

fn vector(size: u8, scalar: ScalarType) -> ValueType {
    ValueType::Vector { size, scalar }
}

fn attribute(layout: &VertexBufferLayout, location: u32) -> (VertexFormat, u64) {
    let attribute = layout
        .attributes
        .iter()
        .find(|attribute| attribute.shader_location == location)
        .unwrap_or_else(|| panic!("location {location} is bound"));
    (attribute.format, attribute.offset)
}

fn constant_word(offset: u64) -> u32 {
    CONSTANT_STREAM_WORDS[(offset / 4) as usize]
}

/// The geometry and constant stream layouts of `inputs` read from the wide stream.
fn wide(inputs: &[VertexInput]) -> Result<[VertexBufferLayout; 2], VertexLayoutError> {
    let layouts = vertex_buffer_layouts(inputs, &TerrainLayout::WIDE)?;
    Ok(layouts.try_into().expect("wide streams have two buffers"))
}

#[test]
fn compatibility_inputs_read_their_stored_fields() {
    let float = ScalarType::Float;
    // The translator declares `mc_Entity` and `at_midBlock` as integer inputs.
    let int = ScalarType::Int;
    let inputs = [
        input(0, "iris_Vertex", vector(3, float)),
        input(1, "iris_Color", vector(4, float)),
        input(2, "iris_Normal", vector(3, float)),
        input(3, "iris_MultiTexCoord0", vector(4, float)),
        input(4, "iris_MultiTexCoord1", vector(4, float)),
        input(8, "mc_Entity", vector(4, int)),
        input(9, "mc_midTexCoord", vector(2, float)),
        input(10, "at_tangent", vector(4, float)),
        input(11, "at_midBlock", vector(4, int)),
    ];
    let [geometry, constant] = wide(&inputs).unwrap();
    assert_eq!(geometry.array_stride, IRIS_VERTEX_STRIDE);
    assert!(constant.attributes.is_empty());
    let expected = [
        (0, POSITION),
        (1, COLOR),
        (2, NORMAL),
        (3, UV0),
        (4, LIGHTMAP),
        (8, BLOCK_ID),
        (9, MID_TEX),
        (10, TANGENT),
        (11, MID_BLOCK),
    ];
    for (location, field) in expected {
        assert_eq!(
            attribute(&geometry, location),
            (field.format, u64::from(field.word) * 4)
        );
    }
}

#[test]
fn integer_stored_fields_bind_only_integer_inputs() {
    // Float declarations of these never reach the layout: the translator converts them from
    // integer inputs, so a float input means the two disagree.
    for (name, field) in [("mc_Entity", BLOCK_ID), ("at_midBlock", MID_BLOCK)] {
        assert_eq!(
            VertexAttribute::from_glsl_name(name).stored_numbers(),
            Some(StoredNumbers::Int)
        );
        let [geometry, _] = wide(&[input(8, name, vector(2, ScalarType::Int))])
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            attribute(&geometry, 8),
            (field.format, u64::from(field.word) * 4)
        );
        assert!(
            wide(&[input(8, name, vector(2, ScalarType::Float))]).is_err(),
            "{name}"
        );
    }
}

#[test]
fn the_lightmap_is_stored_once_for_every_name() {
    // The translator declares an integer `vaUV2` as a float input and converts it.
    let inputs = [
        input(4, "vaUV2", vector(2, ScalarType::Float)),
        input(5, "iris_MultiTexCoord2", vector(4, ScalarType::Float)),
    ];
    let [geometry, _] = wide(&inputs).unwrap();
    for location in [4, 5] {
        assert_eq!(
            attribute(&geometry, location),
            (LIGHTMAP.format, u64::from(LIGHTMAP.word) * 4),
            "gl_MultiTexCoord2 is a second name for the lightmap"
        );
    }
}

#[test]
fn attributes_terrain_lacks_read_iris_constants() {
    let inputs = [
        input(13, "mc_chunkFade", ValueType::Scalar(ScalarType::Float)),
        input(7, "vaUV1", vector(2, ScalarType::Int)),
        input(12, "at_velocity", vector(3, ScalarType::Float)),
        input(15, "myCustomInput", vector(4, ScalarType::Uint)),
    ];
    let [geometry, constant] = wide(&inputs).unwrap();
    assert!(geometry.attributes.is_empty());
    assert_eq!(
        constant.array_stride, 0,
        "every vertex reads the same constants"
    );
    let (format, offset) = attribute(&constant, 13);
    assert_eq!(format, VertexFormat::Float32);
    assert_eq!(
        f32::from_bits(constant_word(offset)),
        1.0,
        "chunks are fully faded in"
    );
    let (format, offset) = attribute(&constant, 7);
    assert_eq!(format, VertexFormat::Sint32x2);
    assert_eq!(
        [constant_word(offset), constant_word(offset + 4)],
        [0, 10],
        "OverlayTexture.NO_OVERLAY"
    );
    let (format, offset) = attribute(&constant, 12);
    assert_eq!(format, VertexFormat::Float32x3);
    assert_eq!(
        (0..3).map(|i| constant_word(offset + i * 4)).sum::<u32>(),
        0
    );
    let (format, offset) = attribute(&constant, 15);
    assert_eq!(format, VertexFormat::Uint32x4);
    assert_eq!(
        (0..4).map(|i| constant_word(offset + i * 4)).sum::<u32>(),
        0
    );
}

#[test]
fn inputs_of_types_terrain_cannot_supply_are_rejected() {
    let matrix = ValueType::Matrix {
        columns: 4,
        rows: 4,
        scalar: ScalarType::Float,
    };
    let error = wide(&[input(0, "iris_Vertex", matrix)]).unwrap_err();
    assert_eq!(
        error,
        VertexLayoutError::UnsupportedType {
            name: "iris_Vertex".into(),
            ty: matrix
        }
    );
    assert!(wide(&[input(1, "iris_Color", vector(4, ScalarType::Int))]).is_err());
    for scalar in [
        ScalarType::Bool,
        ScalarType::Int8,
        ScalarType::Int16,
        ScalarType::Int64,
        ScalarType::Uint8,
        ScalarType::Uint16,
        ScalarType::Uint64,
        ScalarType::Half,
        ScalarType::Double,
    ] {
        for ty in [ValueType::Scalar(scalar), vector(4, scalar)] {
            let inputs = [input(0, "iris_Vertex", ty)];
            for layout in [TerrainLayout::WIDE, TerrainLayout::compact(&inputs)] {
                assert_eq!(
                    vertex_buffer_layouts(&inputs, &layout).unwrap_err(),
                    VertexLayoutError::UnsupportedType {
                        name: "iris_Vertex".into(),
                        ty
                    },
                );
            }
        }
    }
}

#[test]
fn packed_layouts_store_only_what_terrain_reads_and_unpack_positions_per_draw() {
    let float = ScalarType::Float;
    // BSL-like terrain: no tangent, no at_midBlock.
    let inputs = [
        input(0, "iris_Vertex", vector(3, float)),
        input(1, "iris_Color", vector(4, float)),
        input(2, "iris_Normal", vector(3, float)),
        input(3, "iris_MultiTexCoord0", vector(4, float)),
        input(4, "iris_MultiTexCoord1", vector(4, float)),
        input(5, "iris_PackedPosition", vector(4, ScalarType::Uint)),
        input(6, "iris_DrawOrigin", vector(4, float)),
        input(8, "mc_Entity", vector(4, ScalarType::Int)),
        input(9, "mc_midTexCoord", vector(2, float)),
    ];
    let layout = TerrainLayout::compact(&inputs);
    assert!(layout.packed());
    // Packed position (two words, the lightmap in its `w`), colour, uv, mid uv, normal,
    // block id.
    assert_eq!(layout.stride(), 28);
    assert_eq!(layout.word(TANGENT), None);
    assert_eq!(layout.word(MID_BLOCK), None);
    assert_eq!(layout.word(LIGHTMAP), None);
    let layouts = vertex_buffer_layouts(&inputs, &layout).unwrap();
    let [geometry, constant, origins] = &layouts[..] else {
        panic!("packed layouts bind three buffers");
    };
    assert_eq!(geometry.array_stride, 28);
    assert_eq!(attribute(geometry, 5), (VertexFormat::Uint16x4, 0));
    assert_eq!(attribute(geometry, 1), (COLOR.format, 8));
    for (location, field) in [(1, COLOR), (2, NORMAL), (3, UV0), (8, BLOCK_ID)] {
        assert_eq!(
            attribute(geometry, location),
            (field.format, u64::from(layout.word(field).unwrap()) * 4)
        );
    }
    // The float lightmap reads two zeros (GL fills `(…, 0, 1)` as for the stored halves); the
    // translator adds the packed position's `w` bytes.
    let (format, offset) = attribute(constant, 4);
    assert_eq!(format, VertexFormat::Float32x2);
    assert_eq!(
        (0..2).map(|i| constant_word(offset + i * 4)).sum::<u32>(),
        0
    );
    // The float position reads zero; the origin comes per draw.
    let (format, offset) = attribute(constant, 0);
    assert_eq!(format, VertexFormat::Float32x3);
    assert_eq!(
        (0..3).map(|i| constant_word(offset + i * 4)).sum::<u32>(),
        0
    );
    assert_eq!(origins.step_mode, VertexStepMode::Instance);
    assert_eq!(origins.array_stride, DRAW_ORIGIN_BYTES);
    assert_eq!(attribute(origins, 6), (VertexFormat::Float32x4, 0));
    // The wide stream reads float positions and zero for both parts.
    let [geometry, constant] = wide(&inputs).unwrap();
    assert_eq!(attribute(&geometry, 0), (POSITION.format, 0));
    for location in [5, 6] {
        let (_, offset) = attribute(&constant, location);
        assert_eq!(
            (0..4).map(|i| constant_word(offset + i * 4)).sum::<u32>(),
            0
        );
    }
}

#[test]
fn packed_positions_unpack_exactly_on_the_grid() {
    let anchor = [1024, 0, -2048];
    let section = [1040, 64, -2064];
    let [x, y, z, scale] = draw_origin(section, anchor);
    assert_eq!(
        [x, y, z],
        [8.0, 56.0, -24.0],
        "section minus anchor minus the bias"
    );
    // A model corner 3/16 into the block at section-local (5, 0, 15): stored steps unpack to
    // the float position the wide stream holds.
    let local = [5.0 + 3.0 / 16.0, 0.0, 15.0];
    let steps = local.map(|axis: f32| ((axis + 8.0) * 2048.0).round());
    let unpacked = [
        x + steps[0] * scale,
        y + steps[1] * scale,
        z + steps[2] * scale,
    ];
    let wide = [16.0 + local[0], 64.0 + local[1], -16.0 + local[2]];
    assert_eq!(unpacked, wide);
}

#[test]
fn kernel_layout_words_name_every_field() {
    let [first, second, third, _] = TerrainLayout::WIDE.expand_words();
    assert_eq!(first, IRIS_VERTEX_WORDS | (0xff << 16) | (COLOR.word << 24));
    assert_eq!(
        second,
        UV0.word | (MID_TEX.word << 8) | (LIGHTMAP.word << 16) | (NORMAL.word << 24)
    );
    assert_eq!(
        third,
        TANGENT.word | (BLOCK_ID.word << 8) | (MID_BLOCK.word << 16) | (0xff << 24)
    );
    let packed = TerrainLayout::compact(std::iter::empty()).expand_words();
    assert_eq!(packed[0], 2 | (0xff << 8) | (0xff << 24), "position only");
}

#[test]
fn every_layout_placeholder_is_resolved() {
    let source = "IRIS_VERTEX_WORDS IRIS_WORD_POSITION IRIS_WORD_COLOR IRIS_WORD_UV0 \
        IRIS_WORD_MID_TEX IRIS_WORD_LIGHTMAP IRIS_WORD_NORMAL \
        IRIS_WORD_TANGENT IRIS_WORD_BLOCK_ID IRIS_WORD_MID_BLOCK";
    let resolved = substitute_layout_constants(source);
    assert!(!resolved.contains("IRIS_"), "{resolved}");
    assert!(resolved.starts_with(&format!("{IRIS_VERTEX_WORDS}u 0u 3u")));
}
