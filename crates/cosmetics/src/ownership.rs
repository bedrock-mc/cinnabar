//! Session ownership supplied by the authenticated account API, never inferred from display names.
use std::{collections::BTreeSet, sync::RwLock};

#[derive(Default, PartialEq)]
struct Ownership {
    xuid: String,
    items: BTreeSet<String>,
}

static OWNERSHIP: RwLock<Ownership> = RwLock::new(Ownership {
    xuid: String::new(),
    items: BTreeSet::new(),
});

/// Replaces the authenticated session snapshot; an absent identity withdraws every paid grant.
pub fn set_ownership(xuid: &str, ids: &[String]) {
    {
        let current = OWNERSHIP.read().unwrap_or_else(|error| error.into_inner());
        if current.xuid == xuid
            && current.items.len() == ids.len()
            && ids.iter().all(|id| current.items.contains(id))
        {
            return;
        }
    }
    let mut ownership = OWNERSHIP.write().unwrap_or_else(|error| error.into_inner());
    *ownership = Ownership::from_session(xuid, ids);
}

/// Returns paid ownership from the current authenticated session; free ownership stays in the catalog.
pub fn owns(id: &str) -> bool {
    OWNERSHIP
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .items
        .contains(id)
}

impl Ownership {
    /// Fails closed for unbound or unbounded bridge data, including replies from older cores.
    fn from_session(xuid: &str, ids: &[String]) -> Self {
        if xuid.is_empty()
            || xuid.len() > 20
            || !xuid.bytes().all(|byte| byte.is_ascii_digit())
            || ids.len() > crate::MAX_ITEMS
            || ids.iter().any(|id| {
                id.is_empty()
                    || id.len() > 128
                    || !id.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || b"_.:-".contains(&byte)
                    })
            })
        {
            return Self::default();
        }
        Self {
            xuid: xuid.to_owned(),
            items: ids.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_replacement_and_disconnect_remove_previous_grants() {
        let granted = Ownership::from_session("1", &["fixture:paid".into()]);
        assert!(granted.items.contains("fixture:paid"));
        assert!(Ownership::from_session("2", &[]).items.is_empty());
        assert!(
            Ownership::from_session("", &["fixture:paid".into()])
                .items
                .is_empty()
        );
        assert!(
            Ownership::from_session("name", &["fixture:paid".into()])
                .items
                .is_empty()
        );
        assert!(
            Ownership::from_session("1", &["invalid/item".into()])
                .items
                .is_empty()
        );
    }
}
