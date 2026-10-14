use crate::menu::MenuRuntime;
use launcher::menu::MenuAction;

#[test]
fn clearing_the_distant_terrain_cache_is_requested_once_and_reported() {
    let mut menu = MenuRuntime::new(false, 2, "Clearer".into());
    assert!(!menu.take_distant_terrain_clear_request());
    menu.activate(MenuAction::SettingsClearDistantTerrainCache);
    menu.activate(MenuAction::SettingsClearDistantTerrainCache);
    assert!(menu.view().message.is_some(), "the press is acknowledged");
    assert!(menu.take_distant_terrain_clear_request());
    assert!(
        !menu.take_distant_terrain_clear_request(),
        "a repeated press while waiting asks once"
    );
    menu.report_distant_terrain_clear("Cleared the distant terrain cache.".to_owned());
    assert_eq!(
        menu.view().message.as_deref(),
        Some("Cleared the distant terrain cache.")
    );
}
