use super::*;
use mod_api::drawing::DrawingPermissions;

/// Creates an explicitly local owner with one admitted actor incarnation.
fn store(owner: u64) -> (MaskStore, MaskEntity) {
    let mut store = MaskStore::new(owner);
    store.bind(DrawingSession {
        connection: 7,
        dimension: 0,
        local: true,
        permissions: DrawingPermissions::ALL,
    });
    let entity = MaskEntity {
        connection: 7,
        dimension: 0,
        runtime_id: 9,
        spawn_revision: 1,
    };
    store.publish_targets(&[(entity, 36)]);
    (store, entity)
}

#[test]
fn masks_cannot_cross_owners_or_resource_kinds() {
    let (mut a, _) = store(1);
    let (mut b, _) = store(2);
    let mask = a.create(MaskDepth::Test).unwrap();
    let target = a.entity(9).unwrap();
    assert_eq!(b.select(mask, &[]), Err(MaskError::StaleHandle));
    let other = b.create(MaskDepth::Test).unwrap();
    assert_eq!(b.select(other, &[target]), Err(MaskError::StaleHandle));
    assert_eq!(a.select(target, &[]), Err(MaskError::StaleHandle));
    assert_eq!(a.select(mask, &[mask]), Err(MaskError::StaleHandle));
}

#[test]
fn merging_keeps_masks_and_retained_resources_in_owner_order() {
    let outputs: Vec<_> = (1..=2)
        .map(|owner| {
            let (mut masks, _) = store(owner);
            masks.create(MaskDepth::Test).unwrap();
            crate::RenderOutput {
                masks: masks.commit(),
                retained: vec![Arc::default()],
                ..Default::default()
            }
        })
        .collect();
    let mut merge = crate::RenderMerge::default();
    let merged = merge.merge(&outputs);
    assert_eq!(merged.masks.len(), outputs.len());
    assert_eq!(merged.retained.len(), outputs.len());
    for (index, output) in outputs.iter().enumerate() {
        assert_eq!(merged.masks[index], output.masks[0]);
        assert!(Arc::ptr_eq(&merged.retained[index], &output.retained[0]));
    }
    let remaining = merge.merge([&outputs[1]]);
    assert_eq!(remaining.masks, outputs[1].masks);
    assert_eq!(remaining.retained.len(), 1);
    assert!(Arc::ptr_eq(&remaining.retained[0], &outputs[1].retained[0]));
}

#[test]
fn selection_is_an_atomic_bounded_union_and_rolls_back() {
    let (mut store, key) = store(1);
    let mask = store.create(MaskDepth::Test).unwrap();
    let entity = store.entity(9).unwrap();
    store.select(mask, &[entity, entity]).unwrap();
    let committed = store.commit();
    assert_eq!(committed[0].entities, [key]);
    store.begin();
    assert_eq!(
        store.select(mask, &[entity; MAX_MASK_ENTITIES + 1]),
        Err(MaskError::BudgetExceeded)
    );
    store.publish_targets(&[(key, MAX_MASK_VERTICES + 1)]);
    assert_eq!(
        store.select(mask, &[entity]),
        Err(MaskError::BudgetExceeded)
    );
    assert_eq!(store.commit(), committed);
    store.select(mask, &[]).unwrap();
    store.begin();
    assert!(Arc::ptr_eq(&store.commit(), &committed));
    store.select(mask, &[]).unwrap();
    assert!(store.commit()[0].entities.is_empty());
}

#[test]
fn aborted_creation_and_removed_masks_never_reuse_tokens() {
    let (mut store, _) = store(1);
    let aborted = store.create(MaskDepth::Test).unwrap();
    store.begin();
    let live = store.create(MaskDepth::Test).unwrap();
    assert_ne!(aborted, live);
    store.commit();
    store.remove(live).unwrap();
    let replacement = store.create(MaskDepth::Test).unwrap();
    assert_ne!(live, replacement);
    for _ in 1..MAX_MASKS {
        store.create(MaskDepth::Test).unwrap();
    }
    assert_eq!(
        store.create(MaskDepth::Test),
        Err(MaskError::BudgetExceeded)
    );
}

