//! Static map geometry stays independent of startup and session item allocations.
use super::*;

#[test]
fn map_planes_remain_distinct_when_the_item_mesh_range_is_full() {
    for next in [MAX_ITEM_MESHES as u32 - 1, MAX_ITEM_MESHES as u32] {
        let (mut runtime, _, _) = block_fixture();
        runtime.next_mesh = next;
        runtime.take_pending_geometries();
        let planes = runtime.map_meshes();
        assert!(
            planes.iter().all(|id| {
                (0..session::MAX_SESSION_MESHES)
                    .all(|index| session::session_mesh_id(index) != Some(*id))
            }),
            "map planes must not replace session item geometry"
        );
        assert_ne!(planes[0], planes[1]);
        assert!(
            planes
                .iter()
                .all(|id| id.0 < render_model::skin_rig_id(0).0)
        );
        assert_eq!(runtime.next_mesh, next);
        let geometry = runtime.take_pending_geometries();
        assert_eq!(geometry.len(), planes.len());
        let mut builder = render_model::ActorRigFrameBuilder::new(geometry.clone()).unwrap();
        for index in [0, session::MAX_SESSION_MESHES - 1] {
            let id = session::session_mesh_id(index).unwrap();
            let icon = ActorRigGeometry::synthetic_cuboid(id, [0.0; 3], [1.0; 3], 1).unwrap();
            builder.insert_geometry(icon).unwrap();
        }
        for plane in &geometry {
            assert_eq!(builder.geometry(plane.id).unwrap().vertices, plane.vertices);
        }
        let ordinary = runtime.build_item_mesh(|id| {
            ActorRigGeometry::synthetic_cuboid(id, [0.0; 3], [1.0; 3], 1).ok()
        });
        assert_eq!(
            ordinary,
            (next < MAX_ITEM_MESHES as u32).then(|| item_mesh_rig_id(next))
        );
        assert!(ordinary.is_none_or(|id| !planes.contains(&id)));
        runtime.take_pending_geometries();
        runtime.meshes.insert(
            MeshKey::Session(0),
            Some(session::session_mesh_id(0).unwrap()),
        );
        runtime.meshes.insert(
            MeshKey::SessionBlock(session::MAX_SESSION_MESHES - 1),
            session::session_mesh_id(session::MAX_SESSION_MESHES - 1),
        );
        runtime.set_session_items(None, None, Vec::new());
        assert!(!runtime.meshes.contains_key(&MeshKey::Session(0)));
        assert!(
            !runtime
                .meshes
                .contains_key(&MeshKey::SessionBlock(session::MAX_SESSION_MESHES - 1))
        );
        runtime.set_pack_layer(None);
        assert_eq!(runtime.map_meshes(), planes);
        assert!(runtime.take_pending_geometries().is_empty());
        for plane in geometry {
            assert_eq!(builder.geometry(plane.id).unwrap().vertices, plane.vertices);
        }
    }
}
