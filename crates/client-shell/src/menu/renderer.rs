//! The Video renderer picker: Off, Enhanced and the discovered Java shader packs. Choices
//! for renderers this build does not compile are ignored.

use super::{MenuRuntime, RenderMode};
use launcher::menu::RendererChoice;

impl MenuRuntime {
    /// Replaces the discovered shader-pack names the picker offers, in display order. Names
    /// past `u16::MAX` are dropped so every listed pack stays addressable by its choice.
    pub fn sync_shader_packs(&mut self, mut names: Vec<String>) {
        names.truncate(usize::from(u16::MAX));
        if self.shader_packs[..] != names[..] {
            self.shader_packs = names.into();
        }
    }

    /// Mirrors the applied pack name; a pending menu choice wins until taken.
    pub fn sync_shader_pack(&mut self, applied: Option<&str>) {
        if self.shader_pack_request.is_none() && self.shader_pack.as_deref() != applied {
            self.shader_pack = applied.map(str::to_owned);
        }
    }

    /// Consume the pack name the picker selected. Its [`RenderMode::ShaderPack`] switch, when
    /// the mode changed too, arrives through [`Self::take_render_mode_request`].
    pub fn take_shader_pack_request(&mut self) -> Option<String> {
        self.shader_pack_request.take()
    }

    /// The highlighted choice; `None` while the selected pack is not in the synced list.
    pub(super) fn renderer_choice(&self) -> Option<RendererChoice> {
        match self.render_mode {
            RenderMode::Vanilla => Some(RendererChoice::Vanilla),
            RenderMode::Enhanced => Some(RendererChoice::Enhanced),
            RenderMode::ShaderPack => {
                let selected = self.shader_pack.as_deref()?;
                let index = self.shader_packs.iter().position(|name| name == selected)?;
                u16::try_from(index).ok().map(RendererChoice::ShaderPack)
            }
        }
    }

    /// Queues the mode and pack requests for a picker press on a compiled renderer.
    pub(super) fn select_renderer(&mut self, choice: RendererChoice) {
        let mode = match choice {
            RendererChoice::Vanilla
                if render_model::ENHANCED_RENDERING_ENABLED
                    || render_model::SHADER_PACKS_ENABLED =>
            {
                RenderMode::Vanilla
            }
            RendererChoice::Enhanced if render_model::ENHANCED_RENDERING_ENABLED => {
                RenderMode::Enhanced
            }
            RendererChoice::ShaderPack(index) if render_model::SHADER_PACKS_ENABLED => {
                let Some(name) = self.shader_packs.get(usize::from(index)) else {
                    return;
                };
                self.shader_pack = Some(name.clone());
                self.shader_pack_request = Some(name.clone());
                RenderMode::ShaderPack
            }
            _ => return,
        };
        if self.render_mode != mode {
            self.render_mode = mode;
            self.render_mode_request = Some(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    fn menu() -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned(), "Lumen".to_owned()]);
        menu
    }

    #[test]
    fn picking_a_pack_requests_shader_pack_mode_and_its_name_only_when_compiled() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(1)));
        if render_model::SHADER_PACKS_ENABLED {
            assert_eq!(
                menu.take_render_mode_request(),
                Some(RenderMode::ShaderPack)
            );
            assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Lumen"));
            assert_eq!(menu.view().renderer, Some(RendererChoice::ShaderPack(1)));
            menu.activate(MenuAction::SelectRenderer(RendererChoice::Vanilla));
            assert_eq!(menu.take_render_mode_request(), Some(RenderMode::Vanilla));
            assert_eq!(menu.take_shader_pack_request(), None);
            assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
        } else {
            assert_eq!(menu.take_render_mode_request(), None);
            assert_eq!(menu.take_shader_pack_request(), None);
            assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
        }
        assert_eq!(menu.take_render_mode_request(), None);
    }

    #[test]
    fn reselecting_the_saved_pack_requests_it_again() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        menu.select_renderer(RendererChoice::ShaderPack(1));
        assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Lumen"));
    }

    #[test]
    fn picking_enhanced_follows_the_enhanced_feature() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::Enhanced));
        let expected = render_model::ENHANCED_RENDERING_ENABLED.then_some(RenderMode::Enhanced);
        assert_eq!(menu.take_render_mode_request(), expected);
        assert_eq!(
            menu.view().render_mode,
            expected.unwrap_or(RenderMode::Vanilla)
        );
    }

    #[test]
    fn missing_packs_are_ignored_and_the_selection_follows_its_name() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(7)));
        assert_eq!(menu.take_render_mode_request(), None);
        assert_eq!(menu.take_shader_pack_request(), None);
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        assert_eq!(menu.view().renderer, Some(RendererChoice::ShaderPack(1)));
        menu.sync_shader_packs(vec!["Lumen".to_owned()]);
        assert_eq!(menu.view().renderer, Some(RendererChoice::ShaderPack(0)));
        assert_eq!(menu.view().shader_packs[..], ["Lumen".to_owned()]);
        menu.sync_shader_packs(Vec::new());
        assert_eq!(menu.view().renderer, None);
    }

    #[test]
    fn a_pending_pack_choice_survives_mirroring_until_taken() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(0)));
        menu.sync_render_mode(RenderMode::Vanilla);
        menu.sync_shader_pack(None);
        assert_eq!(menu.view().renderer, Some(RendererChoice::ShaderPack(0)));
        assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Dusk.zip"));
        assert_eq!(
            menu.take_render_mode_request(),
            Some(RenderMode::ShaderPack)
        );
        menu.sync_render_mode(RenderMode::Vanilla);
        menu.sync_shader_pack(None);
        assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
    }
}
