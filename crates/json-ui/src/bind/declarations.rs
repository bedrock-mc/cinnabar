//! Binding declarations belong to immutable templates, not data refreshes.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use super::{Binding, Src, bag, spec};
use crate::lru::Lru;
use crate::tree::ResolvedControl;

/// Parsed bindings and creation diagnostics shared by template instances.
pub(super) struct Declaration {
    pub(super) bags: bag::Bags,
    pub(super) bindings: Arc<Vec<Binding>>,
    pub(super) diagnostics: Vec<String>,
    pub(super) observes_scroll: bool,
    pub(super) uses_visibility: bool,
    pub(super) radio_group: bool,
}

struct Entry {
    owner: Weak<ResolvedControl>,
    declaration: Arc<Declaration>,
}

/// Bound the cache across screen and pack replacements.
const MAX_DECLARATIONS: usize = 4096;

thread_local! {
    static CACHE: RefCell<Lru<usize, Entry>> = RefCell::new(Lru::new(MAX_DECLARATIONS));
}

/// Parse a template once while its owning immutable tree stays alive.
pub(super) fn get(src: &Src) -> Arc<Declaration> {
    let control = src.get();
    let key = control as *const ResolvedControl as usize;
    CACHE.with(|cache| {
        if let Some(entry) = cache.borrow().get(&key)
            && entry.owner.as_ptr() == Arc::as_ptr(src.owner())
        {
            return Arc::clone(&entry.declaration);
        }
        let mut diagnostics = Vec::new();
        let bindings = Arc::new(spec::parse(control, &mut diagnostics));
        let declaration = Arc::new(Declaration {
            bags: bag::Bags::new(control),
            observes_scroll: spec::observes_scroll(control, &bindings),
            uses_visibility: bindings.iter().any(|binding| {
                !matches!(binding.kind, spec::Kind::View { .. })
                    && matches!(
                        binding.condition,
                        spec::Condition::Visible
                            | spec::Condition::AlwaysWhenVisible
                            | spec::Condition::VisibilityChanged
                    )
            }),
            radio_group: control.properties.get("radio_toggle_group")
                == Some(&serde_json::Value::Bool(true)),
            bindings,
            diagnostics,
        });
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_DECLARATIONS {
            cache.retain(|entry| entry.owner.strong_count() != 0);
        }
        cache.insert(
            key,
            Entry {
                owner: Arc::downgrade(src.owner()),
                declaration: Arc::clone(&declaration),
            },
        );
        declaration
    })
}
