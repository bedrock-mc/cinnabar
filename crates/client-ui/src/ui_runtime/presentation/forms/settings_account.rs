//! Account section bindings reuse the launcher's current authentication state.

use json_ui::{DataSource, HitRegion, Scalar};

use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

/// Fill the signed-in and signed-out branches in general_section.account_section.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let signed_in = matches!(view.auth_state, AuthState::Authenticated);
    data.set_global("#logged_in", Scalar::Bool(signed_in));
    data.set_global("#not_logged_in", Scalar::Bool(!signed_in));
    data.set_global(
        "#gamertag_label",
        Scalar::Text(super::accounts::current_name(view).to_owned()),
    );
    data.set_global("#ad_account_name", Scalar::Text(view.display_name.clone()));
    data.set_global("#player_name", Scalar::Text(view.display_name.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_settings_bind_the_authenticated_profile_name() {
        let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
        view.auth_state = AuthState::Authenticated;
        view.feeds.profile.gamertag = "Fixture player".into();
        let mut data = DataSource::new();
        bind(&view, &mut data);
        let label = json_ui::ResolvedControl {
            name: "gamertag".into(),
            control_type: Some("label".into()),
            base: None,
            unresolved_base: None,
            properties: [
                ("text".into(), serde_json::json!("#gamertag_label")),
                (
                    "bindings".into(),
                    serde_json::json!([{"binding_name":"#gamertag_label"}]),
                ),
            ]
            .into(),
            children: Vec::new(),
            factory: None,
        };
        let bound = json_ui::bind(&label, &data, &json_ui::EmptyLibrary);
        assert_eq!(bound.properties["text"], "Fixture player");
    }
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
