use super::*;
use glam::{DMat4, DVec3};

/// Java's sine table, indexed independently of the production helper.
fn table_sin(radians: f64) -> f64 {
    let index = ((radians as f32 * 10430.378) as i32) & 0xFFFF;
    (f64::from(index) * 2.0 * std::f64::consts::PI / 65536.0).sin()
}

fn table_cos(radians: f64) -> f64 {
    let index = ((radians as f32 * 10430.378 + 16384.0) as i32) & 0xFFFF;
    (f64::from(index) * 2.0 * std::f64::consts::PI / 65536.0).sin()
}

const PI64: f64 = std::f64::consts::PI;

/// A GL matrix stack in f64: each call post-multiplies, exactly as Java issues them.
#[derive(Clone, Copy)]
struct Gl(DMat4);

impl Gl {
    fn new() -> Self {
        Self(DMat4::IDENTITY)
    }
    fn translatef(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.0 *= DMat4::from_translation(DVec3::new(x, y, z));
        self
    }
    fn rotatef(&mut self, degrees: f64, x: f64, y: f64, z: f64) -> &mut Self {
        self.0 *= DMat4::from_axis_angle(DVec3::new(x, y, z), degrees.to_radians());
        self
    }
    fn scalef(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.0 *= DMat4::from_scale(DVec3::new(x, y, z));
        self
    }
    /// `ModelRenderer.postRender`: rotation point in pixels, then Z, Y, X radians.
    fn post_render(&mut self, point: [f64; 3], angles: [f64; 3]) -> &mut Self {
        self.translatef(point[0] / 16.0, point[1] / 16.0, point[2] / 16.0)
            .rotatef(angles[2].to_degrees(), 0.0, 0.0, 1.0)
            .rotatef(angles[1].to_degrees(), 0.0, 1.0, 0.0)
            .rotatef(angles[0].to_degrees(), 1.0, 0.0, 0.0)
    }
}

/// `renderItemInFirstPerson`'s held-item branch up to `renderItem`, in call order.
fn java_first_person(swing: f64, equip: f64, using: Option<JavaUse>, rotate_around: bool) -> Gl {
    let mut gl = Gl::new();
    match using {
        Some(JavaUse::Consume {
            remaining,
            duration,
        }) => {
            let (t, max) = (f64::from(remaining), f64::from(duration));
            let r = 1.0 - t / max;
            let k = 1.0 - (1.0 - r).powi(27);
            gl.translatef(
                0.0,
                (table_cos(t / 4.0 * PI64) * 0.1).abs() * if r > 0.2 { 1.0 } else { 0.0 },
                0.0,
            );
            gl.translatef(k * 0.6, -k * 0.5, 0.0);
            gl.rotatef(k * 90.0, 0.0, 1.0, 0.0);
            gl.rotatef(k * 10.0, 1.0, 0.0, 0.0);
            gl.rotatef(k * 30.0, 0.0, 0.0, 1.0);
        }
        Some(_) => {}
        None => {
            gl.translatef(
                -table_sin(swing.sqrt() * PI64) * 0.4,
                table_sin(swing.sqrt() * PI64 * 2.0) * 0.2,
                -table_sin(swing * PI64) * 0.2,
            );
        }
    }
    gl.translatef(0.7 * 0.8, -0.65 * 0.8 - (1.0 - equip) * 0.6, -0.9 * 0.8);
    gl.rotatef(45.0, 0.0, 1.0, 0.0);
    gl.rotatef(-table_sin(swing * swing * PI64) * 20.0, 0.0, 1.0, 0.0);
    gl.rotatef(-table_sin(swing.sqrt() * PI64) * 20.0, 0.0, 0.0, 1.0);
    gl.rotatef(-table_sin(swing.sqrt() * PI64) * 80.0, 1.0, 0.0, 0.0);
    gl.scalef(0.4, 0.4, 0.4);
    match using {
        Some(JavaUse::Block) => {
            gl.translatef(-0.5, 0.2, 0.0)
                .rotatef(30.0, 0.0, 1.0, 0.0)
                .rotatef(-80.0, 1.0, 0.0, 0.0)
                .rotatef(60.0, 0.0, 1.0, 0.0);
        }
        Some(JavaUse::Bow { pull }) => {
            let pull = f64::from(pull);
            gl.rotatef(-18.0, 0.0, 0.0, 1.0)
                .rotatef(-12.0, 0.0, 1.0, 0.0)
                .rotatef(-8.0, 1.0, 0.0, 0.0)
                .translatef(-0.9, 0.2, 0.0);
            let d = (pull / 20.0).mul_add(pull / 20.0, pull / 20.0 * 2.0) / 3.0;
            let d = d.min(1.0);
            if d > 0.1 {
                gl.translatef(0.0, table_sin((pull - 0.1) * 1.3) * 0.01 * (d - 0.1), 0.0);
            }
            gl.translatef(0.0, 0.0, d * 0.1)
                .rotatef(-335.0, 0.0, 0.0, 1.0)
                .rotatef(-50.0, 0.0, 1.0, 0.0)
                .translatef(0.0, 0.5, 0.0)
                .scalef(1.0, 1.0, 1.0 + d * 0.2)
                .translatef(0.0, -0.5, 0.0)
                .rotatef(50.0, 0.0, 1.0, 0.0)
                .rotatef(335.0, 0.0, 0.0, 1.0);
        }
        _ => {}
    }
    if rotate_around {
        gl.rotatef(180.0, 0.0, 1.0, 0.0);
    }
    gl
}

