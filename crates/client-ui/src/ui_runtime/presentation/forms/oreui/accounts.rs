//! The saved-accounts picker as an OreUI modal menu: one item per account (the current
//! one checked), the sign-in status while adding, and the account actions.

use std::collections::HashMap;

use super::super::super::IconRef;
use super::modal::Modal;
use super::theme::{DESTRUCTIVE_TINT, TEXT};
use super::widgets::{MenuItem, Variant};
use ui::{UiNode, UiRect};

use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::paint::Canvas;
use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

/// The picker for `view`, with account pictures from `images` once decoded.
pub(super) fn modal<'a>(view: &'a MenuView, images: &HashMap<String, IconRef>) -> Modal<'a> {
    let busy = view.feeds.account_adding;
    let active = view.feeds.account_active_id.as_deref();
    let items = if busy {
        Vec::new()
    } else {
        view.feeds
            .accounts
            .iter()
            .enumerate()
            .map(|(index, account)| {
                let current = active == Some(account.id.as_str());
                MenuItem {
                    label: &account.gamertag,
                    picture_slot: true,
                    picture: account
                        .picture_path
                        .as_ref()
                        .and_then(|path| images.get(path).copied()),
                    selected: current,
                    enabled: true,
                    action: (!current).then_some(MenuAction::SwitchAccount(index)),
                }
            })
            .collect()
    };
    let (body, body_color) = if busy {
        (status(&view.auth_state).into(), TEXT)
    } else {
        let error = view.feeds.account_error.as_deref().unwrap_or_default();
        (error.into(), DESTRUCTIVE_TINT)
    };
    let first = if busy {
        (
            "Cancel sign-in".into(),
            Variant::Secondary,
            Some(MenuAction::CancelSignIn),
        )
    } else {
        (
            "Add account".into(),
            Variant::Primary,
            Some(MenuAction::AddAccount),
        )
    };
    let profile = (!busy).then_some(MenuAction::Navigate(MenuScreen::Profile));
    Modal {
        title: "Accounts",
        items,
        body,
        body_color,
        buttons: vec![first, ("View profile".into(), Variant::Secondary, profile)],
        close: Some(if busy {
            MenuAction::CancelSignIn
        } else {
            MenuAction::DismissDialog
        }),
    }
}

/// What the sign-in in progress is waiting on.
fn status(auth: &AuthState) -> String {
    match auth {
        AuthState::AwaitingCode { uri, code } => {
            format!("Finish signing in in the browser.\n\n{uri}\n\n{code}")
        }
        AuthState::Failed(reason) => reason.clone(),
        AuthState::Authenticated => "Loading Xbox profile…".into(),
        _ => "Opening Microsoft sign-in…".into(),
    }
}

