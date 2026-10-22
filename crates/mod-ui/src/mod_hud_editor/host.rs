//! Host-owned editor lifecycle, independent of extension panels.
use super::*;
impl HudEditor {
    /// Creates a validated layout draft; callers own input and persistence of its result.
    pub fn new(preview: &Hud) -> Result<Self, String> {
        preview.validate()?;
        let mut preview = preview.clone();
        for card in &mut preview.cards {
            publication::prepare_card(card);
        }
        let hidden: std::collections::HashSet<_> = preview
            .cards
            .iter()
            .filter(|card| !card.visible)
            .map(|card| card.id.clone())
            .collect();
        let mut editor = HudEditor {
            autosave: preview.autosave,
            draft: preview.clone(),
            history: history::History::new(&preview, &hidden),
            committed_hidden: hidden.clone(),
            hidden,
            toast: Default::default(),
            anchor_motion: None,
            committed: preview,
            close_requested: false,
            reset: false,
            history_buttons: true,
            chrome_scale: 1.,
            grid: false,
            snap: false,
            bypass_snap: false,
            data_dirty: true,
            snap_dirty: false,
            row_icons: Vec::new(),
            icons: Vec::new(),
            keep_out: Vec::new(),
            selected: None,
            hovered: None,
            drag: None,
            slider: None,
            control_fills: Vec::new(),
            open: true,
            result: None,
            viewport: [0.; 2],
            inspector: Default::default(),
            caption_width: 64.,
            caption_scale_width: 22.,
            caption_bounds: None,
            catalog: None,
            screen: CachedScreen::default(),
            data: Arc::new(DataSource::new()),
            frame: None,
            view: ViewState::default(),
            transitions: Default::default(),
        };
        editor.snap = true;
        Ok(editor)
    }
    /// Current artwork and placement, shared by native previews and the editor chrome.
    pub fn draft(&self) -> &Hud {
        &self.draft
    }
    /// Actual GUI-pixel bounds used by preview painting, hit testing and alignment guides.
    pub fn rectangles(&self) -> impl Iterator<Item = (&str, [f64; 4])> {
        self.draft.cards.iter().map(|card| {
            let at = cards::origin(card, self.viewport);
            let size = cards::dimensions(card);
            (card.id.as_str(), [at[0], at[1], size[0], size[1]])
        })
    }
    /// Last painted action bounds in window-logical coordinates for scripted captures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn action_bounds(&self, action: &str) -> Option<[f32; 4]> {
        let frame = self.frame.as_ref()?;
        let hit = frame
            .hits
            .iter()
            .rev()
            .find(|hit| hit.pressed.as_deref() == Some(action))?;
        Some([
            frame.origin[0] + hit.rect.x as f32 * frame.scale,
            frame.origin[1] + hit.rect.y as f32 * frame.scale,
            hit.rect.w as f32 * frame.scale,
            hit.rect.h as f32 * frame.scale,
        ])
    }
    /// Whether this draft still owns input.
    pub fn is_open(&self) -> bool {
        self.open
    }
    /// The enabled action under a window-logical point, in the editor's dispatch order.
    pub fn action_at(&self, at: [f32; 2]) -> Option<&str> {
        let frame = self.frame.as_ref()?;
        let point = super::slider::frame_point(frame, at);
        frame
            .hits
            .iter()
            .rev()
            .find(|hit| hit.enabled && hit.pressed.is_some() && hit.contains(point))?
            .pressed
            .as_deref()
    }
    /// Routes a host pointer without exposing it to extension controls.
    pub fn host_pointer(&mut self, position: [f32; 2], pressed: bool, held: bool) {
        self.pointer(position, pressed, held, &mut []);
    }
    /// Temporarily bypasses both grid and alignment snapping while Shift is held.
    pub fn set_snap_bypass(&mut self, held: bool) {
        self.snap_dirty |= self.bypass_snap != held;
        self.data_dirty |= self.bypass_snap != held;
        self.bypass_snap = held;
        if held && let Some(drag) = &mut self.drag {
            drag.guides = [None; 2];
            drag.targets = [None; 2];
        }
    }
    /// Routes a host key and its physical editing modifiers.
    pub fn host_key(&mut self, key: &str, modifiers: ui::mod_panel::FieldModifiers) {
        self.key_with_modifiers(key, modifiers);
    }
    /// Rolls back a cancelled pointer gesture while retaining the rest of the draft.
    pub fn cancel_pointer(&mut self) {
        self.cancel_pointer_input();
    }
    /// Ends the draft without applying it.
    pub fn cancel(&mut self) {
        self.finish(false);
    }
    /// Delivers the completed Save or Cancel exactly once.
    pub fn take_result(&mut self) -> Option<EditorResult> {
        self.result.take()
    }
}
