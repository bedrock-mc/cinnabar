use crate::vanilla::Variable;
use std::{collections::BTreeMap, sync::Arc};

use serde_json::Value;

use crate::{Catalog, env::Env};

/// Screen context: the compile-time flags (`$desktop_screen`, `$touch`, …) and any
/// extra variables that gate `ignored`/`variables[]` selection and `$var` values.
///
/// Clones share the variables, so retained screens compare an unchanged context by
/// pointer instead of walking every variable each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub(crate) vars: Arc<BTreeMap<String, Value>>,
}

impl Context {
    /// An empty context. As in the vanilla client, an unset `$flag` reads as its
    /// literal text: `ignored` keeps the control and a `requires` block applies,
    /// while inside an expression it is null.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A non-touch desktop screen context.
    pub fn desktop() -> Self {
        Self::empty()
            .with_flag(Variable::DesktopScreen.key(), true)
            .with_flag(Variable::PocketScreen.key(), false)
            .with_flag(Variable::Touch.key(), false)
    }

    /// The desktop context plus the globals a retail, full-game, non-edu desktop
    /// client computes in code, false ones included.
    pub fn retail(macos: bool) -> Self {
        let platform: &[(&str, bool)] = &[
            (Variable::Win10Edition.key(), !macos),
            (Variable::MicrosoftOs.key(), !macos),
            (Variable::MsPlatform.key(), !macos),
            (Variable::OsxEdition.key(), macos),
            (Variable::AppleOs.key(), macos),
        ];
        let constant: &[(&str, bool)] = &[
            (Variable::IsDesktop.key(), true),
            ("mouse", true),
            (Variable::IsPublish.key(), true),
            (Variable::TestInfrastructureDisabled.key(), true),
            (Variable::NewVideoSettings.key(), true),
            (
                Variable::IsImproveInputResponsePlatformSupported.key(),
                true,
            ),
            (Variable::IsXboxliveEnabled.key(), true),
            (Variable::IsRealmsEnabled.key(), true),
            (Variable::IsSeedsEnabled.key(), true),
            ("is_creative_enabled", true),
            ("is_multiplayer_enabled", true),
            ("is_packs_enabled", true),
            ("is_server_enabled", true),
            ("is_store_enabled", false),
            (Variable::FilePickingSupported.key(), true),
            (Variable::SupportsClipboardSet.key(), true),
            (Variable::SupportsAddFriend.key(), true),
            (Variable::SupportsXblAchievements.key(), true),
            // Channel flags: this is the release app, not Preview.
            (Variable::PreRelease.key(), false),
            (Variable::BetaBuild.key(), false),
            (Variable::IsPreviewApp.key(), false),
            (Variable::Trial.key(), false),
            (Variable::EducationEdition.key(), false),
            (Variable::StoreDisabled.key(), true),
            (Variable::CreatorBuild.key(), false),
            (Variable::PocketEdition.key(), false),
            (Variable::ConsoleEdition.key(), false),
            (Variable::IsConsole.key(), false),
            ("game_pad", false),
            (Variable::CanSplitscreen.key(), false),
            (Variable::IsSecondaryClient.key(), false),
            (Variable::RequiresXblSigninToPlay.key(), false),
            (Variable::IsEditorModeEnabled.key(), false),
            (Variable::CanQuit.key(), true),
            (Variable::WorldArchiveSupport.key(), true),
            ("is_dynamic_textures_platform_supported", true),
            (Variable::IsPregame.key(), false),
            (Variable::ScreenTransitionsEnabled.key(), false),
            (Variable::UseNormalizedFontSize.key(), false),
            ("image_picking_not_supported", false),
            ("vibration_supported", false),
            (Variable::SupportsShare.key(), false),
            (Variable::HideXboxLiveIcon.key(), false),
            (Variable::DisableGamertagControls.key(), false),
            ("multiplayer_requires_live_gold", false),
            (Variable::DeviceMustBeRemovedForXblSignin.key(), false),
            (Variable::IsLowMemoryDevice.key(), false),
            (Variable::Ignore3rdPartyServers.key(), false),
            (Variable::IgnoreAddServers.key(), false),
            (Variable::IsOn3pServer.key(), false),
            (Variable::IsEditorPlaytestRoundtrip.key(), false),
            (Variable::EduSaveToCloudOn.key(), false),
            (Variable::EduSaveToCloudGeneralToggleOn.key(), false),
            (Variable::BuiltWithOreUiDocsAndTests.key(), false),
            // Other platforms and devices.
            ("build_platform_UWP", false),
            (Variable::GoogleOs.key(), false),
            (Variable::IsIos.key(), false),
            (Variable::IsAndroid.key(), false),
            ("is_chromebook", false),
            ("fire_tv", false),
            (Variable::NxOs.key(), false),
            (Variable::IsPs4.key(), false),
            (Variable::IsPs5.key(), false),
            (Variable::XboxOne.key(), false),
            (Variable::Thirdpartyconsole.key(), false),
            ("is_settopbox", false),
            ("is_win10_arm", false),
            ("is_windows_10_mobile", false),
            ("is_mobile_vr", false),
            ("gear_vr", false),
            ("oculus_rift", false),
            ("psvr", false),
            (Variable::IsHolographic.key(), false),
            ("supports_hand_controllers", false),
            ("is_living_room_mode", false),
            ("is_reality_mode", false),
        ];
        let context = platform
            .iter()
            .chain(constant)
            .fold(Self::desktop(), |context, (name, value)| {
                context.with_flag(name, *value)
            });
        // The safe zone at the desktop defaults (safe zone 1, screen position 0)
        // sizes every buffer zero along its axis.
        let vertical = || serde_json::json!(["100%", 0]);
        let horizontal = || serde_json::json!([0, "100%"]);
        context
            .with_var(Variable::TopVerticalSafezoneSize.key(), vertical())
            .with_var(Variable::BottomVerticalSafezoneSize.key(), vertical())
            .with_var(Variable::LeftHorizontalSafezoneSize.key(), horizontal())
            .with_var(Variable::RightHorizontalSafezoneSize.key(), horizontal())
    }

