//! `player-mod`'s own imports: the mod's screens beside the container screens and the
//! session's read-only items and recipes. Every write stages into a copy of the committed
//! screens, which the host publishes only when the callback returns.

pub(super) use super::Stack as GuestStack;
use super::{
    State,
    cinnabar::{
        extension::screen,
        server_experience::ui,
        session::{items, recipes},
    },
};
use crate::{ModEvent, ModScreens};
use anyhow::{Result, ensure};
use server_experience::{
    policy::MAX_HOST_OUTPUT,
    screen::{self as host_screen, Rect, ScreenLayout as HostLayout},
    session_data as data,
};

pub(super) use super::{DataSource, Modifiers, ScreenLayout as Layout};

/// Host calls one callback may make, reads included.
const MAX_HOST_CALLS: usize = 1024;

impl State {
    /// Stops host-call floods even when the guest ignores denied results.
    fn charge(&mut self) -> Result<()> {
        self.calls += 1;
        ensure!(self.calls <= MAX_HOST_CALLS, "host-call limit exceeded");
        Ok(())
    }

    /// Applies one screen write of `bytes` to this callback's copy of the screens, within the
    /// output budget; a denied write changes nothing.
    fn stage(
        &mut self,
        bytes: usize,
        write: impl FnOnce(&mut ModScreens, &Self) -> Result<(), String>,
    ) -> Result<Result<(), String>> {
        self.charge()?;
        if !self.grants.screen {
            return Ok(Err("screen permission denied".into()));
        }
        if bytes > MAX_HOST_OUTPUT - self.output {
            return Ok(Err("host output budget exceeded".into()));
        }
        let mut screens = self
            .pending_screens
            .clone()
            .unwrap_or_else(|| self.screens.clone());
        let result = write(&mut screens, self)
            .and_then(|()| screens.data.check().map_err(|error| error.to_string()));
        if result.is_ok() {
            self.output += bytes;
            self.pending_screens = Some(screens);
        }
        Ok(result)
    }

    fn template(&self, template: Option<&String>) -> Result<(), String> {
        match template {
            Some(template) if !self.declared.templates.contains(template) => {
                Err(format!("{template} is not a template mod.toml declares"))
            }
            _ => Ok(()),
        }
    }

    /// Whether the host may deliver `event`: actions, edit boxes and keys must be declared.
    pub(super) fn check_event(&self, event: &ModEvent) -> Result<()> {
        match event {
            ModEvent::Action { id, .. }
            | ModEvent::SecondaryAction { id, .. }
            | ModEvent::TextChanged { control: id, .. } => {
                ensure!(
                    self.declared.actions.contains(id),
                    "{id} is not a declared action"
                );
            }
            ModEvent::Key { id, .. } => {
                ensure!(self.grants.keys, "keys permission denied");
                ensure!(
                    self.declared.keys.contains(id),
                    "{id} is not a declared key"
                );
            }
            ModEvent::ScreenChanged(_)
            | ModEvent::Scrolled { .. }
            | ModEvent::DataChanged(_)
            | ModEvent::ViewClosed => {}
        }
        Ok(())
    }

    fn denied(&mut self) -> Result<Result<(), String>> {
        self.charge()?;
        Ok(Err("a client part's ui is not a player mod's".into()))
    }

    /// Charges a session read and refuses it without `granted`.
    fn read(&mut self, granted: bool, permission: &str) -> Result<Result<(), String>> {
        self.charge()?;
        Ok(if granted {
            Ok(())
        } else {
            Err(format!("{permission} permission denied"))
        })
    }
}

impl screen::Host for State {
    fn layout(&mut self) -> Result<Option<Layout>> {
        self.charge()?;
        Ok(self.layout.as_ref().map(layout))
    }

    fn set_overlay(&mut self, template: Option<String>) -> Result<Result<(), String>> {
        self.stage(0, |screens, state| {
            state.template(template.as_ref())?;
            screens.overlay = template;
            screens.data.revision += 1;
            Ok(())
        })
    }

    fn open_view(&mut self, template: Option<String>) -> Result<Result<(), String>> {
        self.stage(0, |screens, state| {
            state.template(template.as_ref())?;
            if template.is_some() && state.layout.is_none() {
                return Err("no container screen is open".into());
            }
            screens.view = template;
            screens.data.revision += 1;
            Ok(())
        })
    }