/// `renderItem`'s flat-item transform onto Java's own unit slab.
fn java_flat_item(gl: &mut Gl) {
    gl.translatef(0.0, -0.3, 0.0)
        .scalef(1.5, 1.5, 1.5)
        .rotatef(50.0, 0.0, 1.0, 0.0)
        .rotatef(335.0, 0.0, 0.0, 1.0)
        .translatef(-0.9375, -0.0625, 0.0);
}

fn assert_mat(actual: Mat4, expected: DMat4, epsilon: f64, context: &str) {
    let actual = actual.as_dmat4();
    for (column, (a, e)) in actual
        .to_cols_array()
        .iter()
        .zip(expected.to_cols_array())
        .enumerate()
    {
        assert!(
            (a - e).abs() <= epsilon,
            "{context}: element {column} is {a}, Java gives {e}"
        );
    }
}

/// Our held slab's front-face points and the Java slab point carrying the same texel.
fn sprite_front_points() -> Vec<(Vec3, DVec3)> {
    let rgba = vec![255; 16 * 16 * 4];
    crate::held_sprite_vertices(16, 16, &rgba, [0.0, 0.0, 1.0, 1.0])
        .unwrap()
        .into_iter()
        .filter(|vertex| vertex.normal[2] > 0.5)
        .map(|vertex| {
            let [u, v] = vertex.uv;
            (
                Vec3::from_array(vertex.position),
                DVec3::new(1.0 - f64::from(u), 1.0 - f64::from(v), 0.0),
            )
        })
        .collect()
}

#[test]
fn table_trig_snaps_to_the_java_sine_table() {
    for radians in [0.0, 0.1, 1.0, -1.3, 2.5, 7.0, -40.0] {
        assert_eq!(
            java_sin(radians) as f64,
            table_sin(f64::from(radians)) as f32 as f64
        );
        assert_eq!(
            java_cos(radians) as f64,
            table_cos(f64::from(radians)) as f32 as f64
        );
    }
    assert_eq!(java_cos(0.0), 1.0);
    assert_eq!(java_sin(0.0), 0.0);
}

