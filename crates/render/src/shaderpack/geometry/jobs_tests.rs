use super::*;
use meshing::Face;
use meshing::liquid::{LIQUID_DEPTH_WRITE_BIT, LIQUID_TWO_SIDED_BIT};

const PLAIN: u32 = 0;
const TWO_SIDED: u32 = 1;
const OVERLAY: u32 = 2;
const VARIED: u32 = 3;

fn materials() -> Vec<Material> {
    let with_flags = |flags| Material {
        flags,
        ..Material::unvaried()
    };
    let mut varied = with_flags(0);
    varied.variation_start = 4;
    varied.variation_count = 2;
    vec![
        with_flags(0),
        with_flags(assets::MATERIAL_FLAG_TWO_SIDED),
        with_flags(assets::MATERIAL_FLAG_OVERLAY_MASK),
        varied,
        with_flags(0),
        with_flags(assets::MATERIAL_FLAG_TWO_SIDED),
    ]
}

fn liquid(extra: u32) -> PackedLiquidQuad {
    let mut words =
        PackedLiquidQuad::try_pack([1, 2, 3], Face::PositiveY, [255; 4], 0, 0, [0, 0], false)
            .unwrap()
            .words();
    words[2] |= extra;
    PackedLiquidQuad::try_from_words(words).unwrap()
}

fn face_range(plan: &SectionPlan, kind: JobKind) -> Vec<(u32, u32)> {
    plan.jobs[kind as usize]
        .iter()
        .map(|job| (job[2], job_slots(job[3])))
        .collect()
}

#[test]
fn greedy_quads_expand_to_one_job_per_covered_block_and_categories_stay_contiguous() {
    let materials = materials();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }];
    let model_quads = [ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material: PLAIN,
        flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
    }];
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &templates,
        model_quads: &model_quads,
    };
    let cube_quads = [
        PackedQuad::new([0, 0, 0], Face::PositiveY, 2, 3, PLAIN),
        PackedQuad::new([0, 1, 0], Face::NegativeX, 1, 1, OVERLAY),
        PackedQuad::new([4, 0, 0], Face::PositiveZ, 1, 1, TWO_SIDED),
        PackedQuad::new([5, 0, 0], Face::PositiveZ, 1, 1, VARIED),
    ];
    let model_refs = [PackedModelRef::new(0, 0, 0, 1)];
    let draws = [PackedModelDrawRef::new(0, 0)];
    let liquids = [liquid(LIQUID_TWO_SIDED_BIT), liquid(LIQUID_DEPTH_WRITE_BIT)];
    let section = SectionGeometry {
        metadata_index: 7,
        quad_base: 100,
        cube_quads: &cube_quads,
        solid_len: 2,
        model_refs: &model_refs,
        model_draw_base: 40,
        model_draw_refs: &draws,
        transparent_model_draw_base: 50,
        transparent_model_draw_refs: &draws,
        liquid_base: 60,
        liquid_quads: &liquids,
    };
    let plan = plan_section(&section, &tables);

    // Solid: the 2x3 quad's six blocks, then lava.
    // Cutout: the overlay quad (face + overlay), the two-sided (leaf) quad and the quad whose
    // positional variant is two-sided, then the two-sided model quad.
    // Translucent: the back of the flat two-sided water top, its front, then the same model
    // drawn translucent.
    assert_eq!(plan.categories, [0..7, 7..15, 15..19]);
    assert_eq!(plan.faces, 19);
    // Single-sided cube faces and flat liquid faces group by direction ahead of everything
    // else in their category.
    assert_eq!(
        plan.facing[0][Face::PositiveY as usize],
        0..7,
        "cubes, then the flat lava top"
    );
    assert_eq!(
        plan.facing[1][Face::NegativeX as usize],
        7..9,
        "face and overlay"
    );
    assert_eq!(
        plan.facing[1][Face::PositiveZ as usize],
        9..9,
        "two-sided leaves stay outside axis culling"
    );
    assert_eq!(plan.facing[1][FACING_REST], 9..15, "leaves and the model");
    assert_eq!(
        plan.facing[2][Face::NegativeY as usize],
        15..16,
        "the water top's back faces down"
    );
    assert_eq!(plan.facing[2][Face::PositiveY as usize], 16..17);
    assert_eq!(plan.facing[2][FACING_REST], 17..19);
    let cubes = &plan.jobs[JobKind::Cube as usize];
    let blocks: Vec<_> = cubes[..6]
        .iter()
        .map(|job| (job[0], job[3] & 0xffff))
        .collect();
    assert_eq!(
        blocks,
        [
            (100, 0),
            (100, 1),
            (100, 0x100),
            (100, 0x101),
            (100, 0x200),
            (100, 0x201)
        ],
        "width steps vary fastest"
    );
    assert!(
        cubes.iter().all(|job| job[1] == 7),
        "every job names its origin slot"
    );
    assert_eq!(
        face_range(&plan, JobKind::Cube),
        [
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (5, 1),
            (7, 2),
            (9, 2),
            (11, 2)
        ]
    );
    assert_eq!(cubes[6][3], JOB_OVERLAY_FACE);
    assert_eq!(cubes[7][3], JOB_REVERSE_FACE);
    assert_eq!(face_range(&plan, JobKind::Model), [(13, 2), (17, 2)]);
    assert_eq!(
        plan.jobs[JobKind::Model as usize]
            .iter()
            .map(|job| job[0])
            .collect::<Vec<_>>(),
        [40, 50]
    );
    assert_eq!(
        face_range(&plan, JobKind::Liquid),
        [(6, 1), (15, 1), (16, 1)],
        "jobs follow category and group order; the back face is a job of its own"
    );
    let liquids = &plan.jobs[JobKind::Liquid as usize];
    assert_eq!(liquids[0][0], 61, "lava is the second record");
    assert_eq!(
        [liquids[1][0], liquids[1][3], liquids[2][0], liquids[2][3]],
        [60, JOB_BACK_ONLY, 60, 0]
    );
}

