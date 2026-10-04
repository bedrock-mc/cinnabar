//! Exact authored leg relationships for the two pinned vanilla walking clips.
//! Numeric tolerances cover f32 interpolation/rotation rounding, not a parity approximation.
use super::*;

pub(super) fn verify_source(pack: &Path) {
    let read = |path: &str| {
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(pack.join(path)).unwrap())
            .unwrap()
    };
    let quadruped = read("animations/quadruped.animation.json");
    let chicken = read("animations/chicken.animation.json");
    let quadruped = &quadruped["animations"]["animation.quadruped.walk"];
    let chicken = &chicken["animations"]["animation.chicken.move"];
    assert!(
        quadruped["anim_time_update"].is_string(),
        "authored quadruped clock"
    );
    assert_eq!(quadruped["anim_time_update"], chicken["anim_time_update"]);
    for name in ["leg0", "leg1"] {
        assert_eq!(quadruped["bones"][name], chicken["bones"][name]);
    }
    assert_eq!(quadruped["bones"]["leg0"], quadruped["bones"]["leg3"]);
    assert_eq!(quadruped["bones"]["leg1"], quadruped["bones"]["leg2"]);
}

pub(super) fn verify(world: &WorldStream) -> [Vec<BoneTransform>; 2] {
    let poses = [
        super::selected_bones(world, MOB_IDS[0], "leg"),
        super::selected_bones(world, MOB_IDS[1], "leg"),
    ];
    let rotation = |species: usize, name: &str| {
        let rig = world.actor_rig(MOB_IDS[species]).unwrap();
        let bone = rig
            .bone_names
            .iter()
            .position(|bone| bone.as_ref() == name)
            .unwrap_or_else(|| panic!("{} is missing authored bone {name}", SPECIES[species]));
        Quat::from_array(rig.rest[bone].rotation).inverse()
            * Quat::from_array(rig.current[bone].rotation)
    };
    let cow = rotation(0, "leg0");
    assert!(
        same(cow, rotation(0, "leg3")),
        "cow legs 0 and 3 must share authored phase"
    );
    assert!(
        same(cow.inverse(), rotation(0, "leg1")),
        "cow legs 0 and 1 must oppose"
    );
    assert!(
        same(rotation(0, "leg1"), rotation(0, "leg2")),
        "cow legs 1 and 2 must share authored phase"
    );
    let chicken = rotation(1, "leg0");
    assert!(
        same(chicken.inverse(), rotation(1, "leg1")),
        "chicken legs must oppose"
    );
    assert!(
        same(cow, chicken),
        "equal motion must give cow and chicken the same authored gait phase"
    );
    poses
}

pub(super) fn verify_stopped(world: &WorldStream, species: usize) {
    let rig = world.actor_rig(MOB_IDS[species]).unwrap();
    for (index, name) in rig
        .bone_names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.contains("leg"))
    {
        assert!(
            same(
                Quat::from_array(rig.rest[index].rotation),
                Quat::from_array(rig.current[index].rotation)
            ),
            "{} stopped bone {name} must return to its authored rest rotation",
            SPECIES[species]
        );
    }
}

fn same(a: Quat, b: Quat) -> bool {
    a.to_array()
        .into_iter()
        .zip(b.to_array())
        .all(|(a, b)| (a - b).abs() < 1.0e-5)
}