/// Matrix order: the composed first-person item stack equals Java's calls in order.
#[test]
fn first_person_item_stack_matches_java_at_sampled_progress() {
    let mut cases = Vec::new();
    for swing in [0.0, 0.25, 0.5, 1.0] {
        for equip in [1.0, 0.6, 0.0] {
            cases.push((swing, equip, None, false));
        }
        cases.push((swing, 1.0, Some(JavaUse::Block), false));
        cases.push((swing, 1.0, None, true));
    }
    for pull in [-1.0, 0.0, 1.5, 5.0, 12.0, 19.25, 40.0] {
        cases.push((0.0, 1.0, Some(JavaUse::Bow { pull }), false));
    }
    for remaining in [33.0, 30.5, 24.0, 12.25, 2.0, 1.0] {
        let using = Some(JavaUse::Consume {
            remaining,
            duration: 32.0,
        });
        cases.push((0.0, 1.0, using, false));
    }
    for (swing, equip, using, rotate_around) in cases {
        let hand = JavaHand {
            swing,
            equip,
            using,
        };
        let context = format!("{hand:?} rotate_around={rotate_around}");
        let gl = java_first_person(f64::from(swing), f64::from(equip), using, rotate_around);
        let block = first_person_item(hand, JavaItemMesh::Block, rotate_around);
        let mut cube = gl;
        cube.rotatef(90.0, 0.0, 1.0, 0.0);
        assert_mat(block, cube.0, 2e-5, &context);
        let sprite = first_person_item(hand, JavaItemMesh::Sprite, rotate_around);
        let mut flat = gl;
        java_flat_item(&mut flat);
        for (ours, java) in sprite_front_points() {
            let expected = flat.0.transform_point3(java);
            let actual = sprite.transform_point3(ours).as_dvec3();
            assert!(
                actual.distance(expected) < 2e-5,
                "{context}: texel at {java} lands at {actual}, Java {expected}"
            );
        }
    }
}

/// The Java pixel point of the arm box, through `ModelRenderer` in Java model space.
fn java_arm_point(gl: Gl, arm: JavaPart, corner: DVec3) -> DVec3 {
    let mut gl = gl;
    gl.post_render(
        arm.point.as_dvec3().to_array(),
        arm.angles.as_dvec3().to_array(),
    );
    gl.0.transform_point3(corner / 16.0)
}

/// Our rig vertex for a Java arm-box corner, posed by the arm bone, in rig blocks.
fn rig_arm_point(arm: JavaPart, rig_pivot: Vec3, corner: DVec3) -> Vec3 {
    let java_model = (JavaBiped::REST.right_arm.point.as_dvec3() + corner) / 16.0;
    let rest_vertex = rig_from_java_model().transform_point3(java_model.as_vec3());
    let (rotation, translation) = arm.rig_bone(JavaBiped::REST.right_arm, rig_pivot);
    rotation * (rest_vertex - rig_pivot / 16.0) + translation / 16.0
}

/// Projects camera space at 70 degrees vertical FOV onto an 854x480 screen.
fn screen(view: DVec3) -> [f64; 2] {
    let focal = 1.0 / (70.0_f64.to_radians() * 0.5).tan();
    let aspect = 854.0 / 480.0;
    [
        (focal / aspect * view.x / -view.z + 1.0) * 0.5 * 854.0,
        (1.0 - focal * view.y / -view.z) * 0.5 * 480.0,
    ]
}

fn assert_screen(actual: DVec3, expected: DVec3, context: &str) {
    let (a, e) = (screen(actual), screen(expected));
    assert!(
        (a[0] - e[0]).abs() < 0.01 && (a[1] - e[1]).abs() < 0.01,
        "{context}: on screen at {a:?}, Java {e:?}"
    );
}

