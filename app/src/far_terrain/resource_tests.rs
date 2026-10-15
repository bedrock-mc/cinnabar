use super::tests::{far_terrain_app, flat};
use super::*;
use far_terrain::ColumnSummary;

#[test]
fn pack_switch_clears_far_colors_and_rebuilds_the_active_visit() {
    let mut app = far_terrain_app(64);
    app.update();
    let position = ColumnPos::new(30, 0);
    {
        let mut far = app.world_mut().resource_mut::<FarTerrain>();
        far.cache
            .store()
            .admit(position, &ColumnSummary::decode(&flat(64).bytes).unwrap());
        assert_eq!(far.cache.store().len(), 1);
    }
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .terrain_resource_identity = [1; 32];
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<FarTerrain>()
            .cache
            .store()
            .len(),
        0
    );
}
