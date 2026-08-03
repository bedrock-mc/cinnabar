#[test]
fn bedrock_yaw_and_pitch_map_to_bevys_negative_z_camera() {
    let south = bedrock_camera_rotation(0.0, 0.0) * Vec3::NEG_Z;
    let west = bedrock_camera_rotation(90.0, 0.0) * Vec3::NEG_Z;
    let looking_down = bedrock_camera_rotation(180.0, 45.0) * Vec3::NEG_Z;

    assert!(south.abs_diff_eq(Vec3::Z, 0.0001));
    assert!(west.abs_diff_eq(Vec3::NEG_X, 0.0001));
    assert!(looking_down.y < -0.7);
}