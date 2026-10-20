//! Anonymous completed Profile presentation through the production painter.
//! Empty artwork paths deliberately show fallback art, not acquired avatar/screenshot success.

use super::Fixture;
use bridge::{ProfileAchievement, ProfileAchievements, ProfileStatistics};
use launcher::menu::{MenuProfile, MenuScreen, MenuView, ProfileTab, auth::AuthState};

/// Shows supported profile summaries and achievements without account or image service work.
pub(super) fn overview() -> Fixture {
    let mut view = MenuView::new(true, "ExamplePlayer".into());
    view.screen = MenuScreen::Profile;
    view.auth_state = AuthState::Authenticated;
    view.feeds.profile = MenuProfile {
        loaded: true,
        xuid: "0".into(),
        gamertag: view.display_name.clone(),
        presence: "Exploring an example world".into(),
        friends: Some(0),
        followers: Some(12),
        statistics_loaded: true,
        statistics: Some(ProfileStatistics {
            minutes_played: Some("1501".into()),
            blocks_broken: Some("12345.75".into()),
            mobs_defeated: Some("0".into()),
            distance_travelled: None,
        }),
        achievements_loaded: true,
        achievements: Some(ProfileAchievements {
            unlocked: 1,
            total: 2,
            current_gamerscore: Some(10),
            max_gamerscore: Some(30),
            entries: vec![
                ProfileAchievement {
                    id: "completed-fixture".into(),
                    name: "Explore an example world".into(),
                    description:
                        "Complete an original milestone in this anonymous profile preview.".into(),
                    gamerscore: Some(10),
                    date_unlocked: "2026-01-02T12:00:00Z".into(),
                    ..Default::default()
                },
                ProfileAchievement {
                    id: "locked-fixture".into(),
                    name: "Build an example shelter".into(),
                    description: "An original locked milestone without a suggested order.".into(),
                    gamerscore: Some(20),
                    locked: true,
                    ..Default::default()
                },
            ],
        }),
        avatar_loaded: true,
        avatar_error: true,
        featured_screenshot_loaded: true,
        featured_screenshot_error: true,
        ..Default::default()
    };
    Fixture::Menu(Box::new(view))
}

/// Uses the same completed presentation on Stats, preserving zero and absent statistic values.
pub(super) fn statistics() -> Fixture {
    let Fixture::Menu(mut view) = overview() else {
        unreachable!("the profile overview is a menu fixture")
    };
    view.profile_tab = ProfileTab::Stats;
    Fixture::Menu(view)
}

/// Keeps the loaded overview and genuine zero count after optional facets finish unavailable.
pub(super) fn partial(tab: ProfileTab) -> Fixture {
    let Fixture::Menu(mut view) = overview() else {
        unreachable!("the profile overview is a menu fixture")
    };
    view.profile_tab = tab;
    let profile = &mut view.feeds.profile;
    profile.followers = None;
    profile.statistics = None;
    profile.statistics_error = true;
    profile.achievements = None;
    profile.achievements_error = true;
    Fixture::Menu(view)
}
