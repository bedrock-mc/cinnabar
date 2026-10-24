//! `profile.<NAME>` presets: option values a selector applies together, as Iris reads them.

use std::collections::{BTreeMap, BTreeSet};

use crate::options::{OptionValues, PackOption, find_option};

/// One profile with its includes resolved.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// Values the profile sets, by option name. They are not checked against the options'
    /// allowed values, as in Iris.
    pub values: BTreeMap<String, String>,
    /// Programs (`[<folder>/]<name>`) the pack skips while this profile is current.
    pub disabled_programs: Vec<String>,
}

/// Where the current values stand among the profiles (see [`super::OptionMenu::profiles`]).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileScan {
    /// The first profile whose values all hold; `None` shows "Custom".
    pub current: Option<usize>,
    /// The profile the selector steps to forwards: the one after `current`, wrapping, or
    /// the first when none is current.
    pub next: usize,
    /// The profile it steps to backwards: the one before `current`, wrapping, or the last.
    pub previous: usize,
}

impl Profile {
    /// Whether every value the profile sets for a known option matches `values` (normalized
    /// overrides). Boolean options compare as `true`/`false`; unknown names always match.
    pub fn matches(&self, options: &[PackOption], values: &OptionValues) -> bool {
        self.values.iter().all(|(name, value)| {
            find_option(options, name).is_none_or(|option| option.value(values) == value)
        })
    }
}

/// Finds the current profile and its neighbours; `None` without profiles.
pub(crate) fn scan(
    profiles: &[Profile],
    options: &[PackOption],
    values: &OptionValues,
) -> Option<ProfileScan> {
    let last = profiles.len().checked_sub(1)?;
    Some(
        match profiles
            .iter()
            .position(|profile| profile.matches(options, values))
        {
            Some(index) => ProfileScan {
                current: Some(index),
                next: if index == last { 0 } else { index + 1 },
                previous: index.checked_sub(1).unwrap_or(last),
            },
            None => ProfileScan {
                current: None,
                next: 0,
                previous: last,
            },
        },
    )
}

/// Resolves every profile of `tree` (name → tokens, in declaration order) and orders them
/// for stepping: more values first, declaration order among equals (Iris's stable sort by
/// precedence). Tokens: `!program.<name>` disables a program, `profile.<NAME>` includes
/// another profile, `!OPTION` sets `false`, `OPTION=value` or `OPTION:value` sets a value and
/// a bare boolean option sets `true`; anything else is ignored. Iris refuses to load a pack
/// whose profiles include a missing profile or include one twice along the way; such a pack
/// keeps its options here but gets no profiles.
pub(super) fn parse(tree: &[(String, Vec<String>)], options: &[PackOption]) -> Vec<Profile> {
    let tokens = tree
        .iter()
        .map(|(name, tokens)| (name.as_str(), tokens.as_slice()))
        .collect();
    let mut profiles = Vec::with_capacity(tree.len());
    for (name, _) in tree {
        match resolve(name, &tokens, options) {
            Some(profile) => profiles.push(profile),
            None => return Vec::new(),
        }
    }
    profiles.sort_by_key(|profile| std::cmp::Reverse(profile.values.len()));
    profiles
}

/// Merges includes in token order using a heap stack. Every included name is tracked for
/// the whole traversal, so repeated includes are rejected even across separate branches.
fn resolve(
    name: &str,
    tree: &BTreeMap<&str, &[String]>,
    options: &[PackOption],
) -> Option<Profile> {
    let tokens = tree.get(name)?;
    let mut pending = vec![tokens.iter()];
    let mut parents = BTreeSet::new();
    let mut profile = Profile {
        name: name.to_owned(),
        values: BTreeMap::new(),
        disabled_programs: Vec::new(),
    };
    while let Some(tokens) = pending.last_mut() {
        let Some(token) = tokens.next() else {
            pending.pop();
            continue;
        };
        if let Some(program) = token.strip_prefix("!program.") {
            profile.disabled_programs.push(program.to_owned());
        } else if let Some(included) = token.strip_prefix("profile.") {
            if !parents.insert(included) {
                return None;
            }
            pending.push(tree.get(included)?.iter());
        } else if let Some(option) = token.strip_prefix('!') {
            profile.values.insert(option.to_owned(), "false".to_owned());
        } else if let Some((option, value)) =
            token.split_once('=').or_else(|| token.split_once(':'))
        {
            profile.values.insert(option.to_owned(), value.to_owned());
        } else if find_option(options, token).is_some_and(PackOption::is_boolean) {
            profile.values.insert(token.clone(), "true".to_owned());
        }
    }
    Some(profile)
}
