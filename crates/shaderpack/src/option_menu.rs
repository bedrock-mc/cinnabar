//! The option screens a pack declares in `shaders.properties`, built as Iris builds its
//! Shader Pack Settings menu.
//!
//! Iris reads the layout from the file as plain java.util.Properties, before any
//! preprocessing: `screen` lists the main screen, `screen.<ID>` each sub-screen,
//! `screen.columns` and `screen.<ID>.columns` their column counts, `sliders` the value
//! options drawn as sliders and `profile.<NAME>` the profiles. A screen element is an option
//! name, `[ID]` (a link to sub-screen `ID`), `<empty>` (a blank cell), `<profile>` (the
//! profile selector, dropped when the pack has no profiles) or `*` (every option no screen
//! lists). Unknown names become blank cells. Without `screen`, the main screen is `*`.

mod profiles;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::options::{OptionValues, PackOption, find_option};
use crate::properties::reader::read_plain_entries;

pub use profiles::{Profile, ProfileScan};

/// Element strings with a fixed meaning.
const EMPTY: &str = "<empty>";
const PROFILE: &str = "<profile>";
const REST: &str = "*";
/// Iris lays out a screen without a column count in two columns, or three past this many
/// elements.
const TWO_COLUMN_LIMIT: usize = 18;

/// One cell of an option screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuElement {
    /// A blank cell.
    Empty,
    /// The profile selector.
    Profile,
    /// A button opening the sub-screen with this id. The id may name no screen; Iris then
    /// shows the main screen.
    Link(String),
    /// A pack option; `slider` when `sliders` lists it (only value options draw one).
    Option { name: String, slider: bool },
}

/// One screen's elements in reading order, laid out row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuScreen {
    pub elements: Vec<MenuElement>,
    /// Elements per row: the pack's count (a count below one puts one element on each row,
    /// as in Iris), else two, or three when the screen has more than 18 elements.
    pub columns: usize,
}

/// A pack's option menu: the main screen, its sub-screens and profiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionMenu {
    main: MenuScreen,
    screens: BTreeMap<String, MenuScreen>,
    profiles: Vec<Profile>,
}

/// A screen's element strings and declared column count, read from the file.
struct Layout {
    elements: Vec<String>,
    columns: Option<i32>,
}

/// A screen's resolved elements, before unlisted options fill its `*`.
struct Resolved<'a> {
    id: &'a str,
    columns: Option<i32>,
    elements: Vec<MenuElement>,
    /// Where the screen's first `*` stands.
    rest: Option<usize>,
}

impl OptionMenu {
    /// Builds the menu from the raw `shaders.properties` text and the pack's options. The
    /// last definition of a key wins.
    pub(crate) fn new(properties: &str, options: &[PackOption]) -> OptionMenu {
        let mut main = Layout {
            elements: vec![REST.to_owned()],
            columns: None,
        };
        let mut sub: Vec<(String, Layout)> = Vec::new();
        let mut sub_columns: BTreeMap<String, i32> = BTreeMap::new();
        let mut sliders: Vec<String> = Vec::new();
        let mut profile_tree: Vec<(String, Vec<String>)> = Vec::new();
        for entry in read_plain_entries(properties) {
            let (key, value) = (entry.key.as_str(), entry.value.as_str());
            if key == "sliders" {
                sliders = elements(value);
            }
            if let Some(name) = key.strip_prefix("profile.") {
                upsert(&mut profile_tree, name, elements(value));
            }
            // Iris consumes column keys here, even when the count is not a number.
            if key == "screen.columns" {
                main.columns = value.parse().ok();
                continue;
            }
            if let Some(id) = key
                .strip_prefix("screen.")
                .and_then(|rest| rest.strip_suffix(".columns"))
            {
                match value.parse() {
                    Ok(columns) => {
                        sub_columns.insert(id.to_owned(), columns);
                    }
                    Err(_) => {
                        sub_columns.remove(id);
                    }
                }
                continue;
            }
            if key == "screen" {
                main.elements = elements(value);
            }
            if let Some(id) = key.strip_prefix("screen.") {
                let layout = Layout {
                    elements: elements(value),
                    columns: None,
                };
                upsert(&mut sub, id, layout);
            }
        }
        for (id, layout) in &mut sub {
            layout.columns = sub_columns.get(id).copied();
        }
        let profiles = profiles::parse(&profile_tree, options);
        let sliders: BTreeSet<&str> = sliders.iter().map(String::as_str).collect();
        let mut builder = Builder {
            options,
            sliders: &sliders,
            has_profiles: !profiles.is_empty(),
            used: BTreeSet::new(),
        };
        let mut screens: Vec<Resolved<'_>> = std::iter::once(("", &main))
            .chain(sub.iter().map(|(id, layout)| (id.as_str(), layout)))
            .map(|(id, layout)| {
                let (elements, rest) = builder.elements(&layout.elements);
                Resolved {
                    id,
                    columns: layout.columns,
                    elements,
                    rest,
                }
            })
            .collect();
        // Iris fills only one `*` with the unlisted options (which one depends on hashing);
        // this fills the first in file order, the main screen first.
        let unused = builder.unused();
        if let Some(screen) = screens.iter_mut().find(|screen| screen.rest.is_some()) {
            let index = screen.rest.unwrap_or(0);
            screen.elements.splice(index..index, unused);
        }
        let mut screens = screens.into_iter().map(|screen| {
            let columns = match screen.columns {
                Some(columns) => usize::try_from(columns).unwrap_or(0).max(1),
                None if screen.elements.len() > TWO_COLUMN_LIMIT => 3,
                None => 2,
            };
            let elements = screen.elements;
            (screen.id.to_owned(), MenuScreen { elements, columns })
        });
        let main = screens.next().map(|(_, screen)| screen);
        OptionMenu {
            main: main.unwrap_or(MenuScreen {
                elements: Vec::new(),
                columns: 2,
            }),
            screens: screens.collect(),
            profiles,
        }
    }

