//! The guest's callbacks, found and type-checked once. Every component exports `init` and
//! `frame`; each `player-mod` event export is optional, and a missing one is never called.

use super::{State, player_mod};
use crate::{DataSource, ModEvent};
use anyhow::{Context, Result, bail};
use wasmtime::{
    Store,
    component::{ComponentNamedList, Instance, Lift, Lower, TypedFunc},
};

type Index = Option<u32>;
/// A collection name and the index of a row in it.
type Row = Option<(String, u32)>;
type KeyFunc = TypedFunc<(String, Option<player_mod::GuestStack>, Row), ()>;

#[derive(Default)]
struct Events {
    screen_changed: Option<TypedFunc<(Option<player_mod::Layout>,), ()>>,
    action: Option<TypedFunc<(String, Index), ()>>,
    secondary: Option<TypedFunc<(String, Index), ()>>,
    scrolled: Option<TypedFunc<(f64, f64, f64, player_mod::Modifiers), ()>>,
    text: Option<TypedFunc<(String, String), ()>>,
    key: Option<KeyFunc>,
    data_changed: Option<TypedFunc<(Vec<player_mod::DataSource>,), ()>>,
    view_closed: Option<TypedFunc<(), ()>>,
}

pub(super) struct Exports {
    frame: TypedFunc<(), ()>,
    events: Events,
}

impl Exports {
    pub(super) fn find(
        store: &mut Store<State>,
        instance: &Instance,
    ) -> Result<(TypedFunc<(), ()>, Self)> {
        let (Some(init), Some(frame)) = (
            instance.get_func(&mut *store, "init"),
            instance.get_func(&mut *store, "frame"),
        ) else {
            bail!("component lacks init or frame");
        };
        let events = Events {
            screen_changed: typed(store, instance, "screen-changed")?,
            action: typed(store, instance, "action")?,
            secondary: typed(store, instance, "secondary-action")?,
            scrolled: typed(store, instance, "scrolled")?,
            text: typed(store, instance, "text-changed")?,
            key: typed(store, instance, "key")?,
            data_changed: typed(store, instance, "data-changed")?,
            view_closed: typed(store, instance, "view-closed")?,
        };
        Ok((
            init.typed(&*store)?,
            Self {
                frame: frame.typed(&*store)?,
                events,
            },
        ))
    }

    /// Whether the component exports any event callback.
    pub(super) fn has_events(&self) -> bool {
        let events = &self.events;
        events.screen_changed.is_some()
            || events.action.is_some()
            || events.secondary.is_some()
            || events.scrolled.is_some()
            || events.text.is_some()
            || events.key.is_some()
            || events.data_changed.is_some()
            || events.view_closed.is_some()
    }

    /// Whether the component reads the session in `data-changed`, which gives `init` the load
    /// budget.
    pub(super) fn loads_session(&self) -> bool {
        self.events.data_changed.is_some()
    }

    pub(super) fn frame(&self, store: &mut Store<State>) -> Result<()> {
        self.frame.call(&mut *store, ())?;
        self.frame.post_return(store)
    }

    pub(super) fn event(&self, store: &mut Store<State>, event: &ModEvent) -> Result<()> {
        fn call<P: ComponentNamedList + Lower>(
            func: Option<&TypedFunc<P, ()>>,
            store: &mut Store<State>,
            params: P,
        ) -> Result<()> {
            let Some(func) = func else {
                return Ok(());
            };
            func.call(&mut *store, params)?;
            func.post_return(store)
        }
        let events = &self.events;
        match event {
            ModEvent::ScreenChanged(layout) => call(
                events.screen_changed.as_ref(),
                store,
                (layout.as_ref().map(player_mod::layout),),
            ),
            ModEvent::Action { id, index } => {
                call(events.action.as_ref(), store, (id.clone(), *index))
            }
            ModEvent::SecondaryAction { id, index } => {
                call(events.secondary.as_ref(), store, (id.clone(), *index))
            }
            ModEvent::Scrolled {
                delta,
                x,
                y,
                modifiers,
            } => call(
                events.scrolled.as_ref(),
                store,
                (*delta, *x, *y, player_mod::modifiers(*modifiers)),
            ),
            ModEvent::TextChanged { control, text } => {
                call(events.text.as_ref(), store, (control.clone(), text.clone()))
            }
            ModEvent::Key { id, hovered, row } => call(
                events.key.as_ref(),
                store,
                (
                    id.clone(),
                    hovered.as_ref().map(player_mod::stack),
                    row.clone(),
                ),
            ),
            ModEvent::DataChanged(sources) => {
                let sources = sources
                    .iter()
                    .map(|source| match source {
                        DataSource::Items => player_mod::DataSource::Items,
                        DataSource::Recipes => player_mod::DataSource::Recipes,
                    })
                    .collect();
                call(events.data_changed.as_ref(), store, (sources,))
            }
            ModEvent::ViewClosed => call(events.view_closed.as_ref(), store, ()),
        }
    }
}

/// The export `name` checked against its `player-mod` signature; none when it is absent.
fn typed<P, R>(
    store: &mut Store<State>,
    instance: &Instance,
    name: &str,
) -> Result<Option<TypedFunc<P, R>>>
where
    P: ComponentNamedList + Lower,
    R: ComponentNamedList + Lift,
{
    instance
        .get_func(&mut *store, name)
        .map(|func| {
            func.typed(&*store)
                .with_context(|| format!("export {name} has another signature"))
        })
        .transpose()
}
