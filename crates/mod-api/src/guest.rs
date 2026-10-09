//! [`PlayerMod`]: a `player-mod` guest that overrides only the events it uses.

use crate::player_mod::{DataSource, Modifiers, ScreenLayout, Stack};

/// One `player-mod` component, mirroring the world's exports.
///
/// Only [`PlayerMod::init`] and [`PlayerMod::frame`] are required; every event defaults to doing
/// nothing, so a new event export never breaks a mod's source. Export the type with
/// [`export_player_mod!`](crate::export_player_mod).
pub trait PlayerMod {
    /// Runs once, when the component starts.
    fn init();

    /// Runs once per frame.
    fn frame();

    /// Handles `screen-changed`: a container screen opened, closed (`None`) or was laid out anew.
    fn screen_changed(_layout: Option<ScreenLayout>) {}

    /// Handles `action`: a primary press on an overlay or view control mapped to the declared
    /// action `id`; `collection_index` is the control's row in its nearest collection.
    fn action(_id: String, _collection_index: Option<u32>) {}

    /// Handles `secondary-action`: a right click on a control that maps
    /// `button.menu_secondary_select` to the declared action `id`.
    fn secondary_action(_id: String, _collection_index: Option<u32>) {}

    /// Handles `scrolled`: the wheel turned `delta` notches, positive towards the end, with the
    /// pointer at `x`, `y` in GUI units.
    fn scrolled(_delta: f64, _x: f64, _y: f64, _modifiers: Modifiers) {}

    /// Handles `text-changed`: the edit box named `control` now holds `text`.
    fn text_changed(_control: String, _text: String) {}

    /// Handles `key`: the declared key `id` was pressed over a container screen or the view.
    fn key(_id: String, _hovered: Option<Stack>, _row: Option<(String, u32)>) {}

    /// Handles `data-changed`: the listed session sources changed revision.
    fn data_changed(_sources: Vec<DataSource>) {}

    /// Handles `view-closed`: the host, not the mod, closed the view.
    fn view_closed() {}
}

/// Implements the generated [`player_mod::Guest`](crate::player_mod::Guest) for `$ty` by
/// delegating to its [`PlayerMod`] impl, then exports `$ty` as the `player-mod` world.
#[macro_export]
macro_rules! export_player_mod {
    ($ty:ident) => {
        impl $crate::player_mod::Guest for $ty {
            fn init() {
                <$ty as $crate::PlayerMod>::init()
            }

            fn frame() {
                <$ty as $crate::PlayerMod>::frame()
            }

            fn screen_changed(layout: ::core::option::Option<$crate::player_mod::ScreenLayout>) {
                <$ty as $crate::PlayerMod>::screen_changed(layout)
            }

            fn action(id: ::std::string::String, collection_index: ::core::option::Option<u32>) {
                <$ty as $crate::PlayerMod>::action(id, collection_index)
            }

            fn secondary_action(
                id: ::std::string::String,
                collection_index: ::core::option::Option<u32>,
            ) {
                <$ty as $crate::PlayerMod>::secondary_action(id, collection_index)
            }

            fn scrolled(delta: f64, x: f64, y: f64, modifiers: $crate::player_mod::Modifiers) {
                <$ty as $crate::PlayerMod>::scrolled(delta, x, y, modifiers)
            }

            fn text_changed(control: ::std::string::String, text: ::std::string::String) {
                <$ty as $crate::PlayerMod>::text_changed(control, text)
            }

            fn key(
                id: ::std::string::String,
                hovered: ::core::option::Option<$crate::player_mod::Stack>,
                row: ::core::option::Option<(::std::string::String, u32)>,
            ) {
                <$ty as $crate::PlayerMod>::key(id, hovered, row)
            }

            fn data_changed(sources: ::std::vec::Vec<$crate::player_mod::DataSource>) {
                <$ty as $crate::PlayerMod>::data_changed(sources)
            }

            fn view_closed() {
                <$ty as $crate::PlayerMod>::view_closed()
            }
        }

        $crate::player_mod::export_player_mod!($ty with_types_in $crate::player_mod);
    };
}

#[cfg(test)]
mod tests {
    use super::PlayerMod;
    use crate::player_mod::{Guest, Modifiers};
    use std::cell::RefCell;

    thread_local! {
        static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn called(name: &'static str) {
        CALLS.with_borrow_mut(|calls| calls.push(name));
    }

    /// Overrides only the required callbacks and one event.
    struct Minimal;

    impl PlayerMod for Minimal {
        fn init() {
            called("init");
        }

        fn frame() {
            called("frame");
        }

        fn view_closed() {
            called("view-closed");
        }
    }

    crate::export_player_mod!(Minimal);

    #[test]
    fn a_mod_overriding_one_event_runs_it_and_ignores_the_rest() {
        <Minimal as Guest>::init();
        <Minimal as Guest>::frame();
        <Minimal as Guest>::screen_changed(None);
        <Minimal as Guest>::action("minimal.press".into(), Some(0));
        <Minimal as Guest>::secondary_action("minimal.press".into(), None);
        <Minimal as Guest>::scrolled(1.0, 2.0, 3.0, Modifiers::CTRL);
        <Minimal as Guest>::text_changed("minimal.search".into(), "dirt".into());
        <Minimal as Guest>::key("minimal.key".into(), None, None);
        <Minimal as Guest>::data_changed(Vec::new());
        <Minimal as Guest>::view_closed();
        assert_eq!(
            CALLS.with_borrow(Clone::clone),
            ["init", "frame", "view-closed"]
        );
    }
}
