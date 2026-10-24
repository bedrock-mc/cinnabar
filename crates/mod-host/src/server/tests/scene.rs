use super::*;
use server_experience::runtime::SceneObject;

#[test]
fn legacy_only_snapshots_keep_their_objects_while_new_callbacks_change_the_scene() {
    let mut capabilities = screen_capabilities(&[Permission::Ui, Permission::Scene]);
    capabilities.assets.insert("legacy".into());
    let mut host = BundleHost::launch(
        include_bytes!("../guest_1_0.wat"),
        owner(),
        capabilities,
        1,
        Default::default(),
        drawing_context(1),
    )
    .unwrap();
    host.take_transaction();
    let object = |x| SceneObject::Quad {
        texture: "legacy".into(),
        transform: [x, 0., 0., 0., 0., 0., 1., 1., 1., 1.],
        size: [1.; 2],
    };
    let state = host.store.data_mut();
    state.commands.push(Command::Scene {
        id: 1,
        object: Some(object(1.)),
    });
    state.commit_scene().unwrap();
    assert!(host.take_transaction().commands.is_empty());
    let first = host.take_presentation(1).unwrap().unwrap();
    assert!((first.bytes.len() as u32) < first.total);
    let mut bytes = first.bytes;
    let state = host.store.data_mut();
    state.commands = vec![Command::Scene {
        id: 1,
        object: Some(object(2.)),
    }];
    state.commit_scene().unwrap();
    assert!(host.take_transaction().commands.is_empty());
    while bytes.len() < first.total as usize {
        let part = host.take_presentation(4096).unwrap().unwrap();
        assert_eq!(part.revision, first.revision);
        bytes.extend(part.bytes);
    }
    let older: crate::presentation::Output = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(older.scene[&1], object(1.));
    let next = host.take_presentation(4096).unwrap().unwrap();
    assert_ne!(next.revision, first.revision);
    assert_eq!(next.bytes.len(), next.total as usize);
    let newer: crate::presentation::Output = serde_json::from_slice(&next.bytes).unwrap();
    assert_eq!(newer.scene[&1], object(2.));
    assert!(host.take_presentation(4096).unwrap().is_none());
    host.store.data_mut().commit_scene().unwrap();
    assert!(host.take_presentation(4096).unwrap().is_none());
    host.dispatch(&Event::Epoch, 2, &drawing_context(2))
        .unwrap()
        .into_transaction();
    let rebound = host.take_presentation(4096).unwrap().unwrap();
    let output: crate::presentation::Output = serde_json::from_slice(&rebound.bytes).unwrap();
    assert!(output.scene.is_empty());
}