/// The empty hand's box corners land on the pixels Java draws them at, through our rig.
#[test]
fn empty_hand_corners_project_where_java_draws_them() {
    let rig_pivot = Vec3::new(5.0, 22.0, 0.0);
    let arm = java_biped(&JavaBipedInput::default()).right_arm;
    assert_eq!(arm.point, Vec3::new(-5.0, 2.0, 0.0));
    assert!((arm.angles.z - 0.1).abs() < 1e-7);
    for (swing, equip) in [(0.0, 1.0), (0.25, 1.0), (0.5, 0.4), (1.0, 0.0)] {
        let mut gl = Gl::new();
        let s = f64::from(swing);
        gl.translatef(
            -table_sin(s.sqrt() * PI64) * 0.3,
            table_sin(s.sqrt() * PI64 * 2.0) * 0.4,
            -table_sin(s * PI64) * 0.4,
        )
        .translatef(
            0.8 * 0.8,
            -0.75 * 0.8 - (1.0 - f64::from(equip)) * 0.6,
            -0.9 * 0.8,
        )
        .rotatef(45.0, 0.0, 1.0, 0.0)
        .rotatef(table_sin(s.sqrt() * PI64) * 70.0, 0.0, 1.0, 0.0)
        .rotatef(-table_sin(s * s * PI64) * 20.0, 0.0, 0.0, 1.0)
        .translatef(-1.0, 3.6, 3.5)
        .rotatef(120.0, 0.0, 0.0, 1.0)
        .rotatef(200.0, 1.0, 0.0, 0.0)
        .rotatef(-135.0, 0.0, 1.0, 0.0)
        .translatef(5.6, 0.0, 0.0);
        let camera_from_rig = first_person_arm(swing, equip);
        for corner in [
            DVec3::new(-3.0, -2.0, -2.0),
            DVec3::new(1.0, 10.0, 2.0),
            DVec3::new(-3.0, 10.0, 2.0),
            DVec3::new(1.0, 10.0, -2.0),
        ] {
            let java = java_arm_point(gl, arm, corner);
            let ours = camera_from_rig
                .transform_point3(rig_arm_point(arm, rig_pivot, corner))
                .as_dvec3();
            assert_screen(ours, java, &format!("swing {swing} corner {corner}"));
        }
    }
}

/// A slim-armed rig keeps its own pivot and still moves by Java's rotation-point offsets.
#[test]
fn rig_bone_offsets_the_rig_pivot_by_the_java_point_offset() {
    let mut arm = JavaBiped::REST.right_arm;
    arm.point += Vec3::new(1.0, 2.0, 3.0);
    let (_, translation) = arm.rig_bone(JavaBiped::REST.right_arm, Vec3::new(5.0, 21.5, 0.0));
    assert_eq!(translation, Vec3::new(4.0, 19.5, 3.0));
}

