//! Original procedural cape for local developer recordings.

use bevy::prelude::World;
use serde_json::{Value, json};

use crate::player_skin::LocalPlayerSkin;

/// Changes the synthetic local skin feed without sending cosmetic data to a server.
pub(super) fn apply(world: &mut World, enabled: bool) -> Result<Value, String> {
    let mut skin = world
        .get_resource_mut::<LocalPlayerSkin>()
        .ok_or("the local player skin is unavailable")?;
    skin.set_test_cape(enabled.then(test_cape));
    Ok(json!({ "test_cape": enabled }))
}

/// Creates teal cloth with gold diagonals and a red border from original opaque pixels.
fn test_cape() -> protocol::CapeImage {
    let (width, height) = (64, 32);
    let mut rgba8 = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let color = if y % 16 < 2 || x % 12 < 2 {
                [190, 52, 62, 255]
            } else if (x + y) % 8 < 2 {
                [249, 202, 83, 255]
            } else {
                [24, 113, 123, 255]
            };
            rgba8.extend_from_slice(&color);
        }
    }
    protocol::CapeImage {
        width: width as u32,
        height: height as u32,
        rgba8: rgba8.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cape_changes_local_feed_and_can_be_removed() {
        let mut world = World::new();
        world.insert_resource(LocalPlayerSkin::generated_default("cape fixture"));
        let original = world.resource::<LocalPlayerSkin>().player_skin();
        apply(&mut world, true).unwrap();
        let protocol::PlayerSkin::Standard(skinned) =
            world.resource::<LocalPlayerSkin>().player_skin()
        else {
            panic!("a generated skin must be standard");
        };
        let cape = skinned.cape.unwrap();
        assert_eq!((cape.width, cape.height), (64, 32));
        assert_eq!(cape.rgba8.len(), 64 * 32 * 4);
        assert!(cape.rgba8.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert_ne!(&cape.rgba8[0..4], &cape.rgba8[4 * (64 * 3 + 3)..][..4]);
        apply(&mut world, false).unwrap();
        assert_eq!(world.resource::<LocalPlayerSkin>().player_skin(), original);
    }
}
