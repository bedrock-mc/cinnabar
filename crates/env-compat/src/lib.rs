//! Environment reads for variables renamed from `RUST_MCBE_*` to `CINNABAR_*`; the old
//! spelling is still accepted for one release.

use std::{
    collections::BTreeSet,
    env::VarError,
    ffi::OsString,
    sync::{Mutex, PoisonError},
};

/// Environment keys shared by client layout and tooling.
pub mod markers;

/// Prefix every renamed variable now carries.
pub const PREFIX: &str = "CINNABAR_";
const LEGACY_PREFIX: &str = "RUST_MCBE_";

static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Returns `name`'s value, else its `RUST_MCBE_` spelling's value, warning once per old name.
#[expect(
    clippy::print_stderr,
    reason = "dependency-free crate read before any subscriber exists; stderr is its only warning channel"
)]
pub fn var_os(name: &str) -> Option<OsString> {
    resolve(
        name,
        |key| std::env::var_os(key),
        |legacy| {
            if first_use(&WARNED, legacy) {
                eprintln!("warning: {legacy} is deprecated; set {name} instead");
            }
        },
    )
}

/// [`var_os`] with `std::env::var`'s missing and non-Unicode errors.
pub fn var(name: &str) -> Result<String, VarError> {
    var_os(name)
        .ok_or(VarError::NotPresent)?
        .into_string()
        .map_err(VarError::NotUnicode)
}

/// The pre-rename spelling of `name`, or `None` when it lacks [`PREFIX`].
fn legacy_name(name: &str) -> Option<String> {
    name.strip_prefix(PREFIX)
        .map(|rest| format!("{LEGACY_PREFIX}{rest}"))
}

/// Looks `name` up, then its legacy spelling, calling `on_legacy` only when the fallback answers.
fn resolve(
    name: &str,
    lookup: impl Fn(&str) -> Option<OsString>,
    on_legacy: impl FnOnce(&str),
) -> Option<OsString> {
    if let Some(value) = lookup(name) {
        return Some(value);
    }
    let legacy = legacy_name(name)?;
    let value = lookup(&legacy)?;
    on_legacy(&legacy);
    Some(value)
}

/// Whether `legacy` is recorded in `warned` for the first time.
fn first_use(warned: &Mutex<BTreeSet<String>>, legacy: &str) -> bool {
    warned
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(legacy.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::BTreeMap};

    fn lookup(entries: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let entries = entries
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect::<BTreeMap<_, _>>();
        move |key| entries.get(key).cloned()
    }

    #[test]
    fn new_name_wins_over_the_legacy_name_without_a_warning() {
        let warned = Cell::new(false);
        let value = resolve(
            "CINNABAR_GPU_NODES",
            lookup(&[("CINNABAR_GPU_NODES", "1"), ("RUST_MCBE_GPU_NODES", "0")]),
            |_| warned.set(true),
        );
        assert_eq!(value.as_deref(), Some("1".as_ref()));
        assert!(!warned.get());
    }

    #[test]
    fn legacy_name_answers_when_the_new_name_is_unset_and_reports_it() {
        let reported = Cell::new(None);
        let value = resolve(
            "CINNABAR_CPU_CULLING",
            lookup(&[("RUST_MCBE_CPU_CULLING", "1")]),
            |legacy| reported.set(Some(legacy.to_owned())),
        );
        assert_eq!(value.as_deref(), Some("1".as_ref()));
        assert_eq!(reported.take().as_deref(), Some("RUST_MCBE_CPU_CULLING"));
    }

    #[test]
    fn unprefixed_names_have_no_legacy_fallback() {
        let value = resolve(
            "WGPU_BACKEND",
            lookup(&[("RUST_MCBE_WGPU_BACKEND", "gl")]),
            |_| panic!("no legacy spelling"),
        );
        assert_eq!(value, None);
    }

    #[test]
    fn each_legacy_name_warns_only_once() {
        let warned = Mutex::new(BTreeSet::new());
        assert!(first_use(&warned, "RUST_MCBE_GPU_UI"));
        assert!(!first_use(&warned, "RUST_MCBE_GPU_UI"));
        assert!(first_use(&warned, "RUST_MCBE_GPU_NODES"));
    }
}
