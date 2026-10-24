//! Data-driven UI (DDUI): the server's synced data stores and the screens that read them.
//!
//! Server scripts mirror their UI state into named data stores (`ClientboundDataStore`) and
//! then ask the client to show a screen bound to one store property. The client writes user
//! input back as `ServerboundDataStore` path updates and reports every screen it closes with
//! `ServerboundDataDrivenScreenClosed`.

mod scripted;
mod store;
mod value;

use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{
    ClientboundDataDrivenUICloseScreen, ClientboundDataDrivenUIShowScreen, ClientboundDataStore,
    ServerboundDataDrivenScreenClosed, ServerboundDataStore,
};
use bedrock_protocol_1_26_51::types::{
    BedrockDDUI, BedrockDDUIDataStoreUpdate, BedrockDDUIDataStoreUpdateData,
};
use bedrock_protocol_1_26_51::wire;

pub use scripted::{
    CLOSE_BUTTON_CLICKS, ControlKind, CustomFormView, DropdownItem, FormControl, MessageBoxView,
    SCRIPT_STORE, ScreenButton, ScreenText, ScriptedScreen,
};
pub use store::{DataStores, StoreApply};
pub use value::{DataPrimitive, DataValue, PathError};
use value::{from_wire, text};

use super::UiEvent;

/// One normalized DDUI packet.
#[derive(Debug, Clone, PartialEq)]
pub enum DataDrivenUiEvent {
    /// Store changes, applied in packet order.
    Store(Vec<DataStoreChange>),
    Show(ShowDataDrivenScreen),
    /// Closes the screen showing `form_id`, or whichever screen is showing when `None`.
    Close {
        form_id: Option<u32>,
    },
    /// Asks the client to reload its DDUI definitions.
    Reload,
}

/// A request to show the screen `screen_id` as form instance `form_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowDataDrivenScreen {
    pub screen_id: Arc<str>,
    pub form_id: u32,
    /// Selects the store property the screen reads; scripted forms name their property with it.
    pub data_instance_id: Option<u32>,
}

/// One server data store change.
#[derive(Debug, Clone, PartialEq)]
pub enum DataStoreChange {
    Update(DataStoreUpdate),
    /// Replaces a whole property and its replacement counter.
    Replace {
        store: Arc<str>,
        property: Arc<str>,
        update_count: u32,
        value: DataValue,
    },
    Remove {
        store: Arc<str>,
    },
}

/// One primitive written at a path inside a property, in either direction.
#[derive(Debug, Clone, PartialEq)]
pub struct DataStoreUpdate {
    pub store: Arc<str>,
    pub property: Arc<str>,
    pub path: Arc<str>,
    pub value: DataPrimitive,
    /// The property replacement this write was made against.
    pub property_update_count: u32,
    /// This path's write counter after the write.
    pub path_update_count: u32,
}

/// Why the client closed a data-driven screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDrivenScreenCloseReason {
    /// The server closed this form by its id.
    ProgrammaticClose,
    /// The server closed every form.
    ProgrammaticCloseAll,
    /// The player dismissed the form.
    ClientCanceled,
    /// The client could not show the form.
    UserBusy,
    InvalidForm,
}

impl DataDrivenScreenCloseReason {
    /// The reason's wire spelling, as gophertunnel encodes it.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::ProgrammaticClose => "programmaticclose",
            Self::ProgrammaticCloseAll => "programmaticcloseall",
            Self::ClientCanceled => "clientcanceled",
            Self::UserBusy => "userbusy",
            Self::InvalidForm => "invalidform",
        }
    }
}

/// The packet reporting that the client closed form `form_id`.
#[must_use]
pub fn data_driven_screen_closed_packet(
    form_id: u32,
    reason: DataDrivenScreenCloseReason,
) -> crate::Packet {
    crate::Packet::new(ServerboundDataDrivenScreenClosed {
        form_id: wire::U32LE(form_id),
        close_reason: wire::Str::from_static(reason.wire_name()),
    })
}

/// The packet carrying one client write to the server.
#[must_use]
pub fn data_store_update_packet(update: &DataStoreUpdate) -> crate::Packet {
    let data = match &update.value {
        DataPrimitive::Number(value) => BedrockDDUIDataStoreUpdateData::Double(wire::F64LE(*value)),
        DataPrimitive::Bool(value) => BedrockDDUIDataStoreUpdateData::Bool(*value),
        DataPrimitive::String(value) => {
            BedrockDDUIDataStoreUpdateData::String(value.as_ref().into())
        }
    };
    crate::Packet::new(ServerboundDataStore {
        update: BedrockDDUIDataStoreUpdate {
            data_store_name: update.store.as_ref().into(),
            property: update.property.as_ref().into(),
            path: update.path.as_ref().into(),
            data,
            property_update_count: wire::U32LE(update.property_update_count),
            path_update_count: wire::U32LE(update.path_update_count),
        },
    })
}

pub(crate) fn normalize_data_store(packet: ClientboundDataStore) -> UiEvent {
    let changes = packet
        .updates
        .into_iter()
        .map(|change| match change {
            BedrockDDUI::DataStoreUpdate(update) => DataStoreChange::Update(DataStoreUpdate {
                store: text(update.data_store_name),
                property: text(update.property),
                path: text(update.path),
                value: match update.data {
                    BedrockDDUIDataStoreUpdateData::Double(value) => DataPrimitive::Number(value.0),
                    BedrockDDUIDataStoreUpdateData::Bool(value) => DataPrimitive::Bool(value),
                    BedrockDDUIDataStoreUpdateData::String(value) => {
                        DataPrimitive::String(text(value))
                    }
                },
                property_update_count: update.property_update_count.0,
                path_update_count: update.path_update_count.0,
            }),
            BedrockDDUI::DataStoreChange {
                data_store_name,
                property,
                update_count,
                the_new_property_value,
            } => DataStoreChange::Replace {
                store: text(data_store_name),
                property: text(property),
                update_count: update_count.0,
                value: from_wire(the_new_property_value),
            },
            BedrockDDUI::DataStoreRemoval { data_store_name } => DataStoreChange::Remove {
                store: text(data_store_name),
            },
        })
        .collect();
    UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(changes))
}

pub(crate) fn normalize_show_screen(packet: ClientboundDataDrivenUIShowScreen) -> UiEvent {
    UiEvent::DataDrivenUi(DataDrivenUiEvent::Show(ShowDataDrivenScreen {
        screen_id: text(packet.screen_id),
        form_id: packet.form_id.0,
        data_instance_id: packet.data_instance_id.map(|id| id.0),
    }))
}

pub(crate) fn normalize_close_screen(packet: ClientboundDataDrivenUICloseScreen) -> UiEvent {
    UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
        form_id: packet.form_id.map(|id| id.0),
    })
}
