//! Host-owned consent and status chrome, resolved from a private JSON-UI catalog.

use json_ui::{Catalog, CollectionItem, Context, DataSource, Scalar, ViewState};
use server_experience::policy::Choice;
use std::sync::Arc;
use ui::FONT_DESIGN_PIXEL_TEXELS;
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{FormEngine, ScreenArt, screen::PaintFrame},
};

use crate::ModUi;

const TEMPLATE: &[u8] = include_bytes!("experience.json");

pub(super) struct ExperienceChrome {
    text: String,
    prompt: bool,
    labels: String,
    failed: bool,
    catalog: Arc<Catalog>,
    screen: CachedScreen,
    frame: Option<EngineFrame>,
    view: ViewState,
    reviewed: bool,
}

impl ModUi {
    /// Remote packs cannot replace this catalog or its button actions.
    pub fn set_experience_chrome(
        &mut self,
        text: Option<&str>,
        prompt: bool,
    ) -> Result<(), String> {
        let Some(text) = text else {
            self.experience = None;
            return Ok(());
        };
        if let Some(chrome) = self.experience.as_mut() {
            if chrome.prompt != prompt || chrome.text != text {
                chrome.frame = None;
                chrome.view = ViewState::default();
                chrome.reviewed = false;
            }
            chrome.text = text.to_owned();
            chrome.prompt = prompt;
            return Ok(());
        }
        let catalog = Catalog::from_files([
            ("ui/_global_variables.json", &b"{}"[..]),
            (
                "ui/_ui_defs.json",
                &br#"{"ui_defs":["ui/cinnabar_experience.json"]}"#[..],
            ),
            ("ui/cinnabar_experience.json", TEMPLATE),
        ])
        .map_err(|error| error.to_string())?;
        self.experience = Some(ExperienceChrome {
            text: text.to_owned(),
            prompt,
            labels: String::new(),
            failed: false,
            catalog: Arc::new(catalog),
            screen: CachedScreen::default(),
            frame: None,
            view: ViewState::default(),
            reviewed: false,
        });
        Ok(())
    }

    /// Keeps guest labels in a separate, explicitly untrusted area below the status.
    pub fn set_experience_labels(&mut self, labels: &str) {
        if let Some(chrome) = self.experience.as_mut() {
            chrome.labels = labels.to_owned();
        }
    }

    /// Keyboard approval is possible only after a consent frame reached presentation.
    pub fn experience_prompt_visible(&self) -> bool {
        self.experience
            .as_ref()
            .is_some_and(|chrome| chrome.prompt && !chrome.failed && chrome.frame.is_some())
    }

    /// Approval stays disabled until the full disclosure can be reached and its last page was drawn.
    pub fn experience_approval_ready(&self) -> bool {
        self.experience_prompt_visible()
            && self
                .experience
                .as_ref()
                .is_some_and(|chrome| chrome.reviewed)
    }

    /// Scrolls only the trusted disclosure viewport; it never changes the underlying menu.
    pub fn scroll_experience(&mut self, pages: f64) {
        let Some(chrome) = self.experience.as_mut() else {
            return;
        };
        if !chrome.prompt {
            return;
        }
        let Some(frame) = &chrome.frame else {
            return;
        };
        for (key, metrics) in &frame.report.scrolls {
            chrome.view.scroll.insert(
                key.clone(),
                (metrics.offset + pages * metrics.viewport * 0.8).clamp(0.0, metrics.max_offset()),
            );
        }
    }

    /// Lights the popup button under the pointer, as vanilla's light buttons do on hover.
    pub fn hover_experience(&mut self, point: Option<[f32; 2]>) {
        let Some(chrome) = self.experience.as_mut() else {
            return;
        };
        let Some(frame) = chrome.frame.as_ref().filter(|_| chrome.prompt) else {
            return;
        };
        let hovered = point.and_then(|point| {
            let point = [
                f64::from((point[0] - frame.origin[0]) / frame.scale),
                f64::from((point[1] - frame.origin[1]) / frame.scale),
            ];
            frame
                .hits
                .iter()
                .rev()
                .find(|region| region.enabled && region.pressed.is_some() && region.contains(point))
                .map(|region| region.key.clone())
        });
        chrome.view.hovered = hovered;
    }

