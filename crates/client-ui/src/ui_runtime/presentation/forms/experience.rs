//! Host-owned consent and status chrome, resolved from a private JSON-UI catalog.

use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
};
use crate::ui_runtime::forms::EngineFrame;
use json_ui::{Catalog, CollectionItem, Context, DataSource, Scalar, ViewState};
use server_experience::trust::Choice;
use std::sync::Arc;
use ui::UiNode;
use {
    super::super::{TextMetrics, UiPresentationRuntime},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

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

impl UiPresentationRuntime {
    /// Remote packs cannot replace this catalog or its button actions.
    pub fn set_experience_chrome(
        &mut self,
        text: Option<&str>,
        prompt: bool,
    ) -> Result<(), String> {
        let Some(text) = text else {
            self.form_presentation.experience = None;
            return Ok(());
        };
        if let Some(chrome) = self.form_presentation.experience.as_mut() {
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
        self.form_presentation.experience = Some(ExperienceChrome {
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
        if let Some(chrome) = self.form_presentation.experience.as_mut() {
            chrome.labels = labels.to_owned();
        }
    }

    /// Keyboard approval is possible only after a consent frame reached presentation.
    pub fn experience_prompt_visible(&self) -> bool {
        self.form_presentation
            .experience
            .as_ref()
            .is_some_and(|chrome| chrome.prompt && !chrome.failed && chrome.frame.is_some())
    }

    /// Approval stays disabled until the full disclosure can be reached and its last page was drawn.
    pub fn experience_approval_ready(&self) -> bool {
        self.experience_prompt_visible()
            && self
                .form_presentation
                .experience
                .as_ref()
                .is_some_and(|chrome| chrome.reviewed)
    }

    /// Scrolls only the trusted disclosure viewport; it never changes the underlying menu.
    pub fn scroll_experience(&mut self, pages: f64) {
        let Some(chrome) = self.form_presentation.experience.as_mut() else {
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
        let Some(chrome) = self.form_presentation.experience.as_mut() else {
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
        self.form_presentation
            .experience
            .as_ref()
            .is_some_and(|chrome| chrome.failed)
    }

    /// Maps only hits from the last host-owned consent screen to local choices.
    pub fn experience_choice(&self, point: [f32; 2]) -> Option<Choice> {
        let chrome = self.form_presentation.experience.as_ref()?;
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

    /// Draws trusted chrome last, including when server UI or the HUD is hidden.
    pub(in super::super) fn append_experience_chrome(
        &mut self,
        runtime: &crate::ui_runtime::UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let Some(chrome) = self.form_presentation.experience.as_mut() else {
            return;
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
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
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &|_| None,
            language: runtime.text_generation(),
        };
        let rollback = (nodes.len(), *next);
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let reference = if chrome.prompt {
            "cinnabar_experience.consent"
        } else {
            "cinnabar_experience.indicator"
        };
        match renderer.draw(ScreenArt::default(), inputs, out, |env, root| {
            chrome.screen.render_with(
                reference,
                &chrome.catalog,
                &Context::default(),
                data,
                (root, px, runtime.text_generation()),
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
                nodes.truncate(rollback.0);
                *next = rollback.1;
                chrome.frame = None;
                chrome.failed = true;
                bevy::log::warn!(%error, "server experience chrome could not render");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::{UiRuntime, presentation::forms::tests::mini_engine_presentation};
    use server_experience::policy::{
        MAX_FALLBACK_BYTES, MAX_IDENTIFIER_BYTES, MAX_ORIGINS, MAX_URL_BYTES,
    };

    #[test]
    fn small_viewports_scroll_maximum_disclosures_before_enabling_approval() {
        let player_runtime = player_state::PlayerState::new(1);

        let packages = (0..server_experience::policy::MAX_BUNDLES)
            .map(|i| {
                format!(
                    "Package: {i}{}\nPublisher: {}",
                    "a".repeat(MAX_IDENTIFIER_BYTES - 1),
                    server_experience::crypto::hex(&[7; 32])
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let hosts = (0..MAX_ORIGINS)
            .map(|i| {
                let prefix = format!("https://{i}");
                format!(
                    "{prefix}{}.com",
                    "a".repeat(MAX_URL_BYTES - prefix.len() - ".com".len())
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let origins = hosts.lines().map(str::to_owned).collect();
        for host in hosts.lines() {
            server_experience::fetch::approved_url(host, &origins).unwrap();
        }
        let text = format!(
            "Cinnabar server experience\n{packages}\nPermissions: Messaging, Media\nMedia/download hosts: {hosts}\nThese hosts see your IP address.\nFallback: {}\nServer code is untrusted.",
            "f".repeat(MAX_FALLBACK_BYTES)
        );
        for size in [[320, 240], [640, 360]] {
            let mut presentation = mini_engine_presentation();
            let runtime = UiRuntime::new(1);
            presentation
                .set_experience_chrome(Some(&text), true)
                .unwrap();
            presentation
                .build(
                    &player_runtime,
                    &runtime,
                    0,
                    size,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            assert!(presentation.experience_prompt_visible());
            assert!(!presentation.experience_approval_ready());
            let chrome = presentation.form_presentation.experience.as_ref().unwrap();
            let frame = chrome.frame.as_ref().unwrap();
            assert!(
                frame
                    .report
                    .scrolls
                    .values()
                    .all(|metrics| metrics.content > metrics.viewport)
            );
            assert!(
                frame
                    .hits
                    .iter()
                    .filter(|hit| matches!(
                        hit.pressed.as_deref(),
                        Some("experience.once" | "experience.always")
                    ))
                    .all(|hit| !hit.enabled)
            );
            for _ in 0..500 {
                presentation.scroll_experience(1.0);
                presentation
                    .build(
                        &player_runtime,
                        &runtime,
                        0,
                        size,
                        ui::DpiScale::new(1.0).unwrap(),
                    )
                    .unwrap();
                if presentation.experience_approval_ready() {
                    break;
                }
            }
            assert!(presentation.experience_approval_ready());
            let mut nodes = Vec::new();
            presentation.append_experience_chrome(
                &runtime,
                &mut nodes,
                &mut 1,
                TextMetrics::for_viewport(size, ui::DpiScale::new(1.0).unwrap(), None),
                size.map(|v| v as f32),
            );
            let texts = super::super::pack_harness::drawn_texts(&nodes);
            assert!(
                texts.iter().any(|text| text == "Server code is untrusted."),
                "last disclosure was not painted: {texts:?}"
            );
            presentation
                .build(
                    &player_runtime,
                    &runtime,
                    0,
                    size,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let frame = presentation
                .form_presentation
                .experience
                .as_ref()
                .unwrap()
                .frame
                .as_ref()
                .unwrap();
            let once = frame
                .hits
                .iter()
                .find(|hit| hit.pressed.as_deref() == Some("experience.once"))
                .unwrap();
            assert!(once.enabled);
            let point = [
                frame.origin[0] + (once.rect.x + once.rect.w / 2.0) as f32 * frame.scale,
                frame.origin[1] + (once.rect.y + once.rect.h / 2.0) as f32 * frame.scale,
            ];
            assert_eq!(presentation.experience_choice(point), Some(Choice::Once));
        }
    }

    #[test]
    fn popup_buttons_answer_with_their_choices() {
        let mut presentation = mini_engine_presentation();
        let runtime = UiRuntime::new(1);
        let player_runtime = player_state::PlayerState::new(1);
        presentation
            .set_experience_chrome(Some("Server: 127.0.0.1:19132"), true)
            .unwrap();
        let size = [1280, 720];
        let build = |presentation: &mut UiPresentationRuntime| {
            presentation
                .build(
                    &player_runtime,
                    &runtime,
                    0,
                    size,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        };
        // A disclosure that fits is reviewed by its first frame; the next one arms approval.
        build(&mut presentation);
        build(&mut presentation);
        assert!(presentation.experience_approval_ready());
        let frame = presentation
            .form_presentation
            .experience
            .as_ref()
            .unwrap()
            .frame
            .clone()
            .unwrap();
        for (action, choice) in [
            ("experience.once", Choice::Once),
            ("experience.always", Choice::Always),
            ("experience.never", Choice::Never),
            ("experience.cancel", Choice::Cancel),
        ] {
            let hit = frame
                .hits
                .iter()
                .find(|hit| hit.pressed.as_deref() == Some(action))
                .unwrap_or_else(|| panic!("{action} has no button"));
            assert!(hit.enabled, "{action}");
            let point = [
                frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
                frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
            ];
            assert_eq!(
                presentation.experience_choice(point),
                Some(choice),
                "{action}"
            );
        }
    }
}
