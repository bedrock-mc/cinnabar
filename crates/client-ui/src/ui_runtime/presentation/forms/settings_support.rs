//! General settings links and Help Center use the pinned pack's authored controls.

use super::menu_screens::{Translate, translated};
use crate::menu::{
    MenuAction,
    settings_support::{SupportAction, SupportDialog, SupportLink},
};
use json_ui::{DataSource, HitRegion, Scalar};

/// Resolves each authored hyperlink control without accepting arbitrary destinations.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    let action = match region.pressed.as_deref() {
        Some("button.feedback_link") => SupportAction::Dialog(SupportDialog::Help),
        Some("button.font_license_popup") => SupportAction::Dialog(SupportDialog::FontLicense),
        Some("change_gamertag_button") => SupportAction::Open(SupportLink::Gamertag),
        Some("manage_account_button") => SupportAction::Open(SupportLink::Account),
        _ => SupportAction::Open(link_for_key(&region.key)?),
    };
    Some(MenuAction::SettingsSupport(action))
}

/// Matches named pack controls, including their inherited internal button region.
fn link_for_key(key: &str) -> Option<SupportLink> {
    let contains = |control| {
        key.split('/')
            .any(|segment| segment.split('@').next() == Some(control))
    };
    if contains("attribution_link_button") {
        Some(SupportLink::Attribution)
    } else if contains("licensed_content_link_button") {
        Some(SupportLink::LicensedContent)
    } else if contains("change_gamertag_button_mobile") {
        Some(SupportLink::Gamertag)
    } else if contains("manage_account_button_mobile")
        || (contains("gamertag_controls") && contains("link_button"))
    {
        Some(SupportLink::Account)
    } else {
        None
    }
}

/// Builds the local-license modal and the matching fallback for the feedback prompt.
pub(super) fn dialog_model(
    dialog: SupportDialog,
    translate: Translate<'_>,
) -> (json_ui::FormModel, MenuAction) {
    let words = |key| translated(translate, key, key);
    let (title, body, button1, button2, confirm) = match dialog {
        SupportDialog::Help => (
            words("feedbackPopup.title"),
            String::new(),
            words("gui.feedbackYes"),
            words("gui.no"),
            MenuAction::SettingsSupport(SupportAction::Open(SupportLink::Help)),
        ),
        SupportDialog::FontLicense => (
            words("options.font_license.name"),
            crate::menu::settings_support::font_licenses(),
            words("gui.close"),
            words("gui.close"),
            MenuAction::DismissDialog,
        ),
    };
    (
        json_ui::FormModel::Modal(json_ui::ModalForm {
            title,
            body,
            button1,
            button2,
        }),
        confirm,
    )
}

/// Populates vanilla's three feedback prompt bindings on the actual rating prompt.
pub(super) fn help_data(data: &mut DataSource, translate: Translate<'_>) {
    data.set_strict(true);
    data.set_global(
        "#title",
        Scalar::Text(translated(
            translate,
            "feedbackPopup.title",
            "feedbackPopup.title",
        )),
    );
    data.set_global(
        "#ButtonName",
        Scalar::Text(translated(translate, "gui.feedbackYes", "gui.feedbackYes")),
    );
    data.set_global("#texture", Scalar::Text("textures/ui/rating_screen".into()));
}