/// Third-person grips: Java's world point of a held texel matches ours through the arm bone.
#[test]
fn third_person_items_land_where_java_draws_them() {
    let rig_pivot = Vec3::new(5.0, 22.0, 0.0);
    let (yaw, position) = (37.0_f64, DVec3::new(3.0, 64.0, -2.0));
    for swing in [0.0, 0.3, 0.8] {
        let arm = java_biped(&JavaBipedInput {
            swing,
            held_right: 1,
            age: 13.0,
            ..Default::default()
        })
        .right_arm;
        let mut world = Gl::new();
        world
            .translatef(position.x, position.y, position.z)
            .rotatef(180.0 - yaw, 0.0, 1.0, 0.0)
            .scalef(-1.0, -1.0, 1.0)
            .scalef(0.9375, 0.9375, 0.9375)
            .translatef(0.0, -24.0 * 0.0625 - 0.0078125, 0.0)
            .post_render(
                arm.point.as_dvec3().to_array(),
                arm.angles.as_dvec3().to_array(),
            )
            .translatef(-0.0625, 0.4375, 0.0625);
        let (rotation, translation) = arm.rig_bone(JavaBiped::REST.right_arm, rig_pivot);
        let ours_world = DMat4::from_translation(position)
            * DMat4::from_rotation_y((180.0 - yaw).to_radians())
            * DMat4::from_scale(DVec3::splat(0.9375))
            * DMat4::from_translation(DVec3::Y / 128.0)
            * Mat4::from_rotation_translation(rotation, translation / 16.0).as_dmat4();
        for (item, grip) in [
            (JavaHeldItem::Flat, None),
            (JavaHeldItem::Bow, Some(0)),
            (
                JavaHeldItem::Tool {
                    rotate_around: false,
                    blocking: false,
                },
                Some(1),
            ),
            (
                JavaHeldItem::Tool {
                    rotate_around: true,
                    blocking: true,
                },
                Some(2),
            ),
        ] {
            let mut gl = world;
            match grip {
                None => {
                    gl.translatef(0.25, 0.1875, -0.1875)
                        .scalef(0.375, 0.375, 0.375)
                        .rotatef(60.0, 0.0, 0.0, 1.0)
                        .rotatef(-90.0, 1.0, 0.0, 0.0)
                        .rotatef(20.0, 0.0, 0.0, 1.0);
                }
                Some(0) => {
                    gl.translatef(0.0, 0.125, 0.3125)
                        .rotatef(-20.0, 0.0, 1.0, 0.0)
                        .scalef(0.625, -0.625, 0.625)
                        .rotatef(-100.0, 1.0, 0.0, 0.0)
                        .rotatef(45.0, 0.0, 1.0, 0.0);
                }
                Some(kind) => {
                    if kind == 2 {
                        gl.rotatef(180.0, 0.0, 0.0, 1.0)
                            .translatef(0.0, -0.125, 0.0)
                            .translatef(0.05, 0.0, -0.1)
                            .rotatef(-50.0, 0.0, 1.0, 0.0)
                            .rotatef(-10.0, 1.0, 0.0, 0.0)
                            .rotatef(-60.0, 0.0, 0.0, 1.0);
                    }
                    gl.translatef(0.0, 0.1875, 0.0)
                        .scalef(0.625, -0.625, 0.625)
                        .rotatef(-100.0, 1.0, 0.0, 0.0)
                        .rotatef(45.0, 0.0, 1.0, 0.0);
                }
            }
            java_flat_item(&mut gl);
            let placement = third_person_item(item, JavaItemMesh::Sprite);
            for (ours, java) in sprite_front_points() {
                let expected = gl.0.transform_point3(java);
                let local = placement.transform_point3(ours);
                let actual = ours_world.transform_point3(local.as_dvec3());
                assert!(
                    actual.distance(expected) < 1e-5,
                    "{item:?} swing {swing}: lands at {actual}, Java {expected}"
                );
            }
        }
        let mut gl = world;
        gl.translatef(0.0, 0.1875, -0.3125)
            .rotatef(20.0, 1.0, 0.0, 0.0)
            .rotatef(45.0, 0.0, 1.0, 0.0)
            .scalef(-0.375, -0.375, 0.375)
            .rotatef(90.0, 0.0, 1.0, 0.0);
        let block =
            ours_world * third_person_item(JavaHeldItem::Block, JavaItemMesh::Block).as_dmat4();
        for corner in [DVec3::splat(0.5), DVec3::new(-0.5, 0.5, -0.5)] {
            assert!(
                block
                    .transform_point3(corner)
                    .distance(gl.0.transform_point3(corner))
                    < 1e-5
            );
        }
    }
}

fn degrees(radians: f32) -> f64 {
    f64::from(radians).to_degrees()
}

/// Spot values of the pose formulas, computed with exact trig.
#[test]
fn biped_pose_follows_java_formulas() {
    let input = JavaBipedInput {
        limb_swing: 3.0,
        limb_amount: 0.8,
        age: 40.0,
        head_yaw: 20.0,
        head_pitch: -10.0,
        swing: 0.25,
        held_right: 1,
        ..Default::default()
    };
    let pose = java_biped(&input);
    let body_yaw = (0.25_f64.sqrt() * PI64 * 2.0).sin() * 0.2;
    assert!((f64::from(pose.body.angles.y) - body_yaw).abs() < 2e-4);
    assert!((f64::from(pose.right_arm.point.x) + body_yaw.cos() * 5.0).abs() < 2e-4);
    assert!((f64::from(pose.right_arm.point.z) - body_yaw.sin() * 5.0).abs() < 2e-4);
    let swing_x = (3.0 * 0.6662 + PI64).cos() * 0.8;
    let head_x = (-10.0_f64).to_radians();
    let eased = 1.0 - 0.75_f64.powi(4);
    let reach = (0.25 * PI64).sin() * -(head_x - 0.7) * 0.75;
    let right_x = swing_x * 0.5 - PI64 / 10.0 - ((eased * PI64).sin() * 1.2 + reach)
        + (40.0 * 0.067_f64).sin() * 0.05;
    assert!((f64::from(pose.right_arm.angles.x) - right_x).abs() < 5e-4);
    let right_z = (0.25 * PI64).sin() * -0.4 + (40.0 * 0.09_f64).cos() * 0.05 + 0.05;
    assert!((f64::from(pose.right_arm.angles.z) - right_z).abs() < 2e-4);
    assert!((f64::from(pose.right_arm.angles.y) - body_yaw * 3.0).abs() < 2e-4);
    assert!((degrees(pose.head.angles.y) - 20.0).abs() < 1e-4);
    let left_leg = (3.0 * 0.6662 + PI64).cos() * 1.4 * 0.8;
    assert!((f64::from(pose.left_leg.angles.x) - left_leg).abs() < 2e-4);
    assert_eq!(pose.right_leg.point, Vec3::new(-1.9, 12.0, 0.1));
}

