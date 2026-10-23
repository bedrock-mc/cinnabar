//! Offline marketplace preview. Catalog selections never install or grant mod permissions.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use serde::Deserialize;

/// Shared demonstration data and the permission rules consumed by both storefronts.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub demo: bool,
    pub accounts: BTreeMap<String, Account>,
    pub roles: BTreeMap<String, Vec<String>>,
    pub review_transitions: Vec<ReviewTransition>,
    pub listings: Vec<Listing>,
}

/// A fictional presentation profile; this record is not authenticated account proof.
#[derive(Debug, Deserialize)]
pub struct Account {
    pub name: String,
    pub gamertag: String,
    pub role: String,
}

/// One permitted publication transition, evaluated against the acting account.
#[derive(Debug, Deserialize)]
pub struct ReviewTransition {
    pub from: String,
    pub action: String,
    pub to: String,
    pub permission: String,
    pub owner: String,
    pub requires_reason: bool,
}

/// A synthetic mod listing, without an executable download or account credentials.
#[derive(Debug, Deserialize)]
pub struct Listing {
    pub id: u64,
    pub slug: String,
    pub title: String,
    pub creator: String,
    pub owner: String,
    pub category: String,
    pub summary: String,
    pub description: String,
    pub version: String,
    pub status: String,
    pub permissions: Vec<String>,
    pub license: String,
    pub tone: String,
    pub icon: String,
}

/// Loads the single shared fixture once; malformed shipped data is a build defect.
pub fn catalog() -> &'static Catalog {
    static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../data/marketplace-demo.json"))
            .expect("shipped marketplace demonstration catalog")
    });
    &CATALOG
}

impl Catalog {
    /// Public consumers see only the exact revisions that have been published.
    pub fn published(&self, id: u64) -> Option<&Listing> {
        self.listings
            .iter()
            .find(|item| item.id == id && item.status == "published")
    }

    /// Derives filter labels from public listings, preserving their catalog order.
    pub fn categories(&self) -> Vec<&str> {
        let mut result = Vec::new();
        for item in self
            .listings
            .iter()
            .filter(|item| item.status == "published")
        {
            if !result.contains(&item.category.as_str()) {
                result.push(item.category.as_str());
            }
        }
        result
    }

    /// Unknown roles grant nothing, including roles supplied by an untrusted caller.
    pub fn permits(&self, role: &str, permission: &str) -> bool {
        self.roles
            .get(role)
            .is_some_and(|permissions| permissions.iter().any(|value| value == permission))
    }

    /// Evaluates the demo review policy; production must resolve actors on the server.
    pub fn next_status<'a>(
        &'a self,
        role: &str,
        actor: &str,
        item: &Listing,
        action: &str,
        reason: &str,
    ) -> Option<&'a str> {
        if actor.trim().is_empty() {
            return None;
        }
        self.review_transitions
            .iter()
            .find(|rule| {
                rule.from == item.status
                    && rule.action == action
                    && self.permits(role, &rule.permission)
                    && (!rule.requires_reason || !reason.trim().is_empty())
                    && match rule.owner.as_str() {
                        "self" => actor == item.owner,
                        "other" => actor != item.owner,
                        _ => false,
                    }
            })
            .map(|rule| rule.to.as_str())
    }
}

/// Local navigation commands contain stable listing identities rather than row positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Browse,
    Library,
    Category(Option<usize>),
    Details(u64),
    Save(u64),
    Back,
    Close,
}

/// Session-only POC state; a saved preview is neither an entitlement nor an installed mod.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub open: bool,
    pub library_only: bool,
    pub category: Option<usize>,
    pub details: Option<u64>,
    pub saved: BTreeSet<u64>,
}

impl State {
    /// Reduces local navigation and rejects unpublished or unknown listing identities.
    pub fn apply(&mut self, action: Action) {
        let catalog = catalog();
        match action {
            Action::Browse | Action::Library => {
                self.open = true;
                self.library_only = action == Action::Library;
                self.category = None;
                self.details = None;
            }
            Action::Category(category) if self.open => {
                self.category = category.filter(|index| *index < catalog.categories().len());
                self.details = None;
            }
            Action::Details(id) if self.open && catalog.published(id).is_some() => {
                self.details = Some(id)
            }
            Action::Save(id) if self.open && catalog.published(id).is_some() => {
                if !self.saved.remove(&id) {
                    self.saved.insert(id);
                }
            }
            Action::Back => self.details = None,
            Action::Close => {
                self.open = false;
                self.details = None;
            }
            _ => {}
        }
    }

    /// Applies public visibility, the selected category and the local preview library.
    pub fn listings(&self) -> Vec<&'static Listing> {
        let catalog = catalog();
        let categories = catalog.categories();
        let category = self
            .category
            .and_then(|index| categories.get(index))
            .copied();
        catalog
            .listings
            .iter()
            .filter(|item| {
                item.status == "published"
                    && category.is_none_or(|value| item.category == value)
                    && (!self.library_only || self.saved.contains(&item.id))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
