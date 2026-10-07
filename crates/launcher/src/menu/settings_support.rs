//! Account and Help Center routes use the platform browser and native dialogs.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportLink {
    Help,
    Attribution,
    LicensedContent,
    Gamertag,
    Account,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportDialog {
    Help,
    FontLicense,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportAction {
    Open(SupportLink),
    Dialog(SupportDialog),
}

impl SupportLink {
    /// Fixed destinations from general_section.json and vanilla's feedback link.
    pub fn url(self) -> &'static str {
        match self {
            Self::Help => "https://aka.ms/MCHelp",
            Self::Attribution => "https://www.minecraft.net/attribution/?hideChrome",
            Self::LicensedContent => "https://www.minecraft.net/licensed-content/?hideChrome",
            Self::Gamertag => "https://social.xbox.com/changegamertag",
            Self::Account => "https://account.xbox.com/Settings",
        }
    }
}

/// Attribution shown for the font bundled with the client.
pub fn font_licenses() -> String {
    "Cinnangles Sans\n\nBundled with Cinnabar.".to_owned()
}
