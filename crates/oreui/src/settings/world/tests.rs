use super::super::{Content, SettingSection, sections, sidebar};
use crate::review_tests::paint;
use bridge::{Backend, GameMode};
use launcher::local_worlds::in_world::{Action, View};
use launcher::local_worlds::options::{WorldOption as O, WorldOptions};
use launcher::local_worlds::world::{Difficulty, World};
use launcher::menu::{MenuAction, MenuView};
use std::collections::HashMap;
use std::sync::Arc;

fn view(cheats: bool) -> MenuView {
    let mut options = WorldOptions::default();
    options.restore_cheats(cheats);
    let world = World {
        id: "0123456789abcdef".to_owned(),
        name: "Home".to_owned(),
        game_mode: GameMode::Survival,
        generator: Default::default(),
        difficulty: Difficulty::Normal,
        backend: Backend::Bds,
        seed: 1,
        options,
        options_unavailable: false,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    };
    let mut view = MenuView::new(false, "Player".into());
    view.world_settings = View::of(&world).map(Arc::new);
    view
}

/// The actions the Game section's controls answer to.
fn actions(view: &MenuView) -> Vec<MenuAction> {
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [0.0, 1000.0],
            column_width: 1000.0,
            y: 0.0,
            translate: &|_| None,
            nested: false,
        };
        sections::draw(&mut content, SettingSection::Game).unwrap();
    });
    hits.into_iter().map(|(action, _)| action).collect()
}

fn world(action: Action) -> MenuAction {
    MenuAction::WorldSettings(action)
}

/// Live settings answer presses; settings fixed with the world and cheat rules without cheats do not.
#[test]
fn only_changeable_settings_take_presses() {
    let off = actions(&view(false));
    for live in [
        world(Action::GameMode(GameMode::Creative)),
        world(Action::Difficulty(Difficulty::Hard)),
        world(Action::Option(O::Pvp, 0)),
        world(Action::Option(O::ActivateCheats, 1)),
        world(Action::OptionField(O::RespawnRadius)),
        world(Action::Option(O::PlayerPermissions, 2)),
    ] {
        assert!(off.contains(&live), "{live:?} missing from {off:?}");
    }
    for fixed in [
        world(Action::Option(O::BonusChest, 1)),
        world(Action::Option(O::StartingMap, 1)),
        world(Action::Option(O::KeepInventory, 1)),
        world(Action::OptionField(O::RandomTickSpeed)),
        world(Action::OptionField(O::SimulationDistance)),
    ] {
        assert!(!off.contains(&fixed), "{fixed:?} took a press");
    }
    let on = actions(&view(true));
    assert!(on.contains(&world(Action::Option(O::KeepInventory, 1))));
    assert!(on.contains(&world(Action::OptionField(O::RandomTickSpeed))));
    assert!(on.contains(&world(Action::Option(O::ActivateCheats, 0))));
}

/// The sidebar lists Game only while the host plays their own world.
#[test]
fn the_sidebar_lists_game_only_in_the_hosts_world() {
    let game = MenuAction::SettingsSection(SettingSection::Game.definition().index);
    let listed = |view: &MenuView| {
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            sidebar::draw(canvas, view, [0.0, 0.0, 400.0, 4000.0], &|_| None, None).unwrap();
        });
        hits.iter().any(|(action, _)| *action == game)
    };
    assert!(listed(&view(false)));
    assert!(!listed(&MenuView::new(false, "Player".into())));
}