    fn set_collection(&mut self, name: String, rows_json: Vec<u8>) -> Result<Result<(), String>> {
        let bytes = rows_json.len();
        self.stage(bytes, |screens, _| {
            if !host_screen::collection_name(&name) {
                return Err("invalid collection name".into());
            }
            let rows: Vec<host_screen::Row> = serde_json::from_slice(&rows_json)
                .map_err(|error| format!("invalid rows: {error}"))?;
            host_screen::validate_rows(&rows).map_err(|error| error.to_string())?;
            screens.data.set_collection(name, rows);
            Ok(())
        })
    }

    fn set_value(&mut self, name: String, value: ui::Value) -> Result<Result<(), String>> {
        let value = match value {
            ui::Value::Boolean(value) => host_screen::Value::Bool(value),
            ui::Value::Integer(value) => host_screen::Value::Integer(value),
            ui::Value::Number(value) => host_screen::Value::Number(value),
            ui::Value::Text(value) => host_screen::Value::Text(value),
            ui::Value::Numbers(values) => host_screen::Value::Numbers(values),
        };
        let bytes = name.len() + serde_json::to_vec(&value).map_or(0, |json| json.len());
        self.stage(bytes, |screens, _| {
            if !host_screen::binding_name(&name) {
                return Err("invalid binding name".into());
            }
            value.validate().map_err(|error| error.to_string())?;
            screens.data.set_value(name, value);
            Ok(())
        })
    }

    fn set_text(&mut self, control: String, text: String) -> Result<Result<(), String>> {
        let bytes = control.len() + text.len();
        self.stage(bytes, |screens, state| {
            if !state.declared.actions.contains(&control) {
                return Err(format!("{control} is not a declared action"));
            }
            if !host_screen::edit_text(&text) {
                return Err("invalid edit box text".into());
            }
            screens.data.set_text(control, text);
            Ok(())
        })
    }

    fn focus_text(&mut self, control: String) -> Result<Result<(), String>> {
        let bytes = control.len();
        self.stage(bytes, |screens, state| {
            if !state.declared.actions.contains(&control) {
                return Err(format!("{control} is not a declared action"));
            }
            screens.data.revision += 1;
            screens.focus = Some((screens.data.revision, control));
            Ok(())
        })
    }
}

/// Player mods use only `ui`'s types; its functions are a client part's and always refuse.
impl ui::Host for State {
    fn set_widget(&mut self, _: String, _: String) -> Result<Result<(), String>> {
        self.denied()
    }
    fn open_screen(&mut self, _: Option<String>) -> Result<Result<(), String>> {
        self.denied()
    }
    fn close_screen(&mut self) -> Result<Result<(), String>> {
        self.denied()
    }
    fn set_collection(&mut self, _: String, _: Vec<u8>) -> Result<Result<(), String>> {
        self.denied()
    }
    fn set_value(&mut self, _: String, _: ui::Value) -> Result<Result<(), String>> {
        self.denied()
    }
    fn modal_size(&mut self) -> Result<Option<ui::GuiSize>> {
        self.charge()?;
        Ok(None)
    }
    fn set_text(&mut self, _: String, _: String) -> Result<Result<(), String>> {
        self.denied()
    }
}

impl items::Host for State {
    fn revision(&mut self) -> Result<Result<u64, String>> {
        let revision = self.session.item_revision;
        Ok(self.read(self.grants.items, "items")?.map(|()| revision))
    }

    fn count(&mut self) -> Result<Result<u32, String>> {
        let count = u32::try_from(self.session.items.len()).unwrap_or(u32::MAX);
        Ok(self.read(self.grants.items, "items")?.map(|()| count))
    }

    fn page(&mut self, start: u32, count: u32) -> Result<Result<Vec<items::Item>, String>> {
        let granted = self.read(self.grants.items, "items")?;
        Ok(granted.map(|()| {
            self.session
                .item_page(start, count)
                .iter()
                .map(item)
                .collect()
        }))
    }

    fn lookup(&mut self, key: items::ItemKey) -> Result<Result<Option<items::Item>, String>> {
        let granted = self.read(self.grants.items, "items")?;
        let key = data::ItemKey {
            identifier: key.identifier,
            aux: key.aux,
        };
        Ok(granted.map(|()| self.session.lookup(&key).map(item)))
    }

