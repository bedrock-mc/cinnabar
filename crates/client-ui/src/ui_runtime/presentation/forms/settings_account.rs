//! Account section bindings reuse the launcher's current authentication state.

use json_ui::{DataSource, HitRegion, Scalar};

use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

/// Fill the signed-in and signed-out branches in general_section.account_section.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let signed_in = matches!(view.auth_state, AuthState::Authenticated);
    data.set_global("#logged_in", Scalar::Bool(signed_in));
    data.set_global("#not_logged_in", Scalar::Bool(!signed_in));
    data.set_global("#gamertag_label", Scalar::Text(view.display_name.clone()));
    data.set_global("#ad_account_name", Scalar::Text(view.display_name.clone()));
    data.set_global("#player_name", Scalar::Text(view.display_name.clone()));
}

/// Route the pack's account button names to the existing launcher operations.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    Some(match region.pressed.as_deref()? {
        "sign_in_button" | "button.switch_accounts" => MenuAction::StartSignIn,
        "sign_out_button" | "button.sign_out" => MenuAction::SignOut,
        "realms_invites_button" => MenuAction::Navigate(MenuScreen::Social),
        _ => return None,
    })
}