#[test]
fn sloped_liquid_tops_keep_both_sides_in_one_job_of_the_rest_group() {
    let materials = materials();
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &[],
        model_quads: &[],
    };
    let two_sided = |face, heights| {
        let mut words = PackedLiquidQuad::try_pack([1, 2, 3], face, heights, 0, 0, [0, 0], false)
            .unwrap()
            .words();
        words[2] |= LIQUID_TWO_SIDED_BIT;
        PackedLiquidQuad::try_from_words(words).unwrap()
    };
    let liquids = [
        two_sided(Face::PositiveY, [255, 200, 200, 255]),
        two_sided(Face::NegativeX, [0, 255, 200, 0]),
    ];
    let section = SectionGeometry {
        metadata_index: 0,
        quad_base: 0,
        cube_quads: &[],
        solid_len: 0,
        model_refs: &[],
        model_draw_base: 0,
        model_draw_refs: &[],
        transparent_model_draw_base: 0,
        transparent_model_draw_refs: &[],
        liquid_base: 0,
        liquid_quads: &liquids,
    };
    let plan = plan_section(&section, &tables);
    let translucent = &plan.facing[GeometryCategory::Translucent.index()];
    assert_eq!(
        translucent[Face::NegativeX as usize],
        0..1,
        "a side is a plane"
    );
    assert_eq!(translucent[Face::PositiveX as usize], 1..2, "its back");
    assert_eq!(
        translucent[FACING_REST],
        2..4,
        "the sloped top and its back"
    );
    assert_eq!(plan.jobs[JobKind::Liquid as usize][2][3], JOB_REVERSE_FACE);
}