    /// The screen shown first.
    pub fn main(&self) -> &MenuScreen {
        &self.main
    }

    /// The sub-screen `id` declares, if any.
    pub fn screen(&self, id: &str) -> Option<&MenuScreen> {
        self.screens.get(id)
    }

    /// Profiles in the order the selector steps through them: most option values first,
    /// declaration order among equals.
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    /// Finds the current profile for `values` (normalized overrides; see
    /// [`crate::normalize_values`]) and the profiles either side of it; `None` without
    /// profiles.
    pub fn scan_profiles(
        &self,
        options: &[PackOption],
        values: &OptionValues,
    ) -> Option<ProfileScan> {
        profiles::scan(&self.profiles, options, values)
    }
}

/// Resolves element strings while tracking which options some screen lists.
struct Builder<'a> {
    options: &'a [PackOption],
    sliders: &'a BTreeSet<&'a str>,
    has_profiles: bool,
    used: BTreeSet<String>,
}

impl Builder<'_> {
    /// One screen's elements, and where its first `*` stands. Later `*`s add nothing.
    fn elements(&mut self, strings: &[String]) -> (Vec<MenuElement>, Option<usize>) {
        let mut elements = Vec::with_capacity(strings.len());
        let mut rest = None;
        for string in strings {
            if string == REST {
                rest.get_or_insert(elements.len());
                continue;
            }
            if let Some(element) = self.element(string) {
                elements.push(element);
            }
        }
        (elements, rest)
    }

    /// The element `string` names; `None` for `<profile>` in a pack without profiles.
    fn element(&mut self, string: &str) -> Option<MenuElement> {
        if string == EMPTY {
            return Some(MenuElement::Empty);
        }
        if string == PROFILE {
            return self.has_profiles.then_some(MenuElement::Profile);
        }
        if let Some(id) = string
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            return Some(MenuElement::Link(id.to_owned()));
        }
        if find_option(self.options, string).is_none() {
            return Some(MenuElement::Empty);
        }
        self.used.insert(string.to_owned());
        Some(MenuElement::Option {
            name: string.to_owned(),
            slider: self.sliders.contains(string),
        })
    }

    /// Option elements for every option no screen lists, sorted by name.
    fn unused(&self) -> Vec<MenuElement> {
        self.options
            .iter()
            .filter(|option| !self.used.contains(&option.name))
            .map(|option| MenuElement::Option {
                name: option.name.clone(),
                slider: self.sliders.contains(option.name.as_str()),
            })
            .collect()
    }
}

/// A list value's elements: Iris splits on runs of spaces, so an empty value is one empty
/// element.
fn elements(value: &str) -> Vec<String> {
    let mut parts: Vec<String> = value
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if parts.is_empty() {
        parts.push(String::new());
    }
    parts
}

/// Sets `key` in an insertion-ordered list, keeping the first position as a
/// `LinkedHashMap` does.
fn upsert<T>(list: &mut Vec<(String, T)>, key: &str, value: T) {
    match list.iter_mut().find(|(existing, _)| existing == key) {
        Some((_, slot)) => *slot = value,
        None => list.push((key.to_owned(), value)),
    }
}
