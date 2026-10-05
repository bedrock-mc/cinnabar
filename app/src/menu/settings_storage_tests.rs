//! Disk inventory and deletion boundary regressions.

use super::*;

#[path = "settings_storage/fixtures.rs"]
mod fixtures;

use fixtures::fixture;

#[test]
fn inventory_measures_nested_contents_and_cache_clear_preserves_other_data() {
    let root = fixture();
    let cache = root.join("objects");
    let pack = cache.join("pack");
    fs::create_dir_all(&pack).unwrap();
    fs::write(pack.join("manifest"), [0; 13]).unwrap();
    fs::write(pack.join("texture"), [0; 29]).unwrap();
    fs::write(root.join("account"), b"keep").unwrap();
    let items = entries(&cache).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].bytes, 42);
    remove_entry(&cache, &items[0].path).unwrap();
    assert!(entries(&cache).unwrap().is_empty());
    assert_eq!(fs::read(root.join("account")).unwrap(), b"keep");
    assert!(remove_entry(&cache, &root.join("account")).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn linked_roots_and_entries_are_never_deleted_or_counted() {
    let root = fixture();
    let cache = root.join("objects");
    fs::create_dir_all(&cache).unwrap();
    let outside = root.join("keep");
    fs::write(&outside, b"private").unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("escape")).unwrap();
    assert!(entries(&cache).unwrap().is_empty());
    assert!(remove_entry(&cache, &cache.join("escape")).is_err());
    fs::write(cache.join("pack"), [0; 5]).unwrap();
    let linked = root.join("linked");
    std::os::unix::fs::symlink(&cache, &linked).unwrap();
    assert!(remove_entry(&linked, &linked.join("pack")).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"private");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn confirmed_screenshot_delete_only_removes_saved_pngs() {
    let root = fixture();
    let mut layout = crate::install_layout::scratch("screenshots");
    layout.user_data_root = root.clone();
    fs::create_dir_all(layout.screenshots_dir()).unwrap();
    fs::create_dir_all(layout.resource_pack_cache_dir()).unwrap();
    let screenshot = layout.screenshots_dir().join("saved.png");
    let other = layout.screenshots_dir().join("notes.txt");
    let pack = layout.resource_pack_cache_dir().join("downloaded-pack");
    fs::write(&screenshot, b"png").unwrap();
    fs::write(&other, b"notes").unwrap();
    fs::write(&pack, b"pack").unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout = layout;
    menu.refresh_storage();
    menu.activate_storage(StorageAction::RequestScreenshots);
    assert!(
        screenshot.exists(),
        "request must not delete before confirmation"
    );
    menu.activate_storage(StorageAction::ConfirmDelete);
    assert!(!screenshot.exists());
    assert!(other.exists());
    assert!(pack.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_without_selection_preserves_every_cached_pack() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    fs::create_dir_all(&cache).unwrap();
    for name in ["a", "b"] {
        fs::write(cache.join(name), name).unwrap();
    }
    menu.refresh_storage();
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::RequestDelete);
    assert_ne!(menu.dialog, Some(MenuDialog::StorageDelete));
    menu.activate_storage(StorageAction::ConfirmDelete);
    assert_eq!(entries(&cache).unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn individual_delete_keeps_the_requested_target_after_selection_changes() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    fs::create_dir_all(&cache).unwrap();
    for name in ["a", "b"] {
        fs::write(cache.join(name), name).unwrap();
    }
    menu.refresh_storage();
    let requested = menu.storage.cached[0].path.clone();
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::RequestDelete);
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::ConfirmDelete);
    assert!(!requested.exists());
    assert_eq!(entries(&cache).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}