    /// Missing trusted chrome revokes remote code instead of running it invisibly.
    pub fn experience_chrome_failed(&self) -> bool {
        self.experience.as_ref().is_some_and(|chrome| chrome.failed)
    }

    /// Maps only hits from the last host-owned consent screen to local choices.
    pub fn experience_choice(&self, point: [f32; 2]) -> Option<Choice> {
        let chrome = self.experience.as_ref()?;
        if !chrome.prompt || chrome.failed {
            return None;
        }
        let frame = chrome.frame.as_ref()?;
        let point = [
            f64::from((point[0] - frame.origin[0]) / frame.scale),
            f64::from((point[1] - frame.origin[1]) / frame.scale),
        ];
        let region = frame
            .hits
            .iter()
            .rev()
            .find(|region| region.enabled && region.contains(point))?;
        match region.pressed.as_deref()? {
            "experience.once" if self.experience_approval_ready() => Some(Choice::Once),
            "experience.always" if self.experience_approval_ready() => Some(Choice::Always),
            "experience.never" => Some(Choice::Never),
            "experience.cancel" => Some(Choice::Cancel),
            _ => None,
        }
    }

    /// Draws trusted chrome last, including when server UI or the HUD is hidden; a missing
    /// engine fails the chrome.
    pub fn paint_experience_chrome(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
    ) {
        let Some(chrome) = self.experience.as_mut() else {
            return;
        };
        let Some(renderer) = engine else {
            chrome.failed = true;
            return;
        };
        let mut data = DataSource::new();
        data.set_collection("disclosures", disclosure_rows(&chrome.text));
        data.set_global("#experience_reviewed", Scalar::Bool(chrome.reviewed));
        data.set_global(
            "#experience_hint",
            Scalar::Text(
                if chrome.reviewed {
                    "F9 stops this server's code at any time."
                } else {
                    "Scroll to the end to enable Allow."
                }
                .to_owned(),
            ),
        );
        data.set_global("#experience_text", Scalar::Text(chrome.text.clone()));
        data.set_global("#experience_widgets", Scalar::Text(chrome.labels.clone()));
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let reference = if chrome.prompt {
            "cinnabar_experience.consent"
        } else {
            "cinnabar_experience.indicator"
        };
        match renderer.draw(ScreenArt::default(), paint_frame, |env, root| {
            chrome.screen.render_with(
                reference,
                &chrome.catalog,
                &Context::default(),
                data,
                (root, px, language),
                env,
                &chrome.view,
            )
        }) {
            Ok(frame) => {
                chrome.failed = frame.is_none()
                    || (chrome.prompt
                        && frame.as_ref().is_none_or(|frame| {
                            frame.report.scrolls.is_empty()
                                || frame
                                    .report
                                    .scrolls
                                    .values()
                                    .any(|metrics| metrics.viewport < 20.0)
                        }));
                if !chrome.failed
                    && chrome.prompt
                    && let Some(frame) = &frame
                {
                    chrome.reviewed |= frame
                        .report
                        .scrolls
                        .values()
                        .all(|metrics| metrics.offset >= metrics.max_offset());
                }
                chrome.frame = frame;
            }
            Err(error) => {
                paint_frame.nodes.truncate(rollback.0);
                *paint_frame.next = rollback.1;
                chrome.frame = None;
                chrome.failed = true;
                tracing::warn!(%error, "server experience chrome could not render");
            }
        }
    }
}

/// Splits disclosure text into small measured rows so no valid offer exceeds a label's text budget.
fn disclosure_rows(text: &str) -> Vec<CollectionItem> {
    text.lines()
        .flat_map(|line| {
            line.chars()
                .collect::<Vec<_>>()
                .chunks(256)
                .map(|chunk| {
                    CollectionItem::new("line")
                        .with("#disclosure_text", Scalar::Text(chunk.iter().collect()))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    /// Whether trusted chrome is installed, drawn or not.
    pub fn experience_chrome_installed(&self) -> bool {
        self.experience.is_some()
    }

    /// The consent or status frame the chrome last drew.
    pub fn experience_frame(&self) -> Option<&EngineFrame> {
        self.experience.as_ref()?.frame.as_ref()
    }
}
