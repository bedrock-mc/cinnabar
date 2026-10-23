//! The guest's callbacks, found and type-checked once. Every component exports `init` and
//! `frame`; each `player-mod` event export is optional, and a missing one is never called.

use super::{State, player_mod};
use crate::{DataSource, ModEvent};
use anyhow::{Context, Result, bail};
use wasmtime::{
    Store,
    component::{
        ComponentNamedList, Instance, InstancePre, Lift, Lower, TypedFunc, types::ComponentItem,
    },
};

type Index = Option<u32>;
/// A collection name and the index of a row in it.
type Row = Option<(String, u32)>;
type KeyParams = (String, Option<player_mod::GuestStack>, Row);
const INIT: &str = "init";
const FRAME: &str = "frame";

/// Defines the optional callback signatures once for admission and live instances.
macro_rules! event_callbacks {
    ($($field:ident: $params:ty = $name:literal => $event:pat),* $(,)?) => {
        #[derive(Default)]
        struct Events {
            $($field: Option<TypedFunc<$params, ()>>),*
        }

        impl Events {
            /// Finds each optional event callback using its declared signature.
            fn find(store: &mut Store<State>, instance: &Instance) -> Result<Self> {
                Ok(Self { $($field: typed(store, instance, $name)?),* })
            }

            /// Checks event exports without instantiating or calling the component.
            fn check(pre: &InstancePre<State>) -> Result<()> {
                $(check::<$params>(pre, $name, false)?;)*
                Ok(())
            }

            /// Whether the matching optional callback is present before reserving its lane.
            fn handles(&self, event: &ModEvent) -> bool {
                match event { $($event => self.$field.is_some()),* }
            }

            /// Whether at least one optional event callback is present.
            fn has_events(&self) -> bool {
                $(self.$field.is_some())||*
            }
        }
    };
}

event_callbacks! {
    screen_changed: (Option<player_mod::Layout>,) = "screen-changed" => ModEvent::ScreenChanged(_),
    action: (String, Index) = "action" => ModEvent::Action { .. },
    secondary: (String, Index) = "secondary-action" => ModEvent::SecondaryAction { .. },
    scrolled: (f64, f64, f64, player_mod::Modifiers) = "scrolled" => ModEvent::Scrolled { .. },
    text: (String, String) = "text-changed" => ModEvent::TextChanged { .. },
    key: KeyParams = "key" => ModEvent::Key { .. },
    data_changed: (Vec<player_mod::DataSource>,) = "data-changed" => ModEvent::DataChanged(_),
    view_closed: () = "view-closed" => ModEvent::ViewClosed,
}

pub(super) struct Exports {
    frame: TypedFunc<(), ()>,
    events: Events,
    filter: Option<TypedFunc<(super::ActionFrame,), (super::ActionVerdict,)>>,
}

impl Exports {
    /// Checks the required and optional callback contracts without executing guest code.
    pub(super) fn check(pre: &InstancePre<State>) -> Result<()> {
        check::<()>(pre, INIT, true)?;
        check::<()>(pre, FRAME, true)?;
        Events::check(pre)?;
        check_result::<(super::ActionFrame,), (super::ActionVerdict,)>(pre, "filter-actions", false)
    }

    /// Finds the callbacks after admission has checked their signatures.
    pub(super) fn find(
        store: &mut Store<State>,
        instance: &Instance,
    ) -> Result<(TypedFunc<(), ()>, Self)> {
        let (Some(init), Some(frame)) = (
            instance.get_func(&mut *store, INIT),
            instance.get_func(&mut *store, FRAME),
        ) else {
            bail!("component lacks init or frame");
        };
        let events = Events::find(store, instance)?;
        Ok((
            init.typed(&*store)?,
            Self {
                frame: frame.typed(&*store)?,
                events,
                filter: typed(store, instance, "filter-actions")?,
            },
        ))
    }

    /// Whether this component explicitly opts into action filtering.
    pub(super) fn has_filter(&self) -> bool {
        self.filter.is_some()
    }

    /// Executes and finishes a synchronous verdict before any input is changed.
    pub(super) fn filter(
        &self,
        store: &mut Store<State>,
        frame: super::ActionFrame,
    ) -> Result<super::ActionVerdict> {
        let filter = self
            .filter
            .as_ref()
            .context("component lacks filter-actions")?;
        let (verdict,) = filter.call(&mut *store, (frame,))?;
        filter.post_return(store)?;
        Ok(verdict)
    }

    /// Whether the component exports any event callback.
    pub(super) fn has_events(&self) -> bool {
        self.events.has_events()
    }

    /// Skips absent optional callbacks before consuming an event or loading allowance.
    pub(super) fn handles_event(&self, event: &ModEvent) -> bool {
        self.events.handles(event)
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

/// Type-checks an exported callback, allowing absence only for optional events.
fn check<P>(pre: &InstancePre<State>, name: &str, required: bool) -> Result<()>
where
    P: ComponentNamedList + Lower,
{
    check_result::<P, ()>(pre, name, required)
}

/// Checks an optional callback including its returned transaction.
fn check_result<P, R>(pre: &InstancePre<State>, name: &str, required: bool) -> Result<()>
where
    P: ComponentNamedList + Lower,
    R: ComponentNamedList + Lift,
{
    let component = pre.component().component_type();
    match component.get_export(pre.engine(), name) {
        Some(ComponentItem::ComponentFunc(func)) => func
            .typecheck::<P, R>(&pre.instance_type())
            .with_context(|| format!("export {name} has another signature")),
        None if !required => Ok(()),
        None => bail!("component lacks {name}"),
        Some(_) => bail!("export {name} is not a callback"),
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
