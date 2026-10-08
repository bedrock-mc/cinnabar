use bedrock_client::asset_startup::{ACTOR_ASSETS_FILENAME, actor_asset_path};
use std::path::Path;

#[test]
fn generic_actor_carrier_is_a_sibling_of_the_selected_world_carrier() {
    assert_eq!(
        actor_asset_path(Path::new("custom assets/world.mcbea")),
        Path::new("custom assets").join(ACTOR_ASSETS_FILENAME)
    );
}
