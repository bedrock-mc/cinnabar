use far_terrain::{
    ColumnPos, DiskBounds, DiskCache, DiskEvent, NamespaceKey, RegionPos, ServerKey, Versioned,
    decode_region, encode_region, region_index,
};

use crate::summary::hilly;

fn namespace(destination: &str) -> NamespaceKey {
    NamespaceKey {
        server: ServerKey::from_destination(destination),
        dimension: 0,
        level: 7,
        epoch: 0,
        resources: [0; 32],
    }
}

#[test]
fn region_slots_cover_negative_columns() {
    let pos = ColumnPos::new(-1, -33);
    let region = RegionPos::of(pos);
    assert_eq!(region, RegionPos { x: -1, z: -2 });
    assert_eq!(region.column(region_index(pos)), pos);
    assert_eq!(RegionPos::parse(&region.file_name()), Some(region));
}

#[test]
fn region_codec_round_trips_and_rejects_damage() {
    let summary = Versioned {
        version: 1,
        summary: hilly().encode(),
    };
    let bytes = encode_region(&[(3, summary.clone()), (1000, summary.clone())]);
    assert_eq!(
        decode_region(&bytes).unwrap(),
        vec![(3, summary.clone()), (1000, summary)]
    );
    assert!(decode_region(&bytes[..bytes.len() - 1]).is_err());
    let mut extended = bytes.clone();
    extended.push(0);
    assert!(decode_region(&extended).is_err());
}

#[test]
fn server_identity_is_hashed_and_stable() {
    let key = ServerKey::from_destination("remote:play.example.net:19132");
    let text = key.to_string();
    assert_eq!(text.len(), 32);
    assert!(!text.contains("example"));
    assert_eq!(ServerKey::from_hex(&text), Some(key));
    assert_eq!(
        key,
        ServerKey::from_destination(" REMOTE:play.example.net:19132 ")
    );
    assert_ne!(
        key,
        ServerKey::from_destination("remote:play.example.net:19133")
    );
}

#[test]
fn namespace_directories_keep_resource_identity_and_read_the_legacy_layout() {
    let base = namespace("remote:a:1");
    assert_eq!(base.directory(), "0.0000000000000007.0");
    for resources in [[0; 32], [0xab; 32]] {
        let key = NamespaceKey { resources, ..base };
        assert_eq!(NamespaceKey::parse(key.server, &key.directory()), Some(key));
        assert_eq!(key.world().namespace(key.epoch), key);
    }
    for suffix in [
        "",
        "ab",
        &"AB".repeat(32),
        &"gg".repeat(32),
        &format!("{}.extra", "ab".repeat(32)),
    ] {
        assert!(
            NamespaceKey::parse(base.server, &format!("{}.{suffix}", base.directory())).is_none()
        );
    }
}

fn loaded(disk: &DiskCache) -> Vec<(ColumnPos, u64)> {
    disk.sync();
    disk.events()
        .flat_map(|event| match event {
            DiskEvent::Loaded { columns, .. } => columns,
            DiskEvent::Resolved { .. } | DiskEvent::Written { .. } => Vec::new(),
        })
        .map(|(pos, column)| (pos, column.summary.hash))
        .collect()
}

#[test]
fn written_regions_load_back_and_later_writes_merge() {
    let root = tempfile::tempdir().unwrap();
    let bounds = DiskBounds {
        server_cap_bytes: u64::MAX,
        global_cap_bytes: u64::MAX,
    };
    let disk = DiskCache::spawn(root.path().to_owned(), bounds).unwrap();
    let ns = namespace("remote:a:1");
    let summary = Versioned {
        version: 1,
        summary: hilly().encode(),
    };
    let region = RegionPos { x: 0, z: 0 };
    disk.write(ns, region, vec![(0, summary.clone())]);
    disk.write(ns, region, vec![(33, summary.clone())]);
    disk.load(ns, region);
    assert_eq!(
        loaded(&disk),
        vec![
            (ColumnPos::new(0, 0), summary.summary.hash),
            (ColumnPos::new(1, 1), summary.summary.hash)
        ]
    );
    let names: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names.contains(&ns.server.to_string()), "no address on disk");
}

/// Only one cache worker may merge and replace files under a shared root.
#[test]
fn a_shared_root_has_one_writer_until_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let bounds = DiskBounds {
        server_cap_bytes: u64::MAX,
        global_cap_bytes: u64::MAX,
    };
    let disk = DiskCache::spawn(root.path().into(), bounds).unwrap();
    assert!(DiskCache::spawn(root.path().into(), bounds).is_none());
    drop(disk);
    assert!(DiskCache::spawn(root.path().into(), bounds).is_some());
}

