use crate::geometry::{DpiScale, GeometryError, UiPoint};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiAction {
    Navigate([i8; 2]),
    Accept,
    Cancel,
    TabNext,
    TabPrevious,
    PointerMove {
        position: UiPoint,
    },
    PointerPrimary {
        position: UiPoint,
        phase: PointerPhase,
    },
    PointerSecondary {
        position: UiPoint,
        phase: PointerPhase,
    },
    Scroll {
        delta: UiPoint,
    },
}

impl UiAction {
    pub fn pointer_move_from_physical(
        position: [f32; 2],
        dpi: DpiScale,
    ) -> Result<Self, GeometryError> {
        Ok(Self::PointerMove {
            position: dpi.logical_point(position)?,
        })
    }

    pub fn pointer_primary_from_physical(
        position: [f32; 2],
        phase: PointerPhase,
        dpi: DpiScale,
    ) -> Result<Self, GeometryError> {
        Ok(Self::PointerPrimary {
            position: dpi.logical_point(position)?,
            phase,
        })
    }

    pub fn pointer_secondary_from_physical(
        position: [f32; 2],
        phase: PointerPhase,
        dpi: DpiScale,
    ) -> Result<Self, GeometryError> {
        Ok(Self::PointerSecondary {
            position: dpi.logical_point(position)?,
            phase,
        })
    }

    pub fn scroll_from_physical(delta: [f32; 2], dpi: DpiScale) -> Result<Self, GeometryError> {
        Ok(Self::Scroll {
            delta: dpi.logical_point(delta)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    Pressed,
    Held,
    Released,
}

pub struct UiLimits;

impl UiLimits {
    pub const MAX_NODES: usize = 16_384;
    pub const MAX_TEXT_BYTES: usize = 16_384;
    pub const MAX_FOCUSABLE: usize = 4_096;
    pub const MAX_CLIP_DEPTH: usize = 32;
    pub const MAX_UI_VERTICES: usize = 262_144;
    pub const MAX_UI_INDICES: usize = 393_216;
    pub const MAX_DRAW_BATCHES: usize = 8_192;
    pub const MAX_DRAW_LIST_BYTES: usize = 16 * 1024 * 1024;
}
