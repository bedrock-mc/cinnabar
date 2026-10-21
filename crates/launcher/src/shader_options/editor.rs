//! Runs the screen's commands against a pack's option session and builds its view.

use shaderpack::{MenuElement, OptionSession, OptionValues};

use super::{Action, Control, Element, Input, Status, View};

/// The profile selector's label and its value when no profile matches (Iris's English text).
const PROFILE_LABEL: &str = "Profile";
const CUSTOM_PROFILE: &str = "Custom";

/// What the host does after a command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Values to save as the pack's option file before reloading the pack with them.
    pub save: Option<OptionValues>,
    /// The screen closes.
    pub close: bool,
}

/// One open options screen.
#[derive(Clone, Debug)]
pub struct Editor {
    session: OptionSession,
    pack: String,
    /// Changes whenever another screen is shown.
    screen: u32,
}

impl Editor {
    /// Edits `session` for the pack shown as `pack`.
    pub fn new(session: OptionSession, pack: &str) -> Editor {
        Editor {
            session,
            pack: pack.to_owned(),
            screen: 0,
        }
    }

    /// The session being edited.
    pub fn session(&self) -> &OptionSession {
        &self.session
    }

    /// Runs one command. [`Action::Open`] belongs to the host and does nothing here.
    pub fn handle(&mut self, action: Action) -> Outcome {
        match action {
            Action::Open => Outcome::default(),
            Action::Element {
                screen,
                index,
                input,
            } => {
                if screen == self.screen {
                    self.press(usize::from(index), input);
                }
                Outcome::default()
            }
            Action::Back => {
                if self.session.back() {
                    self.screen = self.screen.wrapping_add(1);
                    Outcome::default()
                } else {
                    self.handle(Action::Done)
                }
            }
            Action::Apply => Outcome {
                save: self.session.apply(),
                close: false,
            },
            Action::Done => Outcome {
                save: self.session.apply(),
                close: true,
            },
            Action::Cancel => {
                self.session.discard();
                Outcome {
                    save: None,
                    close: true,
                }
            }
            Action::Reset => {
                self.session.reset();
                Outcome {
                    save: Some(OptionValues::new()),
                    close: false,
                }
            }
        }
    }

    /// Applies `input` to element `index` of the shown screen.
    fn press(&mut self, index: usize, input: Input) {
        let Some(element) = self.session.screen().elements.get(index).cloned() else {
            return;
        };
        match element {
            MenuElement::Empty => {}
            MenuElement::Link(id) => {
                if input == Input::Next {
                    self.session.open(&id);
                    self.screen = self.screen.wrapping_add(1);
                }
            }
            MenuElement::Profile => self.session.step_profile(input != Input::Previous),
            MenuElement::Option { name, .. } => {
                if self.session.option(&name).is_some_and(|o| o.is_boolean()) {
                    self.session.toggle(&name);
                    return;
                }
                match input {
                    Input::Next => self.session.step(&name, true),
                    Input::Previous => self.session.step(&name, false),
                    Input::Select(choice) => self.session.select(&name, usize::from(choice)),
                }
            }
        }
    }

    /// The shown screen.
    pub fn view(&self) -> View {
        let session = &self.session;
        let lang = session.lang();
        let screen = session.screen();
        let values = session.values();
        let elements = screen
            .elements
            .iter()
            .map(|element| match element {
                MenuElement::Empty => Element {
                    label: String::new(),
                    comment: None,
                    control: Control::Empty,
                    modified: false,
                },
                MenuElement::Link(id) => Element {
                    label: lang.screen_label(id),
                    comment: lang.screen_comment(id),
                    control: Control::Link,
                    modified: false,
                },
                MenuElement::Profile => Element {
                    label: PROFILE_LABEL.to_owned(),
                    comment: lang.profile_comment(),
                    control: Control::Profile {
                        value: session.current_profile().map_or_else(
                            || CUSTOM_PROFILE.to_owned(),
                            |profile| lang.profile_label(&profile.name),
                        ),
                    },
                    modified: false,
                },
                MenuElement::Option { name, slider } => {
                    let control = match session.option(name) {
                        Some(option) if option.is_boolean() => Control::Switch {
                            on: option.value(&values) == "true",
                        },
                        Some(option) => {
                            let current = option.value(&values);
                            let value = lang.value_label(name, current);
                            let choices = option.choices();
                            if *slider && choices.len() > 1 {
                                let index = choices
                                    .iter()
                                    .position(|choice| *choice == current)
                                    .unwrap_or(0);
                                Control::Slider {
                                    value,
                                    index: u16::try_from(index).unwrap_or(u16::MAX),
                                    count: u16::try_from(choices.len()).unwrap_or(u16::MAX),
                                }
                            } else if choices.len() > 1 {
                                Control::Choice { value }
                            } else {
                                Control::Fixed { value }
                            }
                        }
                        None => Control::Empty,
                    };
                    Element {
                        label: lang.option_label(name),
                        comment: lang.option_comment(name),
                        control,
                        modified: session.is_modified(name),
                    }
                }
            })
            .collect();
        View {
            pack: self.pack.clone(),
            title: session
                .screen_id()
                .filter(|id| session.menu().screen(id).is_some())
                .map_or_else(|| self.pack.clone(), |id| lang.screen_label(id)),
            screen: self.screen,
            nested: session.screen_id().is_some(),
            columns: u16::try_from(screen.columns).unwrap_or(u16::MAX),
            elements,
            changed: session.has_changes(),
            status: Status::Ready,
        }
    }
}
