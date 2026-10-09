//! The session's read-only items and recipes as the `cinnabar:session` WIT package gives them:
//! plain data the client builds from what it already decoded, and the host pages to a guest.

use std::{collections::BTreeMap, sync::Arc};

/// Items or recipes one `page` call returns at most.
pub const MAX_PAGE: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemKey {
    pub identifier: String,
    pub aux: u16,
}

/// A stack with vanilla's `#item_id_aux` icon key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub key: ItemKey,
    pub icon: i64,
    pub count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Construction,
    Nature,
    Equipment,
    Items,
    CommandOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub key: ItemKey,
    pub icon: i64,
    pub name: String,
    pub group: Option<String>,
    pub category: Option<Category>,
    pub max_stack: u8,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngredientKind {
    Item(ItemKey),
    AnyAux(String),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ingredient {
    pub kind: IngredientKind,
    pub count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeCategory {
    Crafting,
    Stonecutter,
    Cartography,
    SmithingTransform,
    SmithingTrim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub network_id: u32,
    pub category: RecipeCategory,
    pub shapeless: bool,
    pub width: u8,
    pub height: u8,
    pub ingredients: Vec<Option<Ingredient>>,
    pub outputs: Vec<Stack>,
}

/// Everything the `cinnabar:session` package reads, with a revision for each half. Revisions
/// are the client's own counters of the sources each half was built from, so a guest sees a
/// change exactly when the server resent them.
#[derive(Clone, Debug, Default)]
pub struct SessionData {
    pub item_revision: u64,
    pub items: Arc<[Item]>,
    /// Tag members by tag, for tags the session's items carry.
    pub tags: Arc<BTreeMap<String, Vec<String>>>,
    pub recipe_revision: u64,
    pub recipes: Arc<[Recipe]>,
}

impl SessionData {
    /// The vanilla `#item_id_aux` key of `network_id` with `aux`.
    pub fn icon_key(network_id: i32, aux: u16) -> i64 {
        (i64::from(network_id) << 16) | i64::from(aux)
    }

    /// Up to `count` items from `start`, at most [`MAX_PAGE`].
    pub fn item_page(&self, start: u32, count: u32) -> &[Item] {
        page(&self.items, start, count)
    }

    /// Up to `count` recipes from `start`, at most [`MAX_PAGE`].
    pub fn recipe_page(&self, start: u32, count: u32) -> &[Recipe] {
        page(&self.recipes, start, count)
    }

    pub fn lookup(&self, key: &ItemKey) -> Option<&Item> {
        self.items.iter().find(|item| item.key == *key)
    }

    /// The identifiers in `tag`, at most [`MAX_PAGE`] times eight.
    pub fn tag_members(&self, tag: &str) -> &[String] {
        self.tags
            .get(tag)
            .map_or(&[], |members| &members[..members.len().min(MAX_PAGE * 8)])
    }
}

fn page<T>(all: &[T], start: u32, count: u32) -> &[T] {
    let start = (start as usize).min(all.len());
    let end = start + (count as usize).min(MAX_PAGE).min(all.len() - start);
    &all[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(identifier: &str) -> Item {
        Item {
            key: ItemKey {
                identifier: identifier.into(),
                aux: 0,
            },
            icon: 0,
            name: identifier.into(),
            group: None,
            category: None,
            max_stack: 64,
            tags: Vec::new(),
        }
    }

    #[test]
    fn pages_are_capped_and_clamped() {
        let data = SessionData {
            items: (0..600).map(|i| item(&format!("m:{i}"))).collect(),
            ..SessionData::default()
        };
        assert_eq!(data.item_page(0, 1000).len(), MAX_PAGE);
        assert_eq!(data.item_page(590, 20).len(), 10);
        assert!(data.item_page(600, 20).is_empty());
        assert!(data.item_page(u32::MAX, u32::MAX).is_empty());
        assert_eq!(data.item_page(10, 2)[0].key.identifier, "m:10");
    }

    #[test]
    fn icon_keys_are_vanillas_id_aux() {
        assert_eq!(SessionData::icon_key(388, 0), 388 << 16);
        assert_eq!(SessionData::icon_key(5, 3), (5 << 16) | 3);
        assert!(SessionData::icon_key(-7, 0) < 0);
    }
}
