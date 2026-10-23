//! One visit to a pack's option menu: the screen shown, the values in use and the changes not
//! yet applied, with Iris's editing rules.
//!
//! As in Iris, changes queue until applied; a switch flips, a value option steps through its
//! choices (wrapping) or jumps to a slider position, and the profile selector queues every
//! value of the next or previous profile. Applying keeps the queued values that differ from
//! the defaults; resetting drops every saved value.

use crate::lang::{DEFAULT_LANGUAGE, PackLang};
use crate::option_menu::{MenuScreen, OptionMenu, Profile};
use crate::options::{OptionValues, PackOption, find_option, normalize_values};
use crate::pack::ShaderPack;

/// A pack's option menu with the player's values; holds no pack files.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct OptionSession {
    options: Vec<PackOption>,
    menu: OptionMenu,
    lang: PackLang,
    /// Normalized values the pack runs with.
    applied: OptionValues,
    /// Changes not yet applied, as queued (possibly equal to defaults).
    pending: OptionValues,
    /// Opened sub-screen ids, innermost last; empty on the main screen.
    path: Vec<String>,
}

impl OptionSession {
    /// Opens the menu of `pack` with `saved` values (such as an option file's), keeping
    /// those Iris keeps (see [`normalize_values`]). Text comes from the pack's English lang.
    pub fn new(pack: &ShaderPack, saved: &OptionValues) -> OptionSession {
        OptionSession {
            options: pack.options().to_vec(),
            menu: pack.option_menu(),
            lang: pack.lang(DEFAULT_LANGUAGE),
            applied: normalize_values(pack.options(), saved),
            pending: OptionValues::new(),
            path: Vec::new(),
        }
    }

    /// The pack's options, sorted by name.
    pub fn options(&self) -> &[PackOption] {
        &self.options
    }

    /// The option named `name`.
    pub fn option(&self, name: &str) -> Option<&PackOption> {
        find_option(&self.options, name)
    }

    pub fn menu(&self) -> &OptionMenu {
        &self.menu
    }

    pub fn lang(&self) -> &PackLang {
        &self.lang
    }

    /// The open sub-screen's id; `None` on the main screen.
    pub fn screen_id(&self) -> Option<&str> {
        self.path.last().map(String::as_str)
    }

    /// The screen shown: the open sub-screen, or the main screen when the id names none (as
    /// Iris falls back).
    pub fn screen(&self) -> &MenuScreen {
        self.screen_id()
            .and_then(|id| self.menu.screen(id))
            .unwrap_or(self.menu.main())
    }

    /// Opens sub-screen `id` over the current one.
    pub fn open(&mut self, id: &str) {
        self.path.push(id.to_owned());
    }

    /// Returns to the previous screen; `false` on the main screen.
    pub fn back(&mut self) -> bool {
        self.path.pop().is_some()
    }