impl UiPresentationRuntime {
    /// Draws the saved-accounts picker over the current screen; returns its hit targets,
    /// the only ones that then count.
    pub(in super::super) fn append_oreui_accounts(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        let originals = self
            .form_presentation
            .oreui_originals
            .clone()
            .filter(|_| self.form_presentation.oreui_look == super::Look::Originals);
        let picker = modal(view, &self.menu_artwork.refs);
        let rollback = (nodes.len(), *next);
        let mut offsets = self.menu_scrolls.offsets().clone();
        // A newly focused item off screen scrolls into view, then draws again there.
        let mut first = true;
        loop {
            nodes.truncate(rollback.0);
            *next = rollback.1;
            let mut canvas = Canvas::new(
                nodes,
                next,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                originals.as_deref(),
            );
            canvas.offsets = offsets.clone();
            let focused = super::modal::draw(&mut canvas, view, size, &picker)?;
            let (hits, scrolls) = (canvas.hits, canvas.scrolls);
            let used = offsets.get(super::modal::SCROLL).copied().unwrap_or(0.0);
            let revealed = focused.and_then(|item| {
                let rect = |b: super::paint::Bounds| {
                    super::super::super::rect(b[0], b[1], b[2], b[3]).ok()
                };
                Some(self.menu_scrolls.reveal_focus(
                    super::modal::SCROLL,
                    view.focused_action,
                    rect(item.bounds),
                    rect(item.viewport)?,
                    item.max,
                ))
            });
            match revealed {
                Some(offset) if first && (offset - used).abs() > f32::EPSILON => {
                    offsets.insert(super::modal::SCROLL.to_owned(), offset);
                    first = false;
                }
                _ => {
                    self.menu_scrolls.set_areas(scrolls);
                    return Ok(hits);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::review_tests::{paint, solids};
    use super::super::theme::{
        EDGE, MENU_ITEM, MENU_SECONDARY, NEUTRAL, NEUTRAL100, OVERLAY_MODAL, PRIMARY_ROLE,
    };
    use super::*;
    use crate::menu::MenuDialog;

    const SIZE: [f32; 2] = [1280.0, 720.0];

    fn manager_view() -> MenuView {
        let mut view = MenuView::new(true, "First".into());
        view.auth_state = AuthState::Authenticated;
        view.dialog = Some(MenuDialog::Accounts);
        view.feeds.account_active_id = Some("first".into());
        view.feeds.accounts = ["first", "second"]
            .map(|id| launcher::accounts::AccountProfile {
                id: id.into(),
                gamertag: id.into(),
                picture_path: None,
            })
            .into();
        view
    }

    type Fill = ([f32; 4], [u8; 4]);

    /// The picker's fills, hit targets and logical px per rem.
    fn draw(view: &MenuView) -> (Vec<Fill>, Vec<MenuAction>, f32) {
        let mut rem = 0.0;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            rem = canvas.rem;
            super::super::modal::draw(canvas, view, SIZE, &modal(view, &HashMap::new())).unwrap();
        });
        let actions = hits.into_iter().map(|(action, _)| action).collect();
        (solids(&nodes), actions, rem)
    }

    fn near(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01)
    }

    #[test]
    fn picker_switches_only_to_other_accounts_and_checks_the_current_one() {
        let (fills, actions, rem) = draw(&manager_view());
        for action in [
            MenuAction::SwitchAccount(1),
            MenuAction::AddAccount,
            MenuAction::Navigate(MenuScreen::Profile),
            MenuAction::DismissDialog,
        ] {
            assert!(actions.contains(&action), "missing {action:?}: {actions:?}");
        }
        assert!(!actions.contains(&MenuAction::SwitchAccount(0)));
        let edge = EDGE * rem;
        let rows: Vec<[f32; 4]> = fills
            .iter()
            .filter(|(b, color)| {
                *color == MENU_ITEM.fill && (b[3] - b[1] - (4.8 * rem - edge)).abs() < 0.01
            })
            .map(|(b, _)| *b)
            .collect();
        assert_eq!(rows.len(), 2, "{fills:?}");
        let checked = |row: [f32; 4]| {
            fills.iter().any(|(b, color)| {
                *color == TEXT && b[1] >= row[1] && b[3] <= row[3] && b[0] > (row[0] + row[2]) * 0.5
            })
        };
        assert!(checked(rows[0]), "current account carries the check");
        assert!(!checked(rows[1]));
    }

    #[test]
    fn picker_draws_the_vanilla_modal_surfaces() {
        let (fills, _, rem) = draw(&manager_view());
        let edge = EDGE * rem;
        assert_eq!(fills[0], ([0.0, 0.0, SIZE[0], SIZE[1]], OVERLAY_MODAL));
        let width = (47.6 * rem).min(SIZE[0]);
        let left = (SIZE[0] - width) * 0.5 + edge;
        let header = fills
            .iter()
            .find(|(b, color)| *color == NEUTRAL.fill && (b[3] - b[1] - 4.8 * rem).abs() < 0.01)
            .expect("neutral header");
        assert!(
            (header.0[0] - left).abs() < 0.01
                && (header.0[2] - (left + width - edge * 2.0)).abs() < 0.01
        );
        // The menu list sits on the darkest surface, flush under the header.
        assert!(
            fills
                .iter()
                .any(|(b, color)| *color == NEUTRAL100 && (b[1] - header.0[3]).abs() < 0.01)
        );
        // Add account is a primary button; View profile a secondary one outlined in black.
        assert!(fills.iter().any(|(_, color)| *color == PRIMARY_ROLE.fill));
        let profile = fills
            .iter()
            .find(|(_, color)| *color == MENU_SECONDARY.fill)
            .expect("secondary face")
            .0;
        let outline = [
            profile[0] - edge,
            profile[1] - edge,
            profile[2] + edge,
            profile[3] + 0.4 * rem + edge,
        ];
        assert!(
            fills
                .iter()
                .any(|(b, color)| *color == MENU_SECONDARY.border && near(*b, outline)),
            "{fills:?}"
        );
    }

    #[test]
    fn pending_sign_in_shows_no_accounts_and_closing_cancels_it() {
        let mut view = manager_view();
        view.feeds.account_adding = true;
        let (fills, actions, rem) = draw(&view);
        assert!(!actions.iter().any(|action| matches!(
            action,
            MenuAction::SwitchAccount(_)
                | MenuAction::AddAccount
                | MenuAction::DismissDialog
                | MenuAction::Navigate(_)
        )));
        // Both the X and the Cancel sign-in button cancel.
        assert_eq!(
            actions
                .iter()
                .filter(|action| **action == MenuAction::CancelSignIn)
                .count(),
            2
        );
        let row = 4.8 * rem - EDGE * rem;
        assert!(
            !fills
                .iter()
                .any(|(b, color)| *color == MENU_ITEM.fill && (b[3] - b[1] - row).abs() < 0.01)
        );
    }

    #[test]
    fn overflowing_picker_cuts_its_last_visible_account_in_half() {
        let mut view = manager_view();
        view.feeds.accounts = (0..30)
            .map(|index| launcher::accounts::AccountProfile {
                id: index.to_string(),
                gamertag: format!("Player{index}"),
                picture_path: None,
            })
            .collect();
        view.focused_action = Some(MenuAction::SwitchAccount(29));
        let mut rem = 0.0;
        let mut focused = None;
        let (scrolls, _, _) = paint(HashMap::new(), |canvas| {
            rem = canvas.rem;
            focused =
                super::super::modal::draw(canvas, &view, SIZE, &modal(&view, &HashMap::new()))
                    .unwrap();
        });
        let area = scrolls.first().expect("a scrolling list");
        let rows = area.viewport.height() / (4.8 * rem);
        assert!(area.max > 0.0);
        assert!(
            (rows - rows.floor() - 0.5).abs() < 1e-3,
            "{rows} rows visible"
        );
        let focused = focused.expect("the focused account's place");
        assert!(
            focused.bounds[3] > focused.viewport[3],
            "the last account starts off screen"
        );
    }

    #[test]
    fn account_rows_darken_on_hover_and_press() {
        let row_fills = |hovered, pressed| {
            let mut view = manager_view();
            view.hovered = hovered;
            view.pressed = pressed;
            let (fills, _, rem) = draw(&view);
            let row = 4.8 * rem - EDGE * rem;
            fills
                .into_iter()
                .filter(|(b, color)| color[3] == 255 && (b[3] - b[1] - row).abs() < 0.01)
                .map(|(_, color)| color)
                .collect::<Vec<_>>()
        };
        let other = Some(MenuAction::SwitchAccount(1));
        assert_eq!(row_fills(None, None), [MENU_ITEM.fill; 2]);
        assert_eq!(row_fills(other, None), [MENU_ITEM.fill, MENU_ITEM.hovered]);
        assert_eq!(row_fills(other, other), [MENU_ITEM.fill, MENU_ITEM.pressed]);
        // The current account takes no presses, so it never lights up.
        let current = Some(MenuAction::SwitchAccount(0));
        assert_eq!(row_fills(current, current), [MENU_ITEM.fill; 2]);
    }
}
