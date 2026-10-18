use super::*;

#[test]
fn catalog_rollback_activates_the_readmitted_revision_and_advances_generation() {
    let _catalog = crate::test_support::catalog_lock();
    let a = crate::test_support::emote("fixture:rollback", "Revision A", true);
    let b = crate::test_support::emote("fixture:rollback", "Revision B", false);
    activate(&[b]);
    assert_eq!(Item::from_id(&a.descriptor().id), Some(b));
    let generation = Item::generation();
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/cinnabarapi/schema/emote-clips.json"
    ))
    .unwrap();
    let bytes = serde_json::to_vec(&cases[0]["clip"]).unwrap();
    let readmitted = Item::admit(a.descriptor().clone(), &bytes).unwrap();
    assert_eq!(readmitted, a);
    activate(&[readmitted]);
    assert_eq!(Item::from_id(&a.descriptor().id), Some(a));
    assert_eq!(Item::all(), vec![a]);
    assert!(Item::from_id(&a.descriptor().id).unwrap().descriptor().free);
    assert_ne!(Item::generation(), generation);
}

#[test]
fn parser_matches_the_api_conformance_cases() {
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/cinnabarapi/schema/source.json"
    ))
    .unwrap();
    let fixtures = include_bytes!("../../../../core/cinnabarapi/schema/emote-clips.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(fixtures)),
        source["conformance"]["sha256"].as_str().unwrap()
    );
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/cinnabarapi/schema/emote-clips.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let bytes = serde_json::to_vec(&case["clip"]).unwrap();
        assert_eq!(
            Clip::parse(&bytes).is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn clip_admission_rejects_oversized_or_corrupted_assets_and_counts_them() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/cinnabarapi/schema/emote-clips.json"
    ))
    .unwrap();
    let bytes = serde_json::to_vec(&cases[0]["clip"]).unwrap();
    let item = crate::test_support::emote("fixture:admission", "Admission", true);
    let descriptor = item.descriptor().clone();
    let before = rejected();
    assert!(Item::admit(descriptor.clone(), b"corrupt").is_none());
    assert!(Item::admit(descriptor, &vec![b' '; max_bytes() + 1]).is_none());
    assert!(rejected() >= before + 2);
    assert!(Clip::parse(&vec![b' '; max_bytes() + 1]).is_none());
    assert!(Clip::parse(&bytes).is_some());
}

#[test]
fn logical_rig_has_unique_parent_before_child_joints() {
    for (index, joint) in rig().iter().enumerate() {
        assert!(
            !rig()[..index]
                .iter()
                .any(|previous| previous.joint == joint.joint)
        );
        if let Some(parent) = &joint.parent {
            assert!(
                rig()[..index]
                    .iter()
                    .any(|previous| previous.joint == *parent)
            );
        }
    }
}

#[test]
fn external_api_catalog_clips_admit_without_client_asset_copies() {
    let Some(root) = std::env::var_os("CINNABAR_API_CHECKOUT").map(std::path::PathBuf::from) else {
        eprintln!(
            "missing-fixture: API checkout; set CINNABAR_API_CHECKOUT for emote compatibility"
        );
        return;
    };
    let catalog: Catalog =
        serde_json::from_slice(&std::fs::read(root.join("src/catalog.json")).unwrap()).unwrap();
    assert!(!catalog.emotes.is_empty());
    for descriptor in catalog.emotes {
        assert!(supported(&descriptor));
        let bytes =
            std::fs::read(root.join("assets/cosmetics").join(&descriptor.clip.sha256)).unwrap();
        assert!(Item::admit(descriptor, &bytes).is_some());
    }
}

#[test]
fn catalog_clip_identifiers_fit_the_emote_wire_vocabulary() {
    let item = crate::test_support::emote("fixture:identifier", "Identifier", true);
    assert!(supported(item.descriptor()));
    let mut descriptor = item.descriptor().clone();
    descriptor.id = "fixture:path/clip".into();
    assert!(!supported(&descriptor));
    assert_eq!(
        schema()["$defs"]["CosmeticsCatalog"]["properties"]["emotes"]["maxItems"],
        MAX_ITEMS
    );
}

#[test]
fn admitted_key_rotations_are_unit_length_at_constant_and_exact_key_times() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/cinnabarapi/schema/emote-clips.json"
    ))
    .unwrap();
    let mut value = cases[0]["clip"].clone();
    value["tracks"][0]["keys"][0]["rotation"] = serde_json::json!([0.0, 0.0, 0.0, 0.9999]);
    let clip = Clip::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(clip.tracks[0].keys[0].rotation, [0.0, 0.0, 0.0, 1.0]);
}