    /// The values a reload would use: the applied values with the changes on top, normalized.
    pub fn values(&self) -> OptionValues {
        let mut values = self.applied.clone();
        values.extend(
            self.pending
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
        normalize_values(&self.options, &values)
    }

    /// The values the pack runs with.
    pub fn applied(&self) -> &OptionValues {
        &self.applied
    }

    /// An option's shown value, including unapplied changes; `None` for an unknown name.
    pub fn value(&self, name: &str) -> Option<String> {
        let values = self.values();
        self.option(name)
            .map(|option| option.value(&values).to_owned())
    }

    /// Whether an option's shown value differs from the value the pack runs with.
    pub fn is_modified(&self, name: &str) -> bool {
        let Some(option) = self.option(name) else {
            return false;
        };
        option.value(&self.values()) != option.value(&self.applied)
    }

    /// Whether any change waits to be applied (even one back to the running value).
    pub fn has_changes(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Flips a switch.
    pub fn toggle(&mut self, name: &str) {
        let Some(option) = self.option(name).filter(|option| option.is_boolean()) else {
            return;
        };
        let on = option.value(&self.values()) == "true";
        self.pending.insert(name.to_owned(), (!on).to_string());
    }

    /// Steps a value option to its next (or previous) choice, wrapping. A value outside the
    /// choices steps as if it were the first, as in Iris.
    pub fn step(&mut self, name: &str, forward: bool) {
        let values = self.values();
        let Some(option) = self.option(name) else {
            return;
        };
        let choices = option.choices();
        if choices.is_empty() {
            return;
        }
        let current = option.value(&values);
        let index = choices
            .iter()
            .position(|choice| *choice == current)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % choices.len()
        } else {
            index.checked_sub(1).unwrap_or(choices.len() - 1)
        };
        let value = choices[next].to_owned();
        self.pending.insert(name.to_owned(), value);
    }

    /// Sets a value option to choice `index` (a slider position); out-of-range indices are
    /// ignored.
    pub fn select(&mut self, name: &str, index: usize) {
        let Some(value) = self.option(name).and_then(|option| {
            option
                .choices()
                .get(index)
                .map(|choice| (*choice).to_owned())
        }) else {
            return;
        };
        self.pending.insert(name.to_owned(), value);
    }

    /// The profile the shown values match, if any.
    pub fn current_profile(&self) -> Option<&Profile> {
        let scan = self.menu.scan_profiles(&self.options, &self.values())?;
        scan.current.map(|index| &self.menu.profiles()[index])
    }

    /// Queues every value of the next (or previous) profile.
    pub fn step_profile(&mut self, forward: bool) {
        let Some(scan) = self.menu.scan_profiles(&self.options, &self.values()) else {
            return;
        };
        let target = if forward { scan.next } else { scan.previous };
        let values = &self.menu.profiles()[target].values;
        self.pending.extend(
            values
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }

    /// Applies the changes: returns the values to save and reload with, or `None` when
    /// nothing changed (Iris then does not reload).
    pub fn apply(&mut self) -> Option<OptionValues> {
        if self.pending.is_empty() {
            return None;
        }
        self.applied = self.values();
        self.pending.clear();
        Some(self.applied.clone())
    }

    /// Drops every saved value and change; the caller saves and reloads the defaults.
    pub fn reset(&mut self) {
        self.applied.clear();
        self.pending.clear();
    }

    /// Forgets the changes not yet applied.
    pub fn discard(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::TranslateConfig;
    use crate::source::PackSource;

    fn pack() -> ShaderPack {
        ShaderPack::new(PackSource::from_files([
            (
                "settings.glsl",
                "#define GLOW\n#define QUALITY 2 // [1 2 3]\n\
                 const float sunPathRotation = 0.0; // [0.0 30.0]\n",
            ),
            ("final.fsh", "#ifdef GLOW\n#endif\nvoid main() {}\n"),
            (
                "shaders.properties",
                "screen=<profile> [SUB]\nscreen.SUB=GLOW QUALITY\n\
                 profile.LOW=!GLOW QUALITY=1 !program.final\nprofile.HIGH=QUALITY=3\n",
            ),
            ("lang/en_us.lang", "option.GLOW=Glow\n"),
        ]))
    }

    fn values(pairs: &[(&str, &str)]) -> OptionValues {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn saved_values_load_as_iris_keeps_them() {
        let session = OptionSession::new(
            &pack(),
            &values(&[
                ("GLOW", "yes"),
                ("QUALITY", "2"),
                ("sunPathRotation", "45.0"),
                ("X", "1"),
            ]),
        );
        assert_eq!(session.applied(), &values(&[("sunPathRotation", "45.0")]));
        assert_eq!(session.value("GLOW").as_deref(), Some("true"));
        assert_eq!(session.lang().option_label("GLOW"), "Glow");
    }

    #[test]
    fn edits_queue_until_applied() {
        let mut session = OptionSession::new(&pack(), &OptionValues::new());
        session.toggle("GLOW");
        session.step("QUALITY", true);
        assert!(session.has_changes());
        assert!(session.is_modified("GLOW") && session.is_modified("QUALITY"));
        assert_eq!(session.value("QUALITY").as_deref(), Some("3"));
        session.step("QUALITY", true);
        assert_eq!(session.value("QUALITY").as_deref(), Some("1"), "steps wrap");
        session.step("QUALITY", false);
        session.step("QUALITY", false);
        assert_eq!(session.value("QUALITY").as_deref(), Some("2"));
        assert!(!session.is_modified("QUALITY"));
        session.select("sunPathRotation", 1);
        session.select("sunPathRotation", 9);
        assert_eq!(session.applied(), &OptionValues::new());
        assert_eq!(
            session.apply(),
            Some(values(&[("GLOW", "false"), ("sunPathRotation", "30.0")]))
        );
        assert!(!session.has_changes() && !session.is_modified("GLOW"));
        assert_eq!(session.apply(), None, "nothing queued, nothing to reload");
        session.toggle("GLOW");
        session.discard();
        assert_eq!(session.value("GLOW").as_deref(), Some("false"));
        session.reset();
        assert_eq!(session.values(), OptionValues::new());
    }

    #[test]
    fn a_value_outside_the_choices_steps_as_the_first_choice() {
        let mut session = OptionSession::new(&pack(), &values(&[("QUALITY", "7")]));
        session.step("QUALITY", true);
        assert_eq!(session.value("QUALITY").as_deref(), Some("2"));
        let mut session = OptionSession::new(&pack(), &values(&[("QUALITY", "7")]));
        session.step("QUALITY", false);
        assert_eq!(session.value("QUALITY").as_deref(), Some("3"));
    }

    #[test]
    fn the_profile_selector_queues_whole_profiles() {
        let mut session = OptionSession::new(&pack(), &OptionValues::new());
        assert_eq!(session.current_profile(), None, "defaults match no profile");
        session.step_profile(true);
        assert_eq!(
            session.current_profile().map(|p| p.name.as_str()),
            Some("LOW")
        );
        assert_eq!(session.value("GLOW").as_deref(), Some("false"));
        session.step_profile(true);
        assert_eq!(
            session.current_profile().map(|p| p.name.as_str()),
            Some("HIGH")
        );
        assert_eq!(
            session.value("GLOW").as_deref(),
            Some("false"),
            "values the next profile leaves alone stay"
        );
        session.step_profile(false);
        assert_eq!(
            session.current_profile().map(|p| p.name.as_str()),
            Some("LOW")
        );
    }

    #[test]
    fn the_current_profile_disables_its_programs() {
        let pack = pack();
        let enabled = |options: OptionValues| {
            let config = TranslateConfig {
                options,
                ..TranslateConfig::default()
            };
            let translation = pack.translate(&config);
            let program = translation
                .programs
                .iter()
                .find(|p| p.program.name == "final")
                .expect("final program");
            program.enabled
        };
        assert!(enabled(OptionValues::new()));
        assert!(!enabled(values(&[("GLOW", "false"), ("QUALITY", "1")])));
    }

    #[test]
    fn screens_open_and_close_in_order() {
        let mut session = OptionSession::new(&pack(), &OptionValues::new());
        assert_eq!(session.screen_id(), None);
        session.open("SUB");
        assert_eq!(session.screen_id(), Some("SUB"));
        assert_eq!(session.screen().elements.len(), 2);
        session.open("MISSING");
        assert_eq!(
            session.screen(),
            session.menu().main(),
            "unknown ids show the main screen"
        );
        assert!(session.back() && session.back());
        assert!(!session.back());
    }
}
