//! A native-only portrait of the admitted local profile, never a default skin.

use std::sync::Arc;

use ui::mod_panel::local_player_head as identity;

use render_api::StandardSkin;
use render_model::UiTexturePage;

use super::UiPresentationRuntime;

/// The current authority identity and its extracted portrait. Skin allocations are shared.
#[derive(Default)]
pub(super) struct LocalPlayerIdentity {
    source: Option<Source>,
    portrait: Option<UiTexturePage>,
}

#[derive(PartialEq, Eq)]
struct Source {
    session: u64,
    actor: u64,
    name: Arc<str>,
    skin: StandardSkin,
}

impl LocalPlayerIdentity {
    /// Re-extracts on profile, skin, actor or session changes, and drops absent identities.
    fn observe(&mut self, source: Option<Source>) {
        if self.source == source {
            return;
        }
        self.portrait = source
            .as_ref()
            .and_then(|source| render_model::actor::skin_portrait::extract(&source.skin));
        self.source = source;
    }

    /// Discards a profile captured for an earlier session before any panel paints.
    fn observe_session(&mut self, session: u64) {
        if self
            .source
            .as_ref()
            .is_some_and(|source| source.session != session)
        {
            self.observe(None);
        }
    }

    /// A name accompanies only a usable portrait, with a neutral fallback otherwise.
    fn name(&self) -> &str {
        self.source
            .as_ref()
            .filter(|_| self.portrait.is_some())
            .map_or(identity::DEFAULT_NAME, |source| source.name.as_ref())
    }
}

impl UiPresentationRuntime {
    /// Captures only the current world's admitted local profile. Missing or unsupported
    /// skins clear the old identity; launcher sample/default skins are never substituted.
    pub(super) fn capture_local_player_identity(
        &mut self,
        stream: Option<&chunk_pipeline::WorldStream>,
        session: u64,
    ) {
        let source = stream.and_then(|stream| {
            let actor = stream.local_player_runtime_id();
            let profile = stream.authority().actor_player_profile(actor)?;
            let protocol::PlayerSkin::Standard(skin) = &profile.skin else {
                return None;
            };
            if profile.username.trim().is_empty() {
                return None;
            }
            Some(Source {
                session,
                actor,
                name: Arc::clone(&profile.username),
                skin: skin.clone(),
            })
        });
        self.local_player_identity.observe(source);
    }

    /// Installs the current portrait in whichever atlas owns this frame's engine, then
    /// publishes read-only bindings after all extension-owned values have been prepared.
    pub(super) fn publish_local_player_identity(&mut self, session: u64) {
        self.local_player_identity.observe_session(session);
        if let Some(engine) = self.engine.as_deref() {
            engine
                .textures
                .lock()
                .set_local_player_head(self.local_player_identity.portrait.as_ref());
        }
        self.screens.mod_ui.set_local_player_panel_identity(
            self.local_player_identity.name(),
            self.local_player_identity.portrait.is_some(),
        );
    }
}

#[cfg(test)]
mod tests;
