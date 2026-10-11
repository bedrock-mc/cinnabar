use super::*;

fn skeleton(specs: &[(&str, Option<usize>, bool)]) -> render::LayerSkeleton {
    let mut bones: Vec<_> = specs
        .iter()
        .map(|(_, parent, binding)| RuntimeBone {
            parent: *parent,
            has_binding_expression: *binding,
            ..Default::default()
        })
        .collect();
    let names: Vec<Box<str>> = specs.iter().map(|(name, _, _)| Box::from(*name)).collect();
    bind_roots(&mut bones, &names, &["rightitem".into(), "head".into()]);
    render::LayerSkeleton { bones, names }
}

#[test]
fn selected_geometry_descendants_inherit_their_own_root_parent_frame() {
    let primary = skeleton(&[
        ("unbound", None, false),
        ("child", Some(0), false),
        ("rightitem", None, false),
        ("held_child", Some(2), false),
        ("shield", None, true),
        ("shield_child", Some(4), false),
    ]);
    let layers = BTreeMap::from([(
        1,
        Some(Arc::new(skeleton(&[
            ("head", None, false),
            ("ornament", Some(0), false),
        ]))),
    )]);
    let snapshot = AttachableRigSnapshot {
        geometry: 0,
        pose: &[],
        bone_names: &primary.names,
        render: &[],
        scale: 1.0,
        axis_scale: [1.0; 3],
        bones: &primary.bones,
        layer_skeletons: &layers,
    };
    for index in 0..2 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::Actor)
        );
        assert_eq!(
            snapshot.bone_parent(1, index),
            Some(AttachableBoneParent::OwnerNamed("head")),
        );
    }
    for index in 2..4 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::OwnerNamed("rightitem")),
        );
    }
    for index in 4..6 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::BindingExpression),
        );
    }
    assert_eq!(snapshot.bone_parent(0, 6), None);
    assert_eq!(snapshot.bone_parent(2, 0), None);
}
