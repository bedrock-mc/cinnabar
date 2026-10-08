use super::super::*;

pub(super) fn entity_assets() -> Option<Arc<RuntimeEntityAssets>> {
    static ASSETS: std::sync::LazyLock<Option<Arc<RuntimeEntityAssets>>> =
        std::sync::LazyLock::new(|| {
            let Some(path) = std::env::var_os("CINNABAR_ENTITY_CARRIER") else {
                eprintln!("skipping animal runtime fixture: CINNABAR_ENTITY_CARRIER is unset");
                return None;
            };
            let path = std::path::PathBuf::from(path);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!(
                        "skipping animal runtime fixture: missing entity carrier {}",
                        path.display()
                    );
                    return None;
                }
                Err(error) => panic!("read animal fixture {}: {error}", path.display()),
            };
            Some(Arc::new(
                RuntimeEntityAssets::decode(&bytes).expect("valid animal entity carrier"),
            ))
        });
    ASSETS.clone()
}