#[test]
fn model_back_faces_follow_the_template_quads_sidedness_not_the_material() {
    // Cross plants are two-sided template quads with an ordinary cutout material; a two-sided
    // material on a one-sided quad keeps a single face, as the chunk renderer's model gate does.
    let materials = materials();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 2,
        flags: 0,
    }];
    let quad = |material, flags| ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[0; 2]; 4],
        material,
        flags,
    };
    let model_quads = [
        quad(PLAIN, assets::MODEL_QUAD_FLAG_TWO_SIDED),
        quad(TWO_SIDED, 0),
    ];
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &templates,
        model_quads: &model_quads,
    };
    let model_refs = [PackedModelRef::new(0, 0, 0, 0b11)];
    let draws = [PackedModelDrawRef::new(0, 0), PackedModelDrawRef::new(0, 1)];
    let section = SectionGeometry {
        metadata_index: 0,
        quad_base: 0,
        cube_quads: &[],
        solid_len: 0,
        model_refs: &model_refs,
        model_draw_base: 0,
        model_draw_refs: &draws,
        transparent_model_draw_base: 0,
        transparent_model_draw_refs: &[],
        liquid_base: 0,
        liquid_quads: &[],
    };
    let plan = plan_section(&section, &tables);
    assert_eq!(face_range(&plan, JobKind::Model), [(0, 2), (2, 1)]);
    assert_eq!(plan.jobs[JobKind::Model as usize][0][3], JOB_REVERSE_FACE);
}

#[test]
fn placing_a_plan_moves_its_faces_and_ranges() {
    let materials = materials();
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &[],
        model_quads: &[],
    };
    let cube_quads = [PackedQuad::new([0, 0, 0], Face::PositiveY, 1, 1, PLAIN)];
    let section = SectionGeometry {
        metadata_index: 0,
        quad_base: 0,
        cube_quads: &cube_quads,
        solid_len: 1,
        model_refs: &[],
        model_draw_base: 0,
        model_draw_refs: &[],
        transparent_model_draw_base: 0,
        transparent_model_draw_refs: &[],
        liquid_base: 0,
        liquid_quads: &[],
    };
    let mut plan = plan_section(&section, &tables);
    plan.place(30);
    assert_eq!(plan.categories, [30..31, 31..31, 31..31]);
    assert_eq!(plan.jobs[JobKind::Cube as usize][0][2], 30);
    assert_eq!(plan.jobs.iter().map(Vec::len).sum::<usize>(), 1);
}

#[test]
fn draws_naming_missing_templates_reserve_a_single_face() {
    let materials = materials();
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &[],
        model_quads: &[],
    };
    let model_refs = [PackedModelRef::new(0, 9, 0, 1)];
    let draws = [PackedModelDrawRef::new(0, 0), PackedModelDrawRef::new(5, 0)];
    let section = SectionGeometry {
        metadata_index: 0,
        quad_base: 0,
        cube_quads: &[],
        solid_len: 0,
        model_refs: &model_refs,
        model_draw_base: 0,
        model_draw_refs: &draws,
        transparent_model_draw_base: 0,
        transparent_model_draw_refs: &[],
        liquid_base: 0,
        liquid_quads: &[],
    };
    let plan = plan_section(&section, &tables);
    assert_eq!(face_range(&plan, JobKind::Model), [(0, 1), (1, 1)]);
}

#[test]
fn two_sided_cube_materials_and_variants_keep_unculled_reverse_faces() {
    let materials = materials();
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &[],
        model_quads: &[],
    };
    for material in [TWO_SIDED, VARIED] {
        let quads = [PackedQuad::new([0; 3], Face::PositiveZ, 1, 1, material)];
        let section = SectionGeometry {
            metadata_index: 0,
            quad_base: 0,
            cube_quads: &quads,
            solid_len: 1,
            model_refs: &[],
            model_draw_base: 0,
            model_draw_refs: &[],
            transparent_model_draw_base: 0,
            transparent_model_draw_refs: &[],
            liquid_base: 0,
            liquid_quads: &[],
        };
        let plan = plan_section(&section, &tables);
        assert_eq!(plan.faces, 2);
        assert_eq!(plan.categories[GeometryCategory::Cutout.index()], 0..2);
        assert_eq!(
            plan.facing[GeometryCategory::Cutout.index()][FACING_REST],
            0..2
        );
        assert_eq!(plan.jobs[JobKind::Cube as usize][0][3], JOB_REVERSE_FACE);
    }
}
