use super::super::LocalWorlds;
use super::*;
use launcher::local_worlds::world::NewWorld;

/// Supplies a connected request queue whose worker has not consumed any work yet.
#[allow(
    clippy::field_reassign_with_default,
    reason = "LocalWorlds implements Drop, so struct update cannot move its fields"
)]
fn queued_worlds(
    manager: Manager,
    socket_dir: PathBuf,
) -> (LocalWorlds, Receiver<Request>, Sender<Reply>) {
    let core = CoreLink::spawn(manager.clone(), socket_dir).unwrap();
    let (effects, requests) = unbounded();
    let (replies, events) = unbounded();
    let client = WorldsClient {
        effects,
        events,
        revoked: Arc::new(Mutex::new(false)),
        _core: Some(core),
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        worker: None,
    };
    let mut worlds = LocalWorlds::default();
    worlds.manager = Some(manager);
    worlds.client = Some(client);
    (worlds, requests, replies)
}

#[test]
fn saving_a_thumbnail_queues_disk_work_and_publishes_pixels_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let manager = launcher_host::local_worlds::open_store(dir.path().to_path_buf()).unwrap();
    let world = manager
        .create(&NewWorld {
            name: "Preview".into(),
            ..Default::default()
        })
        .unwrap();
    manager.save_thumbnail(&world.id, b"old view").unwrap();
    let (mut worlds, requests, _replies) =
        queued_worlds(manager.clone(), dir.path().join("no-core"));
    worlds.save_thumbnail(&world.id, b"new view".to_vec());
    assert_eq!(&*worlds.thumbnail(&world.id).unwrap().image, b"new view");
    assert_eq!(manager.thumbnail(&world.id).unwrap(), b"old view");
    let Request::SaveThumbnail { id, image } = requests.try_recv().unwrap() else {
        panic!("queued thumbnail write")
    };
    assert_eq!(id, world.id);
    assert_eq!(&*image, b"new view");
}

#[test]
fn pump_uses_worker_pixels_and_keeps_a_newer_capture_over_an_older_list_reply() {
    let dir = tempfile::tempdir().unwrap();
    let manager = launcher_host::local_worlds::open_store(dir.path().to_path_buf()).unwrap();
    let world = manager
        .create(&NewWorld {
            name: "Preview".into(),
            ..Default::default()
        })
        .unwrap();
    manager.save_thumbnail(&world.id, b"disk view").unwrap();
    let (mut worlds, _requests, replies) = queued_worlds(manager, dir.path().join("no-core"));
    replies
        .send(Reply {
            event: Event::Listed(vec![world.clone()]),
            thumbnails: vec![(
                world.id.clone(),
                WorldThumbnail::new(&world.id, Arc::from(&b"worker view"[..])),
            )],
        })
        .unwrap();
    worlds.pump();
    assert_eq!(&*worlds.thumbnail(&world.id).unwrap().image, b"worker view");
    worlds.save_thumbnail(&world.id, b"latest view".to_vec());
    replies
        .send(Reply {
            event: Event::Listed(vec![world.clone()]),
            thumbnails: vec![(
                world.id.clone(),
                WorldThumbnail::new(&world.id, Arc::from(&b"older view"[..])),
            )],
        })
        .unwrap();
    worlds.pump();
    assert_eq!(&*worlds.thumbnail(&world.id).unwrap().image, b"latest view");
    replies
        .send(Reply {
            event: Event::Deleted(world.id.clone()),
            thumbnails: Vec::new(),
        })
        .unwrap();
    worlds.pump();
    assert!(worlds.thumbnail(&world.id).is_none());
}

#[test]
fn worker_reads_and_persists_thumbnail_bytes_in_request_order() {
    let dir = tempfile::tempdir().unwrap();
    let manager = launcher_host::local_worlds::open_store(dir.path().to_path_buf()).unwrap();
    let world = manager
        .create(&NewWorld {
            name: "Preview".into(),
            ..Default::default()
        })
        .unwrap();
    let client = WorldsClient::spawn(manager.clone(), dir.path().join("no-core")).unwrap();
    client.save_thumbnail(world.id.clone(), Arc::from(&b"saved view"[..]));
    client.send(Effect::List);
    let mut replies = Vec::new();
    test_time::eventually("world worker thumbnail reply", || {
        replies.extend(client.drain());
        !replies.is_empty()
    });
    assert!(matches!(&replies[0].event, Event::Listed(list) if list[0].id == world.id));
    assert_eq!(&*replies[0].thumbnails[0].1.image, b"saved view");
    assert_eq!(manager.thumbnail(&world.id).unwrap(), b"saved view");
}