#[test]
fn session_change_revokes_targets_and_masks_and_id_reuse_cannot_retarget() {
    let (mut store, key) = store(1);
    let mask = store.create(MaskDepth::Test).unwrap();
    let entity = store.entity(9).unwrap();
    store.select(mask, &[entity]).unwrap();
    store.commit();
    store.publish_targets(&[]);
    assert_eq!(store.entity(9), Err(MaskError::StaleHandle));
    assert_eq!(
        store.commit()[0].entities,
        [key],
        "culling hides without retargeting"
    );
    store.publish_targets(&[(
        MaskEntity {
            spawn_revision: 2,
            ..key
        },
        36,
    )]);
    assert_eq!(store.select(mask, &[entity]), Err(MaskError::StaleHandle));
    assert_ne!(store.entity(9).unwrap(), entity);
    store.bind(DrawingSession {
        connection: 8,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    });
    assert!(store.commit().is_empty());
    assert_eq!(store.remove(mask), Err(MaskError::StaleHandle));
    assert_eq!(store.entity(9), Err(MaskError::StaleHandle));
}

#[test]
fn hidden_lookup_and_always_depth_fail_closed_independently() {
    let mut store = MaskStore::new(1);
    assert_eq!(store.create(MaskDepth::Test), Err(MaskError::Denied));
    store.bind(DrawingSession {
        connection: 1,
        permissions: DrawingPermissions {
            entity_highlight: true,
            ..Default::default()
        },
        ..Default::default()
    });
    assert!(store.create(MaskDepth::Test).is_ok());
    let key = MaskEntity {
        connection: 1,
        dimension: 0,
        runtime_id: 9,
        spawn_revision: 1,
    };
    store.publish_targets(&[(key, 36)]);
    let mask = store.create(MaskDepth::Test).unwrap();
    let guessed = DrawingHandle {
        slot: mask.slot + 1,
        ..mask
    };
    assert_eq!(store.select(mask, &[guessed]), Err(MaskError::StaleHandle));
    assert_eq!(store.create(MaskDepth::Always), Err(MaskError::Denied));
    assert_eq!(store.entity(9), Err(MaskError::Denied));
    store.revoke();
    assert!(store.commit().is_empty());
}

#[test]
fn attachment_accounting_includes_resolve_and_all_samples() {
    assert_eq!(attachment_bytes(1920, 1080, 1), Some(1920 * 1080 * 4 * 2));
    assert_eq!(attachment_bytes(1920, 1080, 4), Some(1920 * 1080 * 4 * 9));
    assert_eq!(attachment_bytes(u32::MAX, u32::MAX, u32::MAX), None);
}
#[test]
fn appearance_is_bounded_and_foreign_bindings_are_rejected() {
    let (mut store, _) = store(1);
    let mask = store.create(MaskDepth::Test).unwrap();
    let style = mod_api::drawing::MaskStyle {
        outline: [1.0; 4],
        outline_radius: 1.0,
        glow: [1.0; 4],
        glow_radius: mod_api::drawing::MAX_MASK_GLOW_RADIUS,
    };
    store.set_style(mask, Some(style)).unwrap();
    assert_eq!(
        store.set_style(
            mask,
            Some(mod_api::drawing::MaskStyle {
                outline_radius: f32::NAN,
                ..style
            })
        ),
        Err(MaskError::Invalid)
    );
    assert_eq!(
        store.set_style(
            mask,
            Some(mod_api::drawing::MaskStyle {
                glow_radius: style.glow_radius + 1.0,
                ..style
            })
        ),
        Err(MaskError::Invalid)
    );
    assert_eq!(store.commit()[0].style, Some(style));
    assert!(store.bindings(&[mask]).is_ok());
    assert_eq!(
        store.bindings(&[DrawingHandle { owner: 99, ..mask }]),
        Err(MaskError::StaleHandle)
    );
    store.remove(mask).unwrap();
    assert_eq!(store.bindings(&[mask]), Err(MaskError::StaleHandle));
}

/// Production mask stores and retained geometry cannot allocate overlapping owner namespaces.
#[test]
fn production_resource_owners_are_unique_across_kinds() {
    let mut geometry = crate::retained::Store::default();
    let vertex = |x| crate::retained::Vertex {
        position: [x, 0., 0.],
        color: [1.; 4],
        uv: [0.; 2],
        data: [0.; 4],
    };
    let handle = geometry
        .create_geometry(crate::retained::GeometrySpec::Polyline {
            vertices: vec![vertex(0.), vertex(1.)],
            closed: false,
        })
        .unwrap();
    let a = MaskStore::default();
    let b = MaskStore::default();
    assert_ne!(a.owner, b.owner);
    assert_ne!(a.owner, handle.owner);
    assert_ne!(b.owner, handle.owner);
}
