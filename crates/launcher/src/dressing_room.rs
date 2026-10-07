//! Classic skin choices shared by the launcher and presentation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const STARTER_SKIN_NAMES: [&str; 2] = ["Steve", "Alex"];
pub const MAX_SKIN_NAME_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkinModel {
    #[default]
    Classic,
    Slim,
}

impl SkinModel {
    pub const fn arm_size(self) -> &'static str {
        match self {
            Self::Classic => "wide",
            Self::Slim => "slim",
        }
    }

    pub const fn geometry(self) -> &'static str {
        match self {
            Self::Classic => "geometry.humanoid.custom",
            Self::Slim => "geometry.humanoid.customSlim",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SetSection(DressingRoomSection),
    Select(usize),
    Import,
    ImportCape,
    SelectCape(Option<usize>),
    SetModel(SkinModel),
    BeginRename(usize),
    BeginDelete(usize),
    BeginRenameCape(usize),
    BeginDeleteCape(usize),
    SaveRename,
    ConfirmDelete,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DressingRoomSection {
    #[default]
    Skins,
    Capes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinEditorMode {
    Rename,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinEditorTarget {
    Skin,
    Cape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinEditor {
    pub index: usize,
    pub mode: SkinEditorMode,
    pub draft: String,
    pub target: SkinEditorTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DressingRoomSkin {
    pub id: String,
    pub name: String,
    /// The original runtime asset or the private immutable imported PNG.
    pub path: String,
    pub imported: bool,
    pub model: SkinModel,
    pub skin: protocol::StandardSkin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DressingRoomCape {
    pub id: String,
    pub name: String,
    pub path: String,
    pub cape: protocol::CapeImage,
    pub imported: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DressingRoomView {
    pub section: DressingRoomSection,
    pub skins: Arc<[DressingRoomSkin]>,
    pub selected: Option<usize>,
    pub busy: bool,
    pub message: Option<String>,
    pub editor: Option<SkinEditor>,
    pub capes: Arc<[DressingRoomCape]>,
    pub selected_cape: Option<usize>,
}

impl DressingRoomView {
    pub fn selected_skin(&self) -> Option<&DressingRoomSkin> {
        self.selected.and_then(|index| self.skins.get(index))
    }

    pub fn selected_cape(&self) -> Option<&DressingRoomCape> {
        self.selected_cape.and_then(|index| self.capes.get(index))
    }

    pub fn active_skin(&self) -> Option<protocol::StandardSkin> {
        let mut skin = self.selected_skin()?.skin.clone();
        skin.cape = self.selected_cape().map(|entry| entry.cape.clone());
        Some(skin)
    }
}
