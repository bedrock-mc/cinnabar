use super::*;
use crate::review_tests::paint;
use launcher::menu::view::MenuFriendCard;
use std::collections::HashMap;

/// Search is editable and filtered rows retain the original catalog's join identity.
#[test]
fn search_has_an_input_target_and_filters_without_renumbering_friends() {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Friends;
    view.friends = vec![
        MenuFriendCard {
            gamertag: "Player One".into(),
            world_name: String::new(),
            members: String::new(),
            xuid: String::new(),
            max_members: 0,
        },
        MenuFriendCard {
            gamertag: "Player Two".into(),
            world_name: String::new(),
            members: String::new(),
            xuid: String::new(),
            max_members: 0,
        },
    ]
    .into();
    for query in ["", "tWo", "nobody"] {
        view.friend_query = query.into();
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1920.0, 1080.0]).unwrap();
        });
        let actions: Vec<_> = hits.into_iter().map(|(action, _)| action).collect();
        assert!(actions.contains(&MenuAction::SearchFriends));
        assert_eq!(
            actions.contains(&MenuAction::PlayFriend(0)),
            query.is_empty()
        );
        assert_eq!(
            actions.contains(&MenuAction::PlayFriend(1)),
            query != "nobody"
        );
    }
}