    /// The variables set so far, keyed without `$`.
    pub fn vars(&self) -> &BTreeMap<String, Value> {
        &self.vars
    }

    /// Set a boolean flag (stored under `name`, without a `$`).
    pub fn with_flag(self, name: &str, value: bool) -> Self {
        self.with_var(name, Value::Bool(value))
    }

    /// Set an arbitrary variable value. Writing the value already held keeps the
    /// variables shared with every clone.
    pub fn with_var(mut self, name: &str, value: Value) -> Self {
        if self.vars.get(name) != Some(&value) {
            Arc::make_mut(&mut self.vars).insert(name.to_owned(), value);
        }
        self
    }

    pub(crate) fn root_env(&self, catalog: &Catalog) -> Env {
        let mut env = Env::new();
        for (name, value) in catalog.globals() {
            env.set(name.clone(), value.clone());
        }
        for (name, value) in self.vars.iter() {
            env.set(name.clone(), value.clone());
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::Value;

    use super::{Context, Variable};

    #[test]
    fn clones_share_variables_until_a_write_changes_one() {
        let base = Context::desktop();
        let same = base.clone().with_flag(Variable::Touch.key(), false);
        assert!(Arc::ptr_eq(&base.vars, &same.vars));
        assert_eq!(base, same);
        let changed = base.clone().with_flag("cinnabar_chat_top", true);
        assert!(!Arc::ptr_eq(&base.vars, &changed.vars));
        assert_eq!(
            base,
            Context::desktop(),
            "the shared variables stay unchanged"
        );
        assert_eq!(
            changed.vars().get("cinnabar_chat_top"),
            Some(&Value::Bool(true))
        );
        assert_ne!(base, changed);
        let flipped = same.with_flag(Variable::Touch.key(), true);
        assert_eq!(
            flipped.vars().get(Variable::Touch.key()),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            base.vars().get(Variable::Touch.key()),
            Some(&Value::Bool(false))
        );
    }
}
