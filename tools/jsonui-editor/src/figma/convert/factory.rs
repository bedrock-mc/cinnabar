//! `@factory`: a stack repeating its first child per collection item, with
//! every child supplying one preview item's bound values.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{Body, Ctx, Place, Scope, names, visible_fills};
use crate::figma::{Node, Paint};

impl Ctx<'_> {
    /// Makes `stack` a collection factory of its first child; every child
    /// supplies one preview item's bound values.
    pub(super) fn factory(
        &mut self,
        node: &Node,
        flow: &[&Node],
        collection: &str,
        place: Place,
        stack: &mut Body,
        path: &str,
    ) {
        let Some((template, samples)) = flow.split_first() else {
            self.warn(node, path, "`@factory` needs a child to repeat");
            return;
        };
        let scope = Scope {
            path: path.to_owned(),
            item: Some((collection.to_owned(), 0)),
        };
        let Some((name, body)) = self.control(template, place, &scope) else {
            return;
        };
        let name = self.top.unique(&name);
        self.definitions.insert(name.clone(), Value::Object(body));
        for (index, sample) in samples.iter().enumerate() {
            self.samples(sample, &scope.path_to(sample), collection, index + 1);
        }
        // One preview item per child, bound values or not.
        for index in 0..flow.len() {
            self.item(collection, index);
        }
        // A template ignores item roles, so views of one collection coexist.
        let reference = format!("{}.{name}", self.namespace);
        stack.insert("collection_name".into(), collection.into());
        stack.insert(
            "factory".into(),
            json!({ "name": format!("{collection}_factory"), "control_name": reference }),
        );
    }

    /// Records a factory sample's bound values without converting it.
    fn samples(&mut self, node: &Node, path: &str, collection: &str, index: usize) {
        let directives = names::parse(&node.name);
        if let Some(binding) = &directives.binding {
            let image = node.raster.as_deref().or_else(|| {
                let fills: Vec<&Paint> = visible_fills(node).collect();
                match fills.as_slice() {
                    [paint] if paint.kind == "IMAGE" => paint.image.as_deref(),
                    _ => None,
                }
            });
            let value = match (&node.text, image) {
                (Some(text), _) => Some(text.characters.clone().into()),
                (None, Some(key)) => self.texture(node, key, path).map(Value::from),
                (None, None) => None,
            };
            if let Some(value) = value {
                self.item(collection, index).insert(binding.clone(), value);
            }
        }
        if let Some(binding) = &directives.visible {
            self.item(collection, index)
                .insert(binding.clone(), node.visible.into());
        }
        if node.raster.is_none() {
            for child in &node.children {
                self.samples(
                    child,
                    &format!("{path} / {}", child.name),
                    collection,
                    index,
                );
            }
        }
    }

    pub(super) fn item(&mut self, collection: &str, index: usize) -> &mut BTreeMap<String, Value> {
        let items = self
            .mock
            .collections
            .entry(collection.to_owned())
            .or_default();
        if items.len() <= index {
            items.resize_with(index + 1, BTreeMap::new);
        }
        &mut items[index]
    }
}
