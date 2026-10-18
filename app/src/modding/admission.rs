//! Shared off-frame preparation of a component and its owner-scoped presentation resources.

use assets::RuntimeFontCatalog;
use client_extensions::modding::font;
use mod_host::{ModGrants, ModHost};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

/// The predecessor fence captured before an off-frame admission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DrawingContext {
    source: [u8; 32],
    session: mod_api::drawing::DrawingSession,
    readiness: GuestReadiness,
    remote: client_extensions::modding::drawing_policy::DrawingGrant,
}

/// Distinguishes a fresh admission request from a running or permanently stopped guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuestReadiness {
    Active,
    Restarting,
    Stopped,
}

impl DrawingContext {
    /// Captures only admitted identity and trusted drawing authority.
    pub(super) fn capture(
        host: &ModHost,
        ui: Option<&client_ui::ui_runtime::UiRuntime>,
    ) -> Option<Self> {
        let grant = ui.and_then(|ui| match &ui.experiences.session.state {
            server_experience::session::State::Granted(grant) => Some(grant),
            _ => None,
        });
        host.drawing_session().map(|session| Self {
            source: host.source_digest(),
            session,
            readiness: if host.needs_drawing_restart() {
                GuestReadiness::Restarting
            } else if host.is_active() {
                GuestReadiness::Active
            } else {
                GuestReadiness::Stopped
            },
            remote: client_extensions::modding::drawing_policy::DrawingGrant::capture(
                grant,
                client_ui::experience_session::unix_seconds(),
            ),
        })
    }

    /// Allows automatic fence recovery only for running or fence-revoked guests.
    pub(super) fn restartable(&self) -> bool {
        self.readiness != GuestReadiness::Stopped
    }

    /// Forces admission after revocation even when the final fence equals the admitted one.
    pub(super) fn needs_restart(&self) -> bool {
        self.readiness == GuestReadiness::Restarting
    }

    /// Authorizes replacement bytes independently against the trusted remote grant.
    pub(super) fn policy(&self, source: [u8; 32]) -> mod_api::drawing::DrawingSession {
        let mut session = self.session;
        if session.connection != 0 && !session.local && self.source != source {
            session.permissions = self.remote.permissions(&source);
        }
        session
    }
}

pub(super) struct PreparedMod {
    pub(super) host: ModHost,
    pub(super) font: Option<Arc<RuntimeFontCatalog>>,
    pub(super) panel_ui: Option<Arc<mod_ui::PreparedPanelUi>>,
    pub(super) branding:
        Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
}

/// A captured preference seed or read failure; absence means settings consent is disabled.
pub(super) type SettingsSeed = Result<Option<mod_host::SettingsSnapshot>, String>;

/// Captures one settings input off-frame, preferring committed preferences to the disk companion.
pub(super) fn settings_seed(
    granted: bool,
    current: Option<&mod_host::SettingsSnapshot>,
    anchor: &Path,
) -> SettingsSeed {
    if !granted {
        Ok(None)
    } else {
        mod_host::SettingsSnapshot::read(anchor, current)
            .map(Some)
            .map_err(|error| format!("{error:#}"))
    }
}

/// Prepares one immutable source snapshot; explicit sidecars override optional embedded data.
/// Settings activation and publication follow only after every resource has been accepted.
pub(super) fn prepare(
    path: &Path,
    component: &[u8],
    grants: ModGrants,
    font_bytes: Option<&[u8]>,
    ui: Option<&[u8]>,
    settings: Option<(&Path, &str)>,
    drawing: Option<DrawingContext>,
) -> Result<PreparedMod, String> {
    let session = drawing.map(|drawing| drawing.policy(Sha256::digest(component).into()));
    let host = ModHost::prepare_snapshot_in_session(path, component, grants, settings, session)
        .map_err(|error| format!("{error:#}"))?;
    prepare_presentation(host, font_bytes, ui)
}

/// Admits a verified package snapshot and its presentation together off the frame thread.
pub(super) fn prepare_package(
    path: &Path,
    package: mod_host::package::Package,
    grants: ModGrants,
    font_bytes: Option<&[u8]>,
    ui: Option<&[u8]>,
    settings: Option<(&Path, &str)>,
    drawing: Option<DrawingContext>,
) -> Result<PreparedMod, String> {
    let session = drawing.map(|drawing| drawing.policy(package.digest));
    let host = ModHost::prepare_package_in_session(path, package, grants, settings, session)
        .map_err(|error| format!("{error:#}"))?;
    prepare_presentation(host, font_bytes, ui)
}

/// Prepares static resources only after the guest's effective grants have been installed.
fn prepare_presentation(
    mut host: ModHost,
    font_bytes: Option<&[u8]>,
    ui: Option<&[u8]>,
) -> Result<PreparedMod, String> {
    let controls = host.grants().controls;
    let font = if controls {
        font_bytes
            .or_else(|| host.panel_font_source())
            .map(font::rasterize)
            .transpose()?
            .map(Arc::new)
    } else {
        None
    };
    if controls && let Some(ui) = ui {
        let json = std::str::from_utf8(ui).map_err(|error| error.to_string())?;
        host.set_panel_bundle(json)
            .map_err(|error| format!("{error:#}"))?;
    }
    let panel_ui = host
        .panel_bundle()
        .map(mod_ui::PreparedPanelUi::prepare)
        .transpose()?;
    let branding = prepare_branding(&host)?;
    Ok(PreparedMod {
        host,
        font,
        panel_ui,
        branding,
    })
}

/// Decodes generic static branding only when the host's active environment grant permits it.
pub(super) fn prepare_branding(
    host: &ModHost,
) -> Result<
    Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
    String,
> {
    if host.menu_logo_source().is_none() && host.menu_title().is_none() {
        return Ok(None);
    }
    client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding::prepare(
        host.menu_logo_source(),
        host.menu_title(),
        [mod_api::MAX_MENU_LOGO_WIDTH, mod_api::MAX_MENU_LOGO_HEIGHT],
        mod_api::MAX_MENU_LOGO_PIXELS,
    )
    .map(|branding| Some(Arc::new(branding)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::drawing::{DrawingPermissions, DrawingSession};

    #[test]
    fn changed_remote_bytes_keep_plain_drawing_without_inheriting_digest_scopes() {
        let context = DrawingContext {
            source: [1; 32],
            session: DrawingSession {
                connection: 1,
                permissions: DrawingPermissions::ALL,
                ..Default::default()
            },
            readiness: GuestReadiness::Active,
            remote: Default::default(),
        };
        assert_eq!(context.policy([1; 32]), context.session);
        let changed = context.policy([2; 32]);
        assert_eq!(
            changed.permissions,
            client_extensions::modding::drawing_policy::remote_permissions(None, &[2; 32], 0)
        );
        let local = DrawingContext {
            session: DrawingSession {
                local: true,
                ..context.session
            },
            ..context
        };
        assert_eq!(local.policy([2; 32]), local.session);
    }
}