#[test]
fn sneaking_riding_blocking_and_aiming_set_their_java_poses() {
    let sneak = java_biped(&JavaBipedInput {
        sneaking: true,
        ..Default::default()
    });
    assert_eq!(sneak.body.angles.x, 0.5);
    assert_eq!(sneak.right_leg.point, Vec3::new(-1.9, 9.0, 4.0));
    assert_eq!(sneak.head.point, Vec3::new(0.0, 1.0, 0.0));
    assert!((sneak.left_arm.angles.x - 0.4).abs() < 1e-6);
    let ride = java_biped(&JavaBipedInput {
        riding: true,
        ..Default::default()
    });
    assert!((degrees(ride.right_leg.angles.x) + 72.0).abs() < 1e-4);
    assert!((degrees(ride.left_leg.angles.y) + 18.0).abs() < 1e-4);
    assert!((degrees(ride.left_arm.angles.x) + 36.0).abs() < 1e-4);
    let block = java_biped(&JavaBipedInput {
        held_right: 3,
        ..Default::default()
    });
    assert!((degrees(block.right_arm.angles.x) + 54.0).abs() < 1e-3);
    let aim = java_biped(&JavaBipedInput {
        aimed_bow: true,
        head_yaw: 10.0,
        head_pitch: 30.0,
        held_right: 1,
        ..Default::default()
    });
    assert!((degrees(aim.right_arm.angles.x) - (-90.0 + 30.0)).abs() < 1e-3);
    assert!((f64::from(aim.left_arm.angles.y) - (0.5 + 10.0_f64.to_radians())).abs() < 1e-5);
    assert!((f64::from(aim.right_arm.angles.y) - (-0.1 + 10.0_f64.to_radians())).abs() < 1e-5);
}

#[test]
fn java_item_classes_follow_their_java_items() {
    for tool in [
        "minecraft:iron_sword",
        "minecraft:stone_pickaxe",
        "minecraft:stick",
    ] {
        assert!(is_java_tool(tool));
    }
    for flat in ["minecraft:blaze_rod", "minecraft:mace", "minecraft:apple"] {
        assert!(!is_java_tool(flat));
    }
    assert!(is_java_tool("minecraft:fishing_rod"));
    assert!(is_java_sword("minecraft:diamond_sword") && !is_java_sword("minecraft:bow"));
}

/// A raster texel centre lands on the Java slab point that samples the same texel.
#[test]
fn raster_texels_land_on_their_java_slab_texels() {
    let hand = JavaHand {
        swing: 0.3,
        equip: 0.9,
        using: Some(JavaUse::Bow { pull: 7.5 }),
    };
    let raster = first_person_item(
        hand,
        JavaItemMesh::Raster {
            width: 16,
            height: 16,
        },
        false,
    );
    let mut flat = java_first_person(0.3, f64::from(0.9_f32), hand.using, false);
    java_flat_item(&mut flat);
    for (column, row) in [(0.5, 0.5), (3.5, 12.5), (15.5, 7.5)] {
        let java = DVec3::new(1.0 - column / 16.0, 1.0 - row / 16.0, 0.0);
        let ours = raster
            .transform_point3(Vec3::new(column as f32, 0.0, row as f32))
            .as_dvec3();
        let expected = flat.0.transform_point3(java);
        assert!(ours.distance(expected) < 2e-5, "{ours} vs {expected}");
    }
}

