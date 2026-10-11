use ui::{UiNode, UiNodeId, UiPoint, UiRect, UiVisual};

use super::{PreviewView, UiPresentationRuntime, renderer_frame};
use launcher::menu::MenuScreen;
use {
    crate::ui_runtime::presentation::{TextMetrics, UiPresentationError, rect},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

const DRAG_DEGREES_PER_GUI_PIXEL: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuPreviewConfig {
    pub starting_rotation: f32,
    pub camera_tilt_degrees: f32,
    pub gesture_rotation: bool,
    pub fit_to_control: bool,
    pub idle_animation: bool,
}

impl MenuPreviewConfig {
    pub const MENU: Self = Self {
        starting_rotation: 30.0,
        camera_tilt_degrees: -10.0,
        gesture_rotation: true,
        fit_to_control: false,
        idle_animation: false,
    };

    pub const DRESSING_ROOM: Self = Self {
        fit_to_control: true,
        idle_animation: true,
        ..Self::MENU
    };

    pub(super) fn from_data(data: &std::collections::BTreeMap<String, serde_json::Value>) -> Self {
        let number = |key: &str| data.get(key).and_then(serde_json::Value::as_f64);
        Self {
            starting_rotation: number("starting_rotation").unwrap_or(0.0) as f32,
            camera_tilt_degrees: number("camera_tilt_degrees").unwrap_or(0.0) as f32,
            gesture_rotation: data.get("rotation").and_then(serde_json::Value::as_str)
                == Some("gesture_x"),
            fit_to_control: false,
            idle_animation: false,
        }
    }

    pub(super) fn view(self, offset: [f32; 2]) -> PreviewView {
        if self.idle_animation {
            PreviewView::SkinSelector {
                yaw: self.starting_rotation,
                tilt: self.camera_tilt_degrees,
                offset,
            }
        } else if self.gesture_rotation {
            PreviewView::DollLook {
                yaw: self.starting_rotation,
                tilt: self.camera_tilt_degrees,
                offset,
            }
        } else {
            PreviewView::Doll {
                yaw: self.starting_rotation,
                tilt: self.camera_tilt_degrees,
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreviewControl {
    pub(crate) bounds: UiRect,
    pub(crate) gui_pixel: f32,
}

#[derive(Default)]
pub(crate) struct MenuPreview {
    screen: Option<MenuScreen>,
    rotations: [f32; 3],
    pub(crate) control: Option<PreviewControl>,
    pub(crate) pointer: Option<UiPoint>,
    captured: bool,
    previous: Option<UiPoint>,
}

impl MenuPreview {
    fn slot(&self) -> Option<usize> {
        match self.screen? {
            MenuScreen::Home => Some(0),
            MenuScreen::Pause => Some(1),
            MenuScreen::DressingRoom => Some(2),
            _ => None,
        }
    }

    fn select_screen(&mut self, screen: Option<MenuScreen>) {
        if self.screen != screen {
            self.control = None;
            self.cancel();
            self.screen = screen;
        }
    }

    pub(crate) fn begin_frame(&mut self, screen: Option<MenuScreen>) {
        self.select_screen(screen);
        self.control = None;
    }

    pub(crate) fn rotation(&self) -> f32 {
        self.slot().map_or(0.0, |slot| self.rotations[slot])
    }

    fn cancel(&mut self) {
        self.revoke_capture();
        self.pointer = None;
    }

    pub(crate) fn revoke_capture(&mut self) {
        self.captured = false;
        self.previous = None;
    }

    fn update(
        &mut self,
        screen: Option<MenuScreen>,
        point: Option<UiPoint>,
        held: bool,
        press: bool,
    ) -> bool {
        self.select_screen(screen);
        self.pointer = point;
        let Some(slot) = self.slot() else {
            self.cancel();
            return false;
        };
        if press {
            self.captured = point.is_some_and(|point| {
                self.control
                    .is_some_and(|control| control.bounds.contains(point))
            });
            self.previous = point;
        }
        let owns = self.captured;
        if held && self.captured {
            if let (Some(previous), Some(point), Some(control)) =
                (self.previous, point, self.control)
            {
                self.rotations[slot] = (self.rotations[slot]
                    - (point.x() - previous.x()) / control.gui_pixel * DRAG_DEGREES_PER_GUI_PIXEL)
                    .rem_euclid(360.0);
            }
            self.previous = point;
        } else {
            self.captured = false;
            self.previous = None;
        }
        owns
    }
}

impl PreviewView {
    pub(crate) fn with_menu_rotation(self, rotation: f32) -> Self {
        match self {
            Self::DollLook { yaw, tilt, offset } => Self::DollLook {
                yaw: yaw + rotation,
                tilt,
                offset,
            },
            Self::SkinSelector { yaw, tilt, offset } => Self::SkinSelector {
                yaw: yaw + rotation,
                tilt,
                offset,
            },
            _ => self,
        }
    }
}

impl UiPresentationRuntime {
    /// The interactive character's window-logical input rectangle in the current frame.
    pub fn menu_player_preview_bounds(&self) -> Option<UiRect> {
        self.menu_preview.control.map(|control| control.bounds)
    }

    /// Body yaw, head yaw, head pitch and camera tilt drawn by the current preview.
    pub fn menu_player_preview_angles(&self) -> [f32; 4] {
        self.player_preview_view.angles()
    }

    /// Captures a press on the character until release, retaining its final body rotation.
    pub fn menu_player_preview_pointer(
        &mut self,
        screen: Option<MenuScreen>,
        point: Option<UiPoint>,
        held: bool,
        press: bool,
    ) -> bool {
        let previous = (self.menu_preview.pointer, self.menu_preview.rotation());
        let owns = self.menu_preview.update(screen, point, held, press);
        if previous != (self.menu_preview.pointer, self.menu_preview.rotation()) {
            self.last_frame = None;
        }
        owns
    }

    /// Revokes pointer capture when another screen owns input or the window loses focus.
    pub fn cancel_menu_player_preview_input(&mut self) {
        if self.menu_preview.pointer.is_some() {
            self.last_frame = None;
        }
        self.menu_preview.cancel();
    }

    /// Registers and draws the shared interactive character under the caller's clip.
    #[allow(clippy::too_many_arguments)]
    pub fn append_menu_player_preview(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        control: [f32; 4],
        clip: [f32; 4],
        config: MenuPreviewConfig,
    ) -> Result<(), UiPresentationError> {
        let Some(icon) = self.player_preview_icon else {
            return Ok(());
        };
        let visible = [
            control[0].max(clip[0]),
            control[1].max(clip[1]),
            control[2].min(clip[2]),
            control[3].min(clip[3]),
        ];
        if visible[0] >= visible[2] || visible[1] >= visible[3] {
            self.menu_preview.control = None;
            return Ok(());
        }
        let gui_pixel = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let origin = [self.safe_area.left(), self.safe_area.top()];
        let frame = if config.fit_to_control {
            self.menu_preview_model
                .bounds
                .unwrap_or_else(super::fitting::standard)
                .frame(control, config.camera_tilt_degrees)
        } else {
            let data = std::collections::BTreeMap::new();
            renderer_frame("paper_doll_renderer", &data, control, gui_pixel, None).1
        };
        let centre = if config.fit_to_control {
            super::fitting::head_origin(frame, config.camera_tilt_degrees)
        } else {
            [
                (control[0] + control[2]) * 0.5,
                (control[1] + control[3]) * 0.5,
            ]
        };
        let offset = self.menu_preview.pointer.map_or([0.0; 2], |point| {
            [
                (centre[0] + origin[0] - point.x()) / gui_pixel,
                (centre[1] + origin[1] - point.y()) / gui_pixel,
            ]
        });
        self.player_preview_view = config
            .view(offset)
            .with_menu_rotation(self.menu_preview.rotation());
        if config.idle_animation {
            self.player_preview_bob = super::bob_degrees(self.menu_seconds);
        }
        if config.gesture_rotation {
            self.menu_preview.control = Some(PreviewControl {
                bounds: rect(
                    visible[0] + origin[0],
                    visible[1] + origin[1],
                    visible[2] + origin[0],
                    visible[3] + origin[1],
                )?,
                gui_pixel,
            });
        }
        let parent = UiNodeId::new(*next);
        *next += 1;
        nodes.push(
            UiNode::new(parent, None, rect(clip[0], clip[1], clip[2], clip[3])?)
                .with_clip_children(true),
        );
        let id = UiNodeId::new(*next);
        *next += 1;
        nodes.push(
            UiNode::new(
                id,
                Some(parent),
                rect(
                    frame[0] - clip[0],
                    frame[1] - clip[1],
                    frame[2] - clip[0],
                    frame[3] - clip[1],
                )?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [255; 4],
            }),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
