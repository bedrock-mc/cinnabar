use super::*;
use crate::asset_startup::test_carriers::synthetic_entity_blob;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        // macOS clocks are microsecond-grained, so parallel tests need the counter too.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "actor-carrier-{}-{unique}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn decoded_entities(seed: u8) -> assets::RuntimeEntityAssets {
    assets::RuntimeEntityAssets::decode(&synthetic_entity_blob(seed)).unwrap()
}

#[test]
fn required_actor_carrier_absence_and_size_failure_name_rebuild_command() {
    let directory = Directory::new();
    let world = directory.0.join("world.mcbea");
    let entities = decoded_entities(0);
    let error = read_coherent_actor_assets(&world, &entities).unwrap_err();
    assert!(error.to_string().contains(ACTOR_ASSETS_FILENAME));
    assert!(error.to_string().contains("make actor-assets"));
    let file = std::fs::File::create(actor_asset_path(&world)).unwrap();
    file.set_len(assets::MAX_ACTOR_CARRIER_BYTES as u64 + 1)
        .unwrap();
    let error = read_coherent_actor_assets(&world, &entities).unwrap_err();
    assert!(error.to_string().contains("exceeds startup byte bound"));
    assert!(error.to_string().contains("make actor-assets"));
}

#[test]
fn actor_carrier_compiled_against_another_entity_carrier_fails_closed() {
    let directory = Directory::new();
    let world = directory.0.join("world.mcbea");
    let actor = assets::encode_actor_catalog(&synthetic_entity_blob(0), &[], &[]).unwrap();
    std::fs::write(actor_asset_path(&world), actor).unwrap();
    assert!(read_coherent_actor_assets(&world, &decoded_entities(0)).is_ok());
    let error = read_coherent_actor_assets(&world, &decoded_entities(1)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("actor carrier identity mismatch")
    );
    assert!(error.to_string().contains("make actor-assets"));
}