/// Cape formula at sampled chase offsets: standing, walking away, strafing, falling, sneaking.
#[test]
fn cape_angles_follow_java_formulas() {
    let angles = |chase: [f32; 3], body_yaw: f32, bob: f32, walked: f32, sneaking: bool| {
        java_cape_angles(&JavaCapeInput {
            chase: Vec3::from_array(chase),
            body_yaw,
            bob,
            walked,
            sneaking,
        })
    };
    let java = |chase: [f64; 3], yaw: f64, bob: f64, walked: f64, sneaking: bool| {
        let (s, c) = (
            table_sin(yaw * PI64 / 180.0),
            -table_cos(yaw * PI64 / 180.0),
        );
        let mut lift = (chase[1] * 10.0).clamp(-6.0, 32.0);
        let back = ((chase[0] * s + chase[2] * c) * 100.0).max(0.0);
        let side = (chase[0] * c - chase[2] * s) * 100.0;
        lift += table_sin(walked * 6.0) * 32.0 * bob + if sneaking { 25.0 } else { 0.0 };
        (6.0 + back / 2.0 + lift, side / 2.0)
    };
    let cases = [
        ([0.0, 0.0, 0.0], 0.0, 0.0, 0.0, false),
        ([0.0, 0.0, -0.4], 0.0, 0.1, 3.7, false),
        ([0.3, 0.0, 0.0], 0.0, 0.08, 1.2, false),
        ([0.0, 1.2, 0.0], 45.0, 0.0, 0.0, false),
        ([0.0, -2.0, 0.1], 90.0, 0.0, 0.0, true),
        ([0.1, 0.0, 0.5], 200.0, 0.02, 9.0, false),
    ];
    for (chase, yaw, bob, walked, sneaking) in cases {
        let (tilt, side) = angles(chase, yaw, bob, walked, sneaking);
        let (java_tilt, java_side) = java(
            chase.map(f64::from),
            f64::from(yaw),
            f64::from(bob),
            f64::from(walked),
            sneaking,
        );
        assert!(
            (f64::from(tilt) - java_tilt).abs() < 1e-3,
            "{chase:?}: {tilt} vs {java_tilt}"
        );
        assert!(
            (f64::from(side) - java_side).abs() < 1e-3,
            "{chase:?}: {side} vs {java_side}"
        );
    }
    assert_eq!(angles([0.0; 3], 0.0, 0.0, 0.0, false), (6.0, 0.0));
}

/// The cape corners land where Java's cape stack draws its cloak box, through our bind pivot.
#[test]
fn cape_corners_land_where_java_draws_them() {
    let input = JavaCapeInput {
        chase: Vec3::new(0.2, 0.3, -0.5),
        body_yaw: 30.0,
        bob: 0.06,
        walked: 2.5,
        sneaking: true,
    };
    let (tilt, side) = java_cape_angles(&input);
    let pivot = Vec3::new(0.0, 24.0, 3.0) / 16.0;
    let (rotation, translation) = java_cape_bone(&input, pivot);
    let mut posed = Gl::new();
    posed
        .translatef(0.0, 0.0, 0.125)
        .rotatef(f64::from(tilt), 1.0, 0.0, 0.0)
        .rotatef(f64::from(side), 0.0, 0.0, 1.0)
        .rotatef(-f64::from(side), 0.0, 1.0, 0.0)
        .rotatef(180.0, 0.0, 1.0, 0.0);
    let mut rest = Gl::new();
    rest.translatef(0.0, 0.0, 0.125)
        .rotatef(180.0, 0.0, 1.0, 0.0);
    let rig = rig_from_java_model().as_dmat4();
    for corner in [DVec3::new(-5.0, 0.0, -1.0), DVec3::new(5.0, 16.0, 0.0)] {
        let java = rig.transform_point3(posed.0.transform_point3(corner / 16.0));
        let bind = rig
            .transform_point3(rest.0.transform_point3(corner / 16.0))
            .as_vec3();
        let ours = (rotation * (bind - pivot) + translation).as_dvec3();
        assert!(ours.distance(java) < 1e-5, "{corner}: {ours} vs {java}");
    }
}
