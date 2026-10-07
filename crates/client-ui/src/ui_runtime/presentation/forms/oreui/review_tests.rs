//! Small in-memory fixtures for overflow and scrolling review regressions.
use super::paint::Canvas;
use super::*;
use crate::menu::{InboxItem, MenuFriendCard, MenuRealmCard};
use crate::ui_runtime::presentation::menu_scroll::ScrollArea;
use crate::ui_runtime::presentation::tests::fixture_font;
use std::collections::HashMap;

/// Draws one route without installed carriers or texture files.
pub(super) fn paint(
    offsets: HashMap<String, f32>,
    draw: impl FnOnce(&mut Canvas<'_>),
) -> (Vec<ScrollArea>, Vec<(MenuAction, UiRect)>, Vec<UiNode>) {
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.offsets = offsets;
    draw(&mut canvas);
    let (scrolls, hits) = (canvas.scrolls, canvas.hits);
    (scrolls, hits, nodes)
}

/// Every solid fill as window bounds and colour, in draw order.
pub(super) fn solids(nodes: &[UiNode]) -> Vec<([f32; 4], [u8; 4])> {
    let origin = |mut id: Option<ui::UiNodeId>| {
        let mut at = [0.0, 0.0];
        while let Some(node) = id.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
            let min = node.bounds().min();
            at = [at[0] + min.x(), at[1] + min.y()];
            id = node.parent();
        }
        at
    };
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Solid { color, .. } => {
                let [x, y] = origin(node.parent());
                let (min, max) = (node.bounds().min(), node.bounds().max());
                Some(([min.x() + x, min.y() + y, max.x() + x, max.y() + y], *color))
            }
            _ => None,
        })
        .collect()
}

/// Makes enough friends to overflow either list.
fn friends_view() -> MenuView {
    let mut view = crate::menu::MenuView::new(true, "Test".into());
    view.friends = (0..40)
        .map(|index| MenuFriendCard {
            gamertag: format!("Friend {index}"),
            world_name: "World".into(),
            members: "1/8".into(),
            xuid: index.to_string(),
        })
        .collect();
    view
}

#[test]
fn review_friend_list_registers_its_full_scroll_extent() {
    let view = friends_view();
    let (scrolls, _, _) = paint(HashMap::new(), |c| {
        friends::draw(c, &view, [1280.0, 720.0]).unwrap()
    });
    let area = scrolls
        .iter()
        .find(|area| area.max > 0.0)
        .expect("overflow friends need a viewport");
    let (_, hits, _) = paint(HashMap::from([(area.key.clone(), area.max)]), |c| {
        friends::draw(c, &view, [1280.0, 720.0]).unwrap()
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::PlayFriend(39))
    );
}

#[test]
fn review_inbox_list_registers_its_full_scroll_extent() {
    let mut view = friends_view();
    view.feeds.home.inbox = (0..40)
        .map(|index| InboxItem {
            header: index.to_string(),
            body: "Message".into(),
            category: "News".into(),
            unread: index < 20,
            ..Default::default()
        })
        .collect();
    let (scrolls, _, _) = paint(HashMap::new(), |c| {
        inbox::draw(c, &view, [1280.0, 720.0], &|_| None).unwrap()
    });
    assert!(scrolls.iter().any(|area| area.max > 0.0));
}

#[test]
fn review_world_list_reveals_the_last_entry() {
    let view = friends_view();
    let (scrolls, _, _) = paint(HashMap::new(), |c| {
        play::draw(c, &view, [1280.0, 720.0], &HashMap::new()).unwrap()
    });
    let area = scrolls
        .iter()
        .find(|area| area.max > 0.0)
        .expect("overflow worlds need a viewport");
    let (_, hits, _) = paint(HashMap::from([(area.key.clone(), area.max)]), |c| {
        play::draw(c, &view, [1280.0, 720.0], &HashMap::new()).unwrap()
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::PlayFriend(39))
    );
}

#[test]
fn review_realm_list_reveals_the_last_entry() {
    let mut view = friends_view();
    view.realms = (0..40)
        .map(|index| MenuRealmCard {
            name: index.to_string(),
            state: "OPEN".into(),
            target: String::new(),
            address: String::new(),
            owner: "Test".into(),
            online_players: 0,
            max_players: 10,
            days_left: 1,
            expired: false,
            member: false,
        })
        .collect();
    let render = |c: &mut Canvas<'_>| {
        let grid = grid::Grid::new(c.r(1.0), 1280.0);
        play_realms::draw(c, &view, &grid, [0.0, 100.0, 1280.0, 700.0]).unwrap();
    };
    let (scrolls, _, _) = paint(HashMap::new(), render);
    let area = scrolls
        .iter()
        .find(|area| area.max > 0.0)
        .expect("overflow realms need a viewport");
    let (_, hits, _) = paint(HashMap::from([(area.key.clone(), area.max)]), render);
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::SelectRealm(39))
    );
}

#[test]
fn review_short_profile_is_scrollable() {
    let mut view = crate::menu::MenuView::new(true, "Test".into());
    view.auth_state = crate::menu::auth::AuthState::Authenticated;
    view.feeds.profile.loaded = true;
    let (scrolls, _, _) = paint(HashMap::new(), |c| {
        let size = [c.r(60.0), c.r(40.0)];
        profile::draw(c, &view, size, None, &HashMap::new()).unwrap();
    });
    assert!(
        scrolls.iter().any(|area| area.max > 0.0),
        "narrow profile needs scrolling"
    );
}

#[test]
fn review_short_world_settings_are_scrollable() {
    let mut view = crate::menu::MenuView::new(true, "Test".into());
    use protocol::world_control::{Backend, Difficulty, GameMode, Generator, World};
    let mut worlds = crate::local_worlds::WorldsMenu::default();
    worlds.apply(crate::local_worlds::Event::Listed(vec![World {
        id: "test".into(),
        name: "World".into(),
        game_mode: GameMode::Survival,
        generator: Generator::Flat,
        difficulty: Difficulty::Normal,
        backend: Backend::Dragonfly,
        seed: 1,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    }]));
    worlds.update(crate::local_worlds::Input::BeginEdit(0));
    view.local = worlds.view();
    let (scrolls, _, _) = paint(HashMap::new(), |c| {
        world_settings::draw(c, &view, [1280.0, 300.0], crate::local_worlds::Screen::Edit).unwrap()
    });
    assert!(
        scrolls.iter().any(|area| area.max > 0.0),
        "short world settings need scrolling"
    );
}
