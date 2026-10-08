use super::*;
struct Adapter;
impl CollisionWorld for Adapter {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut query = CollisionQuery::synthetic(vec![]);
        query.identity.registry.preg_sha256 = [7; 32];
        Ok(query)
    }
}
#[test]
fn review_default_physics_uses_its_adapters_registry_identity() {
    let expected = Adapter
        .collision_boxes(Aabb::player_at(Vec3::ZERO))
        .unwrap()
        .identity;
    assert_eq!(Adapter.block_physics([0; 3]).unwrap().identity, expected);
}
