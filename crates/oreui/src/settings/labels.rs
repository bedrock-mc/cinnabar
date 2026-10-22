//! Displayed option labels and help text shared by category rows and search.

use super::Content;
use launcher::menu::settings_options::SettingId;

impl Content<'_, '_> {
    /// Resolves the same translated labels and client help text used by editable rows.
    pub(super) fn option_text(&self, id: SettingId) -> (String, String) {
        let option = id.definition();
        let label_key = if id == SettingId::GuiScale {
            "options.guiScale.optionName.name"
        } else {
            option.oreui_label.unwrap_or(option.label)
        };
        let title = self.word(label_key);
        let base_key = label_key.strip_suffix(".name").unwrap_or(label_key);
        let description_key = match id {
            SettingId::KeyboardMouseInvertYAxis => {
                "options.invertYAxis.Mouse.description".to_owned()
            }
            SettingId::ControllerInvertYAxis => "options.invertYAxis.description".to_owned(),
            _ => format!("{base_key}.description"),
        };
        let description =
            (self.translate)(&description_key).map_or_else(String::new, |text| text.to_string());
        let description = if id == SettingId::ChunkOcclusionCulling {
            "Skip terrain hidden behind other blocks. Visible terrain stays available.".to_owned()
        } else if id == SettingId::GraphicsApi {
            "Restart required. Auto checks driver stability before choosing an API. WGPU_BACKEND takes priority over this setting.".to_owned()
        } else if id == SettingId::Vrr {
            "Automatic uses display detection. Choose On if variable refresh is enabled in your display and system settings, or Off to disable the VRR frame cap.".to_owned()
        } else if id == SettingId::ViewBobbing {
            "Minimal bobs only the hand; Full also bobs the camera.".to_owned()
        } else if id == SettingId::CinnabarVisible {
            "Let other Cinnabar players see a Cinnabar badge next to your name.".to_owned()
        } else if id == SettingId::CinnabarBadges {
            "Show Cinnabar badges beside player names and in the player list.".to_owned()
        } else if id == SettingId::AutoUpdateEnabled {
            "Check the selected channel and download verified updates in the background.".to_owned()
        } else if id == SettingId::InstallUpdatesOnQuit {
            "Install a downloaded update and restart when you quit. Later defers it for this session.".to_owned()
        } else if id == SettingId::UpdateChannel {
            "Beta for regular releases. Nightly for the latest changes.".to_owned()
        } else if id == SettingId::ScreenAnimations {
            "Smooth highlights, button presses and screen transitions. Turn off for an instant interface.".to_owned()
        } else if id == SettingId::VerboseLogging {
            "Write detailed network, session, core and resource pack logs. Applies now and at launch. CINNABAR_LOG or RUST_LOG take priority when set.".to_owned()
        } else if id == SettingId::RecordNetworkSession {
            "Save each new server session's packets to the logs folder. The recording contains chat messages and player names. It stays on this device and is never uploaded.".to_owned()
        } else if id == SettingId::SendCrashReports {
            "Send each crash report to the Cinnabar team when the game stops unexpectedly: the error, your device and graphics driver, and the last log lines with names, servers and addresses removed. Uses a random install ID and keeps reports for 90 days. Off sends nothing unless you choose Send report.".to_owned()
        } else if description.is_empty() {
            let fallback = super::sections::fallback(&description_key);
            if fallback == description_key {
                String::new()
            } else {
                fallback.to_owned()
            }
        } else {
            description
        };
        (title, description)
    }
}
