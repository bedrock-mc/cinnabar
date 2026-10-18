use super::*;
use std::cell::Cell;

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

thread_local! {
    pub(in crate::retained) static VERTEX_VISITS: Cell<usize> = const { Cell::new(0) };
}

/// Fills a shared mesh while keeping its payload within one callback's byte allowance.
fn large_mesh() -> GeometrySpec {
    let GeometrySpec::Triangles {
        mut vertices,
        indices,
    } = mesh()
    else {
        unreachable!()
    };
    let extent = mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS;
    for (vertex, position) in
        vertices
            .iter_mut()
            .zip([[extent, 0., 0.], [0., extent, 0.], [0., 0., extent]])
    {
        vertex.position = position;
    }
    vertices.resize(mod_api::MAX_DRAW_VERTICES / 4, vertices[0]);
    GeometrySpec::Triangles { vertices, indices }
}

#[test]
fn shared_mesh_instances_do_not_revisit_vertices() {
    let mut store = Store::default();
    VERTEX_VISITS.set(0);
    let geometry = store.create_geometry(large_mesh()).unwrap();
    assert_eq!(VERTEX_VISITS.get(), mod_api::MAX_DRAW_VERTICES / 4);
    store.commit();
    store.begin();
    VERTEX_VISITS.set(0);
    for count in [1, mod_api::MAX_DRAW_INSTANCES] {
        for _ in 0..2 {
            store.begin();
            let instances = (0..count)
                .map(|index| {
                    let mut instance = drawing(geometry);
                    let magnitude = 1. - index as f32 / (2 * mod_api::MAX_DRAW_INSTANCES) as f32;
                    let scale = if index % 2 == 0 {
                        magnitude
                    } else {
                        -magnitude
                    };
                    instance.transform.scale = [scale; 3];
                    instance.transform.translation[0] =
                        mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS * (1. - scale.abs());
                    instance
                })
                .collect();
            store
                .replace_group("shared".into(), instances, DrawingPermissions::ALL)
                .unwrap();
            assert_eq!(VERTEX_VISITS.get(), 0);
            store.commit();
        }
    }
    let before = Arc::clone(store.snapshot());
    store.begin();
    let mut invalid = vec![drawing(geometry); mod_api::MAX_DRAW_INSTANCES];
    invalid.last_mut().unwrap().transform.translation[0] = mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS;
    for _ in 0..2 {
        assert_eq!(
            store
                .replace_group("shared".into(), invalid.clone(), DrawingPermissions::ALL)
                .unwrap_err()
                .code,
            ErrorCode::Invalid
        );
    }
    assert_eq!(VERTEX_VISITS.get(), 0);
    store.rollback();
    assert!(Arc::ptr_eq(&before, store.snapshot()));

    store.begin();
    let instance = drawing(geometry);
    let handle = store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    for _ in 0..2 {
        store.begin();
        store
            .update(handle, instance.clone(), DrawingPermissions::ALL)
            .unwrap();
        store.commit();
    }
    assert_eq!(VERTEX_VISITS.get(), 0);
}

#[test]
fn empty_callbacks_allocate_nothing_and_preserve_the_snapshot() {
    let mut store = Store::default();
    let geometry = store.create_geometry(large_mesh()).unwrap();
    store
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    let (_, allocations) = test_alloc::count(|| {
        for _ in 0..8 {
            store.begin();
            assert!(store.staged.is_none());
            assert_eq!(store.mutation_bytes, 0);
            store.commit();
            assert!(store.staged.is_none());
        }
    });
    assert_eq!(allocations, 0);
    assert!(Arc::ptr_eq(&before, store.snapshot()));
}

#[test]
fn cached_bounds_cover_signed_and_nonuniform_transforms() {
    let GeometrySpec::Triangles {
        mut vertices,
        indices,
    } = mesh()
    else {
        unreachable!()
    };
    let positions = [[-4., 0., 0.], [0., 3., 0.], [0., 0., -2.]];
    for (vertex, position) in vertices.iter_mut().zip(positions) {
        vertex.position = position;
    }
    let mut store = Store::default();
    let geometry = store
        .create_geometry(GeometrySpec::Triangles { vertices, indices })
        .unwrap();
    VERTEX_VISITS.set(0);
    let extent = mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS;
    for (axis, position) in [4., 3., 2.].into_iter().enumerate() {
        let mut instance = drawing(geometry);
        instance.transform.scale = [0.; 3];
        instance.transform.scale[axis] = -extent / (2. * position);
        instance.transform.translation[axis] = -extent / 2.;
        let handle = store
            .create(instance.clone(), DrawingPermissions::ALL)
            .unwrap();
        instance.transform.translation[axis] -= 1.;
        assert_eq!(
            store
                .update(handle, instance, DrawingPermissions::ALL)
                .unwrap_err()
                .code,
            ErrorCode::Invalid
        );
    }
    store.commit();
    assert_eq!(store.snapshot().instances.len(), positions.len());
    assert_eq!(VERTEX_VISITS.get(), 0);
}

#[test]
fn unchanged_instance_updates_stage_nothing_and_allocate_nothing() {
    let mut store = Store::default();
    let geometry = store.create_geometry(large_mesh()).unwrap();
    let instance = drawing(geometry);
    let handle = store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    let (_, allocations) = test_alloc::count(|| {
        for _ in 0..8 {
            store.begin();
            store
                .update(handle, instance.clone(), DrawingPermissions::ALL)
                .unwrap();
            assert!(store.staged.is_none());
            assert_eq!(store.mutation_bytes, 0);
            store.commit();
        }
    });
    assert_eq!(allocations, 0);
    assert!(Arc::ptr_eq(&before, store.snapshot()));
    assert_eq!(before.geometry_revision, store.snapshot().geometry_revision);
    assert_eq!(before.instance_revision, store.snapshot().instance_revision);
}
