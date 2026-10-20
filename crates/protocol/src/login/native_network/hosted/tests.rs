use super::*;
use bedrock_client::packs::{PackEvent, PackOffer, PackTransferLimits};
use bedrock_protocol_1_26_51::types::{PackInfoData, PackInstanceId};

/// Uses the real preload/final-stack owner, retaining the absence of upstream chunk metadata.
fn preloaded(archive: Vec<u8>, id: uuid::Uuid) -> packs::Acquired {
    let mut offered = PackInfoData::default();
    offered.pack_id_version.pack_uuid = id;
    offered.pack_id_version.pack_version.version = "1.0.0".into();
    offered.pack_size = wire::U64LE(archive.len() as u64);
    offered.cdn_url = "https://example.invalid/original.zip".into();
    let facts = packs::verified::describe(&archive, &offered).unwrap();
    let mut offer = PackOffer::new_with_metadata(
        packets::ResourcePacksInfo {
            resource_packs: vec![offered],
            resource_pack_required: true,
            ..Default::default()
        },
        PackTransferLimits::default(),
    )
    .unwrap();
    offer.supply_preloaded(id, archive).unwrap();
    let (mut transfer, _) = offer.start().unwrap();
    let PackEvent::Finished(complete) = transfer
        .receive(
            packets::ResourcePackStack {
                texture_pack_list: vec![PackInstanceId {
                    pack_id: id.to_string().into(),
                    version: "1.0.0".into(),
                    ..Default::default()
                }],
                texture_pack_required: true,
                ..Default::default()
            }
            .into(),
        )
        .unwrap()
    else {
        panic!("real preloaded stack did not complete");
    };
    let (_, handoff, metadata) = complete.into_parts_with_metadata();
    packs::Acquired {
        handoff,
        metadata: metadata.map(Box::new),
        preloaded: BTreeMap::from([(id, facts)]),
    }
}

#[test]
fn hosted_snapshot_accepts_actual_preloaded_archive_without_invented_data_info() {
    let id = uuid::Uuid::from_u128(1);
    let original = resource_pack::test_support::archive_bytes(id, &[]);
    resource_pack::verify_archive_identity(&original, id, "1.0.0").unwrap();
    let acquired = preloaded(original.clone(), id);
    assert!(matches!(
        acquired.metadata.as_ref().unwrap().data.as_slice(),
        [None]
    ));
    let result = snapshot(acquired);
    assert!(
        result.is_ok(),
        "a verified original preloaded archive required nonexistent upstream DataInfo"
    );
    let (snapshot, archives) = result.unwrap();
    assert_eq!(archives.as_slice(), &[Bytes::from(original)]);
    assert_eq!(
        snapshot.offer.resource_packs[0].pack_id_version.pack_uuid,
        id
    );
    assert_eq!(snapshot.contents.len(), 1);
}

/// Retained facts cannot be paired with replacement archive metadata after preload completion.
#[test]
fn hosted_preloaded_snapshot_rejects_replaced_version_size_and_key() {
    let id = uuid::Uuid::from_u128(1);
    let original = resource_pack::test_support::archive_bytes(id, &[]);
    for replaced in ["facts", "version", "size", "key"] {
        let mut acquired = preloaded(original.clone(), id);
        let mut archives = acquired.handoff.into_archives();
        let archive = &mut archives[0];
        let offered = &mut acquired.metadata.as_mut().unwrap().offer.resource_packs[0];
        match replaced {
            "facts" => acquired.preloaded.clear(),
            "version" => {
                archive.version = "01.0.0".into();
                offered.pack_id_version.pack_version.version = archive.version.clone().into();
            }
            "size" => {
                archive.archive.push(0);
                offered.pack_size = wire::U64LE(archive.archive.len() as u64);
            }
            "key" => {
                *archive = bedrock_packs::ResourcePackArchive::with_content_key(
                    id,
                    archive.version.clone(),
                    archive.sub_pack_name.clone(),
                    std::mem::take(&mut archive.archive),
                    b"replacement".to_vec(),
                );
                offered.content_key = "replacement".into();
            }
            _ => unreachable!(),
        }
        acquired.handoff = bedrock_packs::ResourcePackHandoff::from_archives(archives);
        assert!(
            snapshot(acquired).is_err(),
            "replacement {replaced} bypassed the original verified archive descriptor"
        );
    }
}