    fn tag_members(&mut self, tag: String) -> Result<Result<Vec<String>, String>> {
        let granted = self.read(self.grants.items, "items")?;
        Ok(granted.map(|()| self.session.tag_members(&tag).to_vec()))
    }
}

impl recipes::Host for State {
    fn revision(&mut self) -> Result<Result<u64, String>> {
        let revision = self.session.recipe_revision;
        Ok(self
            .read(self.grants.recipes, "recipes")?
            .map(|()| revision))
    }

    fn count(&mut self) -> Result<Result<u32, String>> {
        let count = u32::try_from(self.session.recipes.len()).unwrap_or(u32::MAX);
        Ok(self.read(self.grants.recipes, "recipes")?.map(|()| count))
    }

    fn page(&mut self, start: u32, count: u32) -> Result<Result<Vec<recipes::Recipe>, String>> {
        let granted = self.read(self.grants.recipes, "recipes")?;
        Ok(granted.map(|()| {
            self.session
                .recipe_page(start, count)
                .iter()
                .map(recipe)
                .collect()
        }))
    }
}

pub(super) fn modifiers(held: crate::KeyModifiers) -> Modifiers {
    [
        (held.ctrl, Modifiers::CTRL),
        (held.shift, Modifiers::SHIFT),
        (held.alt, Modifiers::ALT),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(Modifiers::empty(), |all, (_, modifier)| all | modifier)
}

pub(super) fn layout(layout: &HostLayout) -> Layout {
    let rect = |rect: &Rect| screen::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    Layout {
        screen: layout.screen.clone(),
        size: ui::GuiSize {
            width: layout.size.width,
            height: layout.size.height,
            scale: layout.size.scale,
        },
        gui: rect(&layout.gui),
        exclusions: layout.exclusions.iter().map(rect).collect(),
        view: layout.view.as_ref().map(rect),
    }
}

fn key(key: &data::ItemKey) -> items::ItemKey {
    items::ItemKey {
        identifier: key.identifier.clone(),
        aux: key.aux,
    }
}

pub(super) fn stack(stack: &data::Stack) -> GuestStack {
    GuestStack {
        key: key(&stack.key),
        icon: stack.icon,
        count: stack.count,
    }
}

fn item(item: &data::Item) -> items::Item {
    items::Item {
        key: key(&item.key),
        icon: item.icon,
        name: item.name.clone(),
        group: item.group.clone(),
        category: item.category.map(|category| match category {
            data::Category::Construction => items::Category::Construction,
            data::Category::Nature => items::Category::Nature,
            data::Category::Equipment => items::Category::Equipment,
            data::Category::Items => items::Category::Items,
            data::Category::CommandOnly => items::Category::CommandOnly,
        }),
        max_stack: item.max_stack,
        tags: item.tags.clone(),
    }
}

fn recipe(recipe: &data::Recipe) -> recipes::Recipe {
    let ingredient = |ingredient: &data::Ingredient| recipes::Ingredient {
        kind: match &ingredient.kind {
            data::IngredientKind::Item(item) => recipes::IngredientKind::Item(key(item)),
            data::IngredientKind::AnyAux(id) => recipes::IngredientKind::AnyAux(id.clone()),
            data::IngredientKind::Tag(tag) => recipes::IngredientKind::Tag(tag.clone()),
        },
        count: ingredient.count,
    };
    recipes::Recipe {
        network_id: recipe.network_id,
        category: match recipe.category {
            data::RecipeCategory::Crafting => recipes::Category::Crafting,
            data::RecipeCategory::Stonecutter => recipes::Category::Stonecutter,
            data::RecipeCategory::Cartography => recipes::Category::Cartography,
            data::RecipeCategory::SmithingTransform => recipes::Category::SmithingTransform,
            data::RecipeCategory::SmithingTrim => recipes::Category::SmithingTrim,
        },
        shapeless: recipe.shapeless,
        width: recipe.width,
        height: recipe.height,
        ingredients: recipe
            .ingredients
            .iter()
            .map(|cell| cell.as_ref().map(ingredient))
            .collect(),
        outputs: recipe.outputs.iter().map(stack).collect(),
    }
}