/// Damage to a version header, including an exhausted counter, is a cache miss.
#[test]
fn region_versions_are_integrity_checked() {
    let column = Versioned {
        version: 100,
        summary: hilly().encode(),
    };
    let encoded = encode_region(&[(0, column)]);
    for version in [101, u64::MAX - 1, u64::MAX] {
        let mut damaged = encoded.clone();
        damaged[24..32].copy_from_slice(&version.to_le_bytes());
        assert!(decode_region(&damaged).is_err());
    }
    for version in [u64::MAX - 1, u64::MAX] {
        assert!(
            decode_region(&encode_region(&[(
                0,
                Versioned {
                    version,
                    summary: hilly().encode()
                }
            )]))
            .is_err()
        );
    }
}

#[test]
fn caps_evict_least_recently_used_regions() {
    let root = tempfile::tempdir().unwrap();
    let summary = Versioned {
        version: 1,
        summary: hilly().encode(),
    };
    let region_bytes = encode_region(&[(0, summary.clone())]).len() as u64;
    let bounds = DiskBounds {
        server_cap_bytes: region_bytes * 2,
        global_cap_bytes: region_bytes * 3,
    };
    let disk = DiskCache::spawn(root.path().to_owned(), bounds).unwrap();
    let (a, b) = (namespace("remote:a:1"), namespace("remote:b:1"));
    let regions = [0, 1, 2].map(|x| RegionPos { x, z: 0 });
    disk.write(a, regions[0], vec![(0, summary.clone())]);
    disk.write(a, regions[1], vec![(0, summary.clone())]);
    disk.load(a, regions[0]);
    disk.write(a, regions[2], vec![(0, summary.clone())]);
    for region in regions {
        disk.load(a, region);
    }
    let survivors: Vec<i32> = loaded(&disk).iter().map(|(pos, _)| pos.x).collect();
    assert_eq!(
        survivors,
        vec![0, 0, 64],
        "region 1 was least recently used"
    );

    disk.write(b, regions[0], vec![(0, summary.clone())]);
    disk.write(b, regions[1], vec![(0, summary.clone())]);
    for region in regions {
        disk.load(a, region);
    }
    assert_eq!(
        loaded(&disk).len(),
        1,
        "the global cap evicted the oldest of server a"
    );
}

/// Of two writes to one slot, the higher version stays whatever order they land in.
#[test]
fn region_writes_keep_the_higher_version() {
    let root = tempfile::tempdir().unwrap();
    let bounds = DiskBounds {
        server_cap_bytes: u64::MAX,
        global_cap_bytes: u64::MAX,
    };
    let disk = DiskCache::spawn(root.path().to_owned(), bounds).unwrap();
    let ns = namespace("remote:a:1");
    let region = RegionPos { x: 0, z: 0 };
    let newer = Versioned {
        version: 9,
        summary: hilly().encode(),
    };
    let mut flat = hilly();
    flat.cells_mut()[0].height += 3;
    let older = Versioned {
        version: 4,
        summary: flat.encode(),
    };
    disk.write(ns, region, vec![(0, newer.clone())]);
    disk.write(ns, region, vec![(0, older)]);
    disk.load(ns, region);
    assert_eq!(
        loaded(&disk),
        vec![(ColumnPos::new(0, 0), newer.summary.hash)]
    );
}

/// Two worlds resolved before either writes get different epochs.
#[test]
fn resolutions_reserve_their_epochs() {
    let root = tempfile::tempdir().unwrap();
    let bounds = DiskBounds {
        server_cap_bytes: u64::MAX,
        global_cap_bytes: u64::MAX,
    };
    let disk = DiskCache::spawn(root.path().to_owned(), bounds).unwrap();
    let world = namespace("remote:a:1").world();
    let mut hilly_probe = hilly();
    hilly_probe.cells_mut()[0].height += 9;
    disk.resolve(
        world,
        1,
        far_terrain::ResolutionIntent::Probed,
        vec![(ColumnPos::new(0, 0), hilly())],
    );
    disk.resolve(
        world,
        2,
        far_terrain::ResolutionIntent::Probed,
        vec![(ColumnPos::new(0, 0), hilly_probe)],
    );
    disk.sync();
    let epochs: Vec<u32> = disk
        .events()
        .filter_map(|event| match event {
            DiskEvent::Resolved { epoch, .. } => Some(epoch),
            _ => None,
        })
        .collect();
    assert_eq!(epochs.len(), 2);
    assert_ne!(epochs[0], epochs[1]);
}
