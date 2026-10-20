//! Anonymous dressing-room states built from the original cosmetic fixtures.

#[cfg(feature = "test-support")]
use std::sync::Arc;

#[cfg(feature = "test-support")]
use super::{Fixture, MenuScreen, MenuView};

/// Shows an anonymous wardrobe with the first original cosmetic equipped.
#[cfg(feature = "test-support")]
pub(super) fn gallery() -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::DressingRoom;
    let item = render_model::hats::fixture::bunny();
    render_model::hats::fixture::crown();
    render_model::hats::fixture::devil_horns();
    render_model::hats::fixture::shoulder_parrot();
    render_model::hats::fixture::dragon_wings();
    Arc::make_mut(&mut view.dressing_room).section =
        launcher::dressing_room::DressingRoomSection::Cosmetics;
    Arc::make_mut(&mut view.dressing_room)
        .selected_cosmetics
        .insert(item.descriptor().slot.clone(), Some(item.id().into()));
    Fixture::Menu(Box::new(view))
}

/// Shows a filtered cosmetic gallery without changing the equipped character.
#[cfg(feature = "test-support")]
pub(super) fn cosmetic_category(category: launcher::dressing_room::CosmeticCategory) -> Fixture {
    let mut fixture = gallery();
    if let Fixture::Menu(view) = &mut fixture {
        Arc::make_mut(&mut view.dressing_room).cosmetic_category = category;
    }
    fixture
}

/// Shows the anchored category choices with a selected filter.
#[cfg(feature = "test-support")]
pub(super) fn cosmetic_filter() -> Fixture {
    let mut fixture = cosmetic_category(launcher::dressing_room::CosmeticCategory::Head);
    if let Fixture::Menu(view) = &mut fixture {
        Arc::make_mut(&mut view.dressing_room).cosmetic_filter_open = true;
    }
    fixture
}

/// Shows the catalog gallery with the original devil horns selected.
#[cfg(feature = "test-support")]
pub(super) fn devil_horns() -> Fixture {
    let mut fixture = gallery();
    if let Fixture::Menu(view) = &mut fixture {
        let item = render_model::hats::fixture::devil_horns();
        Arc::make_mut(&mut view.dressing_room)
            .selected_cosmetics
            .insert(item.descriptor().slot.clone(), Some(item.id().into()));
    }
    fixture
}

/// Shows independent head and back selections together in the compact catalog gallery.
#[cfg(feature = "test-support")]
pub(super) fn dragon_wings() -> Fixture {
    let mut fixture = devil_horns();
    if let Fixture::Menu(view) = &mut fixture {
        let item = render_model::hats::fixture::dragon_wings();
        Arc::make_mut(&mut view.dressing_room)
            .selected_cosmetics
            .insert(item.descriptor().slot.clone(), Some(item.id().into()));
    }
    fixture
}

/// Shows the parrot alongside head and back items in the cosmetics gallery.
#[cfg(feature = "test-support")]
pub(super) fn parrot() -> Fixture {
    let mut fixture = dragon_wings();
    if let Fixture::Menu(view) = &mut fixture {
        let item = render_model::hats::fixture::shoulder_parrot();
        let wardrobe = Arc::make_mut(&mut view.dressing_room);
        wardrobe.section = launcher::dressing_room::DressingRoomSection::Cosmetics;
        wardrobe.cosmetic_category = launcher::dressing_room::CosmeticCategory::Pet;
        wardrobe
            .selected_cosmetics
            .insert(item.descriptor().slot.clone(), Some(item.id().into()));
    }
    fixture
}
