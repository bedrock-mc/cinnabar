//! Complete content dependencies for subscribers that defer their reads.

use super::*;

#[test]
fn contents_only_dependencies_detect_changed_large_member_tails() {
    const MEMBER_BYTES: usize = 67_108_865;
    let path = "textures/complete.bin";
    let mut payload = vec![0x5a; MEMBER_BYTES];
    payload[MEMBER_BYTES - 8..].copy_from_slice(b"end-data");
    let before = crate::test_support::stored_stack(&[(path, &payload), ("unwatched.bin", b"old")]);
    assert_eq!(before.packs()[0].file_size(path), Some(MEMBER_BYTES as u64));
    assert_eq!(
        before.packs()[0].read_file(path).unwrap().unwrap().as_ref(),
        payload
    );
    let mut dependencies = Dependencies::default();
    compile(Subscriber::Ui, &before, &mut dependencies, |view| {
        view.track_contents("textures/");
    });
    assert_eq!(
        dependencies[&Subscriber::Ui],
        BTreeSet::from([PackDependency::Contents("textures/".into())])
    );
    let previous = Previous {
        admission: PackAdmission::Validated(before.clone()),
        dependencies,
    };
    {
        let unchanged =
            crate::test_support::stored_stack(&[(path, &payload), ("unwatched.bin", b"new")]);
        assert!(!before.same_contents(&unchanged));
        assert_eq!(
            unchanged.packs()[0]
                .read_file(path)
                .unwrap()
                .unwrap()
                .as_ref(),
            payload
        );
        assert!(
            !Changes::between(
                &unchanged,
                Some((&previous.admission, &previous.dependencies))
            )
            .ui,
            "unchanged complete content reuses the subscriber across an unrelated archive edit"
        );
    }
    payload[MEMBER_BYTES - 1] ^= 1;
    let changed = crate::test_support::stored_stack(&[(path, &payload), ("unwatched.bin", b"new")]);
    assert!(!before.same_contents(&changed));
    assert_eq!(
        changed.packs()[0].file_size(path),
        Some(MEMBER_BYTES as u64)
    );
    assert_eq!(
        changed.packs()[0]
            .read_file(path)
            .unwrap()
            .unwrap()
            .as_ref(),
        payload
    );
    assert!(
        Changes::between(
            &changed,
            Some((&previous.admission, &previous.dependencies))
        )
        .ui,
        "changed tail bytes must invalidate a contents-only dependency"
    );
}
