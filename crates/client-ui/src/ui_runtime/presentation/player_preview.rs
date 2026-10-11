//! Native player-preview projection, direct UI geometry and CPU hand fallback.
//!
//! Both model paths use the standard-biped vertex and UV contract of the 3-D
//! actor renderer. Visible live-player/paper-doll controls use `geometry` and
//! original skin texels at destination framebuffer resolution; the cached CPU
//! raster is retained for compatibility and first-person fallback carriers.

use std::sync::Arc;

use assets::RuntimeEquipmentCatalog;

use {super::UiPresentationRuntime, ui::IconRef};

pub(super) mod cape;
pub(super) mod controller;
mod equipment;
mod fitting;
pub mod geometry;
pub(super) mod model;
mod skin;
pub use cape::render_cape_thumbnail;
pub use controller::MenuPreviewConfig;
pub use equipment::{
    PreviewEquipment, PreviewHandItem, PreviewHeldModel, PreviewHeldPlacement, PreviewTexture,
};
pub use model::render_skin_thumbnail;
use render_model::{ActorVertex, standard_biped_overlay_vertices, standard_biped_vertices};
pub use skin::local_preview_skin;

impl UiPresentationRuntime {
    /// Retain the CPU quad. Only exact current-render coverage may omit it in
    /// the overlay pass; a previous completion alone cannot remove this carrier.
    pub fn cpu_empty_hand_fallback(&self) -> Option<IconRef> {
        self.hud_frame.right_hand
    }

    /// [`Self::set_player_preview_skin`] that defers a pose change while no raster showing it is
    /// on screen; mouse look otherwise re-rasterizes and re-uploads the page every frame.
    pub fn sync_player_preview(
        &mut self,
        skin: Option<&[u8]>,
        pose: PlayerPreviewPose,
        preview_shown: bool,
        hands_shown: bool,
        seconds: f64,
    ) {
        // The model sways only while a renderer shows it.
        if preview_shown {
            self.player_preview_bob = bob_degrees(seconds);
        }
        let hands_changed = self.player_preview_raster_pose.is_none_or(|drawn| {
            drawn.pitch_degrees != pose.pitch_degrees || drawn.sneaking != pose.sneaking
        });
        let wanted =
            preview_shown || (hands_shown && hands_changed) || self.player_preview_pixels.is_none();
        // A skin change still redraws, at the pose already drawn.
        let model = match self.player_preview_pose {
            Some(drawn) if !wanted => drawn,
            _ => pose,
        };
        // Model geometry draws the preview itself, leaving the rasters only the hands, which
        // follow the pose only while they show.
        let raster = match self.player_preview_raster_pose {
            Some(drawn)
                if self.gui_models.enabled
                    && self.player_preview_pixels.is_some()
                    && !(hands_shown && hands_changed) =>
            {
                drawn
            }
            _ => model,
        };
        self.set_player_preview_poses(skin, model, raster);
    }

    /// Where worn armor textures come from; without it the model wears none.
    pub fn set_equipment_catalog(&mut self, catalog: Option<Arc<RuntimeEquipmentCatalog>>) {
        self.equipment_catalog = catalog;
    }

    /// Dresses the model in the local player's armor and held item, naming each
    /// stack's item through `identify`.
    pub fn dress_player_preview(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
        identify: impl Fn(&protocol::NetworkItemStack) -> Option<Arc<str>>,
    ) {
        use inventory::inventory_ledger::InventoryTarget;
        let ledger = runtime.inventory_ledger(player_runtime);
        let named = |stack: Option<&protocol::NetworkItemStack>| {
            stack.and_then(|stack| Some((identify(stack)?, stack.clone())))
        };
        let armor: [_; 4] = std::array::from_fn(|slot| {
            named(ledger.target_stack(InventoryTarget::Armor(slot as u8)))
        });
        let held = named(
            player_runtime
                .selected_hotbar_slot()
                .and_then(|slot| ledger.displayed_stack(slot)),
        );
        self.set_player_preview_gear(
            armor.each_ref().map(|worn| {
                worn.as_ref()
                    .map(|(id, stack)| (&**id, protocol::item_custom_color(&stack.extra_data)))
            }),
            held.as_ref().map(|(id, stack)| (&**id, stack.metadata)),
        );
        let off_hand = named(ledger.target_stack(InventoryTarget::Offhand));
        self.player_preview_gear.hands = [held, off_hand].map(|item| {
            let (identifier, stack) = item?;
            Some(PreviewHandItem {
                identifier,
                metadata: stack.metadata,
                charged_projectile: protocol::item_charged_projectile(&stack.extra_data),
            })
        });
    }

    /// Dresses armor and held items using session attachables first, then the base catalog.
    /// Armor follows world-player material rules; only leather applies the stack dye.
    pub fn set_player_preview_gear(
        &mut self,
        armor: [Option<(&str, Option<u32>)>; 4],
        held: Option<(&str, u32)>,
    ) {
        let pack = self.gui_models.pack_equipment.source.clone();
        let base = self.equipment_catalog.clone();
        let from_pack: [bool; 4] = std::array::from_fn(|slot| {
            armor[slot].is_some_and(|(identifier, _)| {
                pack.as_deref()
                    .is_some_and(|pack| pack.binding(identifier).is_some())
            })
        });
        let resolved: [_; 4] = std::array::from_fn(|slot| {
            let catalog = if from_pack[slot] { &pack } else { &base };
            worn_armor(catalog.as_deref(), armor[slot])
        });
        self.wear_pack_armor(std::array::from_fn(|slot| {
            let (_, texture) = resolved[slot].as_ref().filter(|_| from_pack[slot])?;
            Some(*texture)
        }));
        let armor = std::array::from_fn(|slot| {
            // Pack art the GUI atlas could not hold falls back to the item's base art.
            if from_pack[slot]
                && self.gui_models.enabled
                && resolved[slot].is_some()
                && self.gui_models.pack_equipment.region(slot).is_none()
            {
                return worn_armor(base.as_deref(), armor[slot]).map(|(preview, _)| preview);
            }
            resolved[slot].as_ref().map(|(preview, _)| preview.clone())
        });
        let hands = [
            held.map(|(identifier, metadata)| PreviewHandItem {
                identifier: Arc::from(identifier),
                metadata,
                charged_projectile: None,
            }),
            None,
        ];
        let held = held.and_then(|(identifier, metadata)| {
            let sprite = self.icon_catalog.as_ref()?.lookup(identifier, metadata)?;
            Some(PreviewTexture {
                rgba: Arc::clone(&sprite.rgba8),
                width: sprite.width,
                height: sprite.height,
                tint: None,
            })
        });
        self.player_preview_gear = PreviewEquipment { armor, held, hands };
    }

    /// The current model raster's RGBA pixels, empty before the first.
    #[cfg(test)]
    pub fn player_preview_raster(&self) -> Vec<u8> {
        self.player_preview_pixels
            .as_ref()
            .map_or_else(Vec::new, |rasters| rasters.preview.clone())
    }
}

/// The texture `catalog` binds to a worn `(item, dye)`, tinted only when the binding's
/// material is a color mask, and that texture's identifier.
fn worn_armor<'a>(
    catalog: Option<&'a RuntimeEquipmentCatalog>,
    worn: Option<(&str, Option<u32>)>,
) -> Option<(PreviewTexture, &'a str)> {
    let (identifier, dye) = worn?;
    let catalog = catalog?;
    let binding = catalog.binding(identifier)?;
    let texture = catalog.texture(&binding.texture.identifier)?;
    let tint = binding
        .color_mask_rgb(dye)
        .map(|rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
    let preview = PreviewTexture {
        rgba: Arc::clone(&texture.rgba8),
        width: texture.width,
        height: texture.height,
        tint,
    };
    Some((preview, &*texture.identifier))
}

pub const PREVIEW_WIDTH: u32 = 96;
pub const PREVIEW_HEIGHT: u32 = 112;
pub const HAND_WIDTH: u32 = 64;
pub const HAND_HEIGHT: u32 = 64;

/// Raster pixels per block, and where the model's feet and centre line sit.
pub const PREVIEW_PIXELS_PER_BLOCK: f32 = 48.0;
pub const PREVIEW_FEET_Y: f32 = 106.0;
/// A player's eye height above its feet, the pivot of the preview model's pitch.
pub const PLAYER_EYE_HEIGHT: f32 = protocol::STANDING_PLAYER_EYE_HEIGHT;
/// The player entity's render scale.
pub(super) const PLAYER_MODEL_SCALE: f32 = 0.9375;
/// The HUD translates its shared outer actor frame while swimming.
pub(super) const HUD_SWIM_OFFSET: f32 = 0.8;
/// UI rendering retains vanilla's model-part origin rather than the world feet origin.
pub const PLAYER_UI_ORIGIN: f32 = client_world::MODEL_PART_ORIGIN_Y / 16.0 * PLAYER_MODEL_SCALE;
const POINTER_DISTANCE_GUI_PIXELS: f32 = 40.0;
const POINTER_ANGLE_FACTOR: f32 = 20.0;
const HEAD_PIVOT: [f32; 3] = [0.0, 1.5, 0.0];
const SHOULDER_PIVOTS: [[f32; 3]; 2] = [
    [-5.0 / 16.0, 22.0 / 16.0, 0.0],
    [5.0 / 16.0, 22.0 / 16.0, 0.0],
];

/// How a UI renderer shows the player model; both face the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewView {
    /// `live_player_renderer`: the model turns toward the pointer, `offset` the
    /// renderer's centre minus the pointer in GUI pixels.
    Live { offset: [f32; 2] },
    /// `paper_doll_renderer`: a fixed turn (`starting_rotation`) under a camera tilt
    /// (`camera_tilt_degrees`), both in degrees.
    Doll { yaw: f32, tilt: f32 },
    /// A gesture-controlled paper doll keeps its camera tilt while its head follows the pointer.
    DollLook {
        yaw: f32,
        tilt: f32,
        offset: [f32; 2],
    },
    /// The skin selector retains menu head tracking while playing the player idle sway.
    SkinSelector {
        yaw: f32,
        tilt: f32,
        offset: [f32; 2],
    },
    /// Native fixed body yaw while the actor keeps its animated pose and relative head look.
    Hud,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self::Live { offset: [0.0; 2] }
    }
}

impl PreviewView {
    /// `(body yaw, head yaw, head pitch, model pitch)` in degrees. A live renderer
    /// turns the body `atan(dx / 40) * 20`, head
    /// `atan(dx / 40) * 40` and `atan(dy / 40) * -20`, the whole model tilted by
    /// `atan(dy / 40) * -20` about the eyes.
    fn angles(self) -> [f32; 4] {
        match self {
            Self::Live { offset: [dx, dy] } => {
                let (x, y) = (
                    (dx / POINTER_DISTANCE_GUI_PIXELS).atan(),
                    (dy / POINTER_DISTANCE_GUI_PIXELS).atan(),
                );
                [
                    x * POINTER_ANGLE_FACTOR,
                    x * POINTER_ANGLE_FACTOR * 2.0,
                    y * -POINTER_ANGLE_FACTOR,
                    y * -POINTER_ANGLE_FACTOR,
                ]
            }
            Self::Doll { yaw, tilt } => [yaw, yaw, 0.0, tilt],
            Self::DollLook {
                yaw,
                tilt,
                offset: [dx, dy],
            }
            | Self::SkinSelector {
                yaw,
                tilt,
                offset: [dx, dy],
            } => {
                let direction = if ((yaw + 180.0).rem_euclid(360.0) - 180.0).abs() > 90.0 {
                    -1.0
                } else {
                    1.0
                };
                [
                    yaw,
                    yaw + (dx / POINTER_DISTANCE_GUI_PIXELS).atan()
                        * POINTER_ANGLE_FACTOR
                        * direction,
                    (dy / POINTER_DISTANCE_GUI_PIXELS).atan() * -POINTER_ANGLE_FACTOR,
                    tilt,
                ]
            }
            Self::Hud => {
                // Vanilla fixes both HUD body-yaw samples.
                // rotate_y already uses the native yaw direction.
                let yaw = -22.5;
                [yaw, yaw, 0.0, 0.0]
            }
        }
    }
}

/// Builds the live preview or paper-doll pose and logical rect around the model-part origin.
/// Uses each renderer's scale and rotation; the doll also offsets by inverse GUI scale.
pub fn renderer_frame(
    renderer: &str,
    data: &std::collections::BTreeMap<String, serde_json::Value>,
    dest: [f32; 4],
    px: f32,
    pointer: Option<[f32; 2]>,
) -> (PreviewView, [f32; 4]) {
    let (w, h) = (dest[2] - dest[0], dest[3] - dest[1]);
    let centre = [(dest[0] + dest[2]) * 0.5, (dest[1] + dest[3]) * 0.5];
    let (view, block, anchor) = if renderer == "live_player_renderer" {
        let offset = pointer.map_or([0.0; 2], |point| {
            [centre[0] / px - point[0], centre[1] / px - point[1]]
        });
        // The eye offset the live renderer applies cancels a player's eye-level actor
        // position, leaving the actor's UI origin on the centre as the HUD's is.
        (PreviewView::Live { offset }, w.min(h), PLAYER_UI_ORIGIN)
    } else if renderer == "hud_player_renderer" {
        (PreviewView::Hud, w, PLAYER_UI_ORIGIN)
    } else {
        let offset = pointer.map_or([0.0; 2], |point| {
            [centre[0] / px - point[0], centre[1] / px - point[1]]
        });
        let view = MenuPreviewConfig::from_data(data).view(offset);
        // The native menu model retains its authored Y=24 origin. The
        // paper-doll renderer subtracts inverse GUI scale in model pixels.
        let anchor = PLAYER_UI_ORIGIN - 1.0 / (px * 16.0);
        (view, (w / 20.0).min(h / 39.0) * 16.0, anchor)
    };
    // Logical pixels per raster pixel; the anchor point lands on the centre.
    let scale = block / PREVIEW_PIXELS_PER_BLOCK;
    let anchor_y = PREVIEW_FEET_Y - anchor * PREVIEW_PIXELS_PER_BLOCK;
    let left = centre[0] - PREVIEW_WIDTH as f32 * 0.5 * scale;
    let top = centre[1] - anchor_y * scale;
    (
        view,
        [
            left,
            top,
            left + PREVIEW_WIDTH as f32 * scale,
            top + PREVIEW_HEIGHT as f32 * scale,
        ],
    )
}

/// The idle arm sway of `animation.player.bob`, in degrees, at `seconds` of life:
/// `cos(t * 103.2) * 2.865 + 2.865`. Geometry follows its continuous native
/// value; a CPU raster cache may quantize its own request independently.
pub fn bob_degrees(seconds: f64) -> f32 {
    let degrees = (seconds * 103.2).to_radians().cos() * 2.865 + 2.865;
    degrees as f32
}

/// Authoritative pose supplied to UI model geometry without angle quantization.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerPreviewPose {
    pub body_yaw_degrees: f32,
    pub head_yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub sneaking: bool,
}

/// Raster bundle retained so the presentation can rebuild its bounded dynamic
/// texture pages when menu artwork or held-item carriers change.
#[derive(Clone, Debug)]
pub struct PlayerPreviewRasters {
    pub preview: Vec<u8>,
    pub left_hand: Vec<u8>,
    pub right_hand: Vec<u8>,
}

impl PlayerPreviewPose {
    pub fn new(
        body_yaw_degrees: f32,
        head_yaw_degrees: f32,
        pitch_degrees: f32,
        sneaking: bool,
    ) -> Self {
        Self {
            body_yaw_degrees,
            head_yaw_degrees,
            pitch_degrees,
            sneaking,
        }
    }

    /// The local actor's pose; the default stance off-world.
    pub fn of_local_player(stream: Option<&chunk_pipeline::WorldStream>) -> Self {
        let Some(actor) =
            stream.and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        else {
            return Self::default();
        };
        let sneaking = matches!(
            actor.metadata.get(&0),
            Some(protocol::ActorMetadataValue::Flags(flags)) if flags & (1_u64 << 1) != 0
        );
        Self::new(actor.body_yaw, actor.head_yaw, actor.pitch, sneaking)
    }
}

#[derive(Clone, Copy)]
struct ProjectedVertex {
    screen: [f32; 2],
    depth: f32,
    uv: [f32; 2],
    world: [f32; 3],
}

/// Renders a nearest-neighbour, orthographic 3-D biped preview from a
/// validated packed player skin, facing the viewer as `view` turns it, its
/// arms swayed by `bob` degrees. Transparent pixels remain transparent.
pub fn render(
    skin: &[u8],
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    gear: &PreviewEquipment,
) -> Vec<u8> {
    let mut vertices = standard_biped_vertices();
    vertices.extend(standard_biped_overlay_vertices());
    render_body(&vertices, skin, pose, view, bob, gear)
}

pub(super) fn render_body(
    vertices: &[ActorVertex],
    skin: &[u8],
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    gear: &PreviewEquipment,
) -> Vec<u8> {
    render_body_with_cape(vertices, skin, pose, view, bob, gear, None)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_body_with_cape(
    vertices: &[ActorVertex],
    skin: &[u8],
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    gear: &PreviewEquipment,
    cape: Option<&protocol::CapeImage>,
) -> Vec<u8> {
    let width = PREVIEW_WIDTH as usize;
    let height = PREVIEW_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let rig = Rig::new(pose, view, bob, [gear.held.is_some(), false]);
    let mut draw = |vertices: &[ActorVertex], sample: &dyn Fn([f32; 2]) -> Option<[u8; 4]>| {
        for triangle in vertices.as_chunks::<3>().0 {
            let projected = [0, 1, 2].map(|corner| rig.project(triangle[corner]));
            rasterize_triangle(&mut pixels, &mut depth, width, height, sample, projected);
        }
    };
    draw(vertices, &|uv| sample_skin(skin, uv));
    if let Some(texture) = cape.and_then(cape::texture) {
        draw(cape::rest_vertices(), &|uv| {
            texture.sample(uv).filter(|texel| texel[3] >= 128)
        });
    }
    for (slot, texture) in gear.armor.iter().enumerate() {
        if let Some(texture) = texture {
            draw(&equipment::armor_vertices(slot), &|uv| texture.sample(uv));
        }
    }
    if let Some(item) = &gear.held {
        draw(&equipment::held_vertices(), &|uv| item.sample(uv));
    }
    pixels
}

/// A posed, viewer-facing biped.
struct Rig {
    parts: [Option<bevy::math::Affine3A>; 6],
    body: f32,
    head_yaw: f32,
    head_pitch: f32,
    model_pitch: f32,
    bob: f32,
    sneaking: bool,
    /// Each held item raises its own arm (`animation.player.holding`: -18 degrees).
    holding: [bool; 2],
}

impl Rig {
    fn new(pose: PlayerPreviewPose, view: PreviewView, bob: f32, holding: [bool; 2]) -> Self {
        let [body, head_yaw, head_pitch, model_pitch] = view.angles();
        // A vanilla paper doll sets variable.is_paperdoll=1. The vanilla player
        // controller's paperdoll branch excludes holding, sneak and idle bob.
        let is_live = !matches!(
            view,
            PreviewView::Doll { .. }
                | PreviewView::DollLook { .. }
                | PreviewView::SkinSelector { .. }
        );
        Self {
            parts: [None; 6],
            body: body.to_radians(),
            head_yaw: (head_yaw - body).to_radians(),
            head_pitch: head_pitch.to_radians(),
            model_pitch: model_pitch.to_radians(),
            bob: if is_live || matches!(view, PreviewView::SkinSelector { .. }) {
                bob.to_radians()
            } else {
                0.0
            },
            sneaking: is_live && pose.sneaking,
            holding: holding.map(|holding| is_live && holding),
        }
    }

    fn project(&self, vertex: ActorVertex) -> ProjectedVertex {
        let mut local = vertex.position;
        if let Some(Some(transform)) = self.parts.get(vertex.part as usize) {
            local = transform
                .transform_point3(bevy::math::Vec3::from_array(local))
                .to_array();
        } else {
            if self.sneaking {
                local = sneak_pose(local, vertex.part);
            }
            match vertex.part {
                0 => {
                    local = rotate_x(local, self.head_pitch, HEAD_PIVOT);
                    local = rotate_y(local, self.head_yaw, HEAD_PIVOT);
                }
                // The arms sway out from the shoulders.
                2 => {
                    let shoulder = SHOULDER_PIVOTS[0];
                    if self.holding[0] {
                        local = rotate_x(local, -18f32.to_radians(), shoulder);
                    }
                    local = rotate_z(local, -self.bob, shoulder);
                }
                3 => {
                    let shoulder = SHOULDER_PIVOTS[1];
                    if self.holding[1] {
                        local = rotate_x(local, -18f32.to_radians(), shoulder);
                    }
                    local = rotate_z(local, self.bob, shoulder);
                }
                _ => {}
            }
        }
        // The player renders at `scale: 0.9375` (player.entity.json).
        local = local.map(|axis| axis * PLAYER_MODEL_SCALE);
        // The model faces the viewer (its front is +Z, nearest), turned by the
        // body yaw, then tilts about its eyes.
        local = rotate_y(local, self.body, [0.0; 3]);
        let eye = [0.0, PLAYER_EYE_HEIGHT, 0.0];
        let world = rotate_x(local, self.model_pitch, eye);
        ProjectedVertex {
            screen: [
                PREVIEW_WIDTH as f32 * 0.5 + world[0] * PREVIEW_PIXELS_PER_BLOCK,
                PREVIEW_FEET_Y - world[1] * PREVIEW_PIXELS_PER_BLOCK,
            ],
            depth: world[2],
            uv: vertex.uv,
            world,
        }
    }
}

/// Renders one first-person arm from the authoritative player skin. Bedrock's
/// HUD has a distinct viewmodel path (`short_arm_offset_*` and
/// `player_arm_height` in the client), so this is deliberately separate from
/// the corner paper-doll. It gives empty hands and item carriers a real skin
/// silhouette while the full 3-D item viewmodel is still on the render-path
/// roadmap.
pub fn render_hand(skin: &[u8], pose: PlayerPreviewPose, left: bool) -> Vec<u8> {
    let width = HAND_WIDTH as usize;
    let height = HAND_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let part = if left { 2 } else { 3 };
    let mut vertices = standard_biped_vertices();
    vertices.extend(standard_biped_overlay_vertices());
    for triangle in vertices.as_chunks::<3>().0 {
        if triangle.iter().any(|vertex| vertex.part != part) {
            continue;
        }
        let projected = [
            project_hand(triangle[0], pose, left),
            project_hand(triangle[1], pose, left),
            project_hand(triangle[2], pose, left),
        ];
        rasterize_triangle(
            &mut pixels,
            &mut depth,
            width,
            height,
            &|uv| sample_skin(skin, uv),
            projected,
        );
    }
    pixels
}

fn rasterize_triangle(
    pixels: &mut [u8],
    depth: &mut [f32],
    width: usize,
    height: usize,
    sample: &dyn Fn([f32; 2]) -> Option<[u8; 4]>,
    projected: [ProjectedVertex; 3],
) {
    let area = edge(
        projected[0].screen,
        projected[1].screen,
        projected[2].screen,
    );
    if !area.is_finite() || area.abs() < f32::EPSILON {
        return;
    }
    let min_x = projected
        .iter()
        .map(|vertex| vertex.screen[0].floor() as i32)
        .min()
        .unwrap_or_default()
        .clamp(0, width as i32 - 1);
    let max_x = projected
        .iter()
        .map(|vertex| vertex.screen[0].ceil() as i32)
        .max()
        .unwrap_or_default()
        .clamp(0, width as i32 - 1);
    let min_y = projected
        .iter()
        .map(|vertex| vertex.screen[1].floor() as i32)
        .min()
        .unwrap_or_default()
        .clamp(0, height as i32 - 1);
    let max_y = projected
        .iter()
        .map(|vertex| vertex.screen[1].ceil() as i32)
        .max()
        .unwrap_or_default()
        .clamp(0, height as i32 - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    let shade = face_shade(projected[0].world, projected[1].world, projected[2].world);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let weights = [
                edge(projected[1].screen, projected[2].screen, point) / area,
                edge(projected[2].screen, projected[0].screen, point) / area,
                edge(projected[0].screen, projected[1].screen, point) / area,
            ];
            if weights.iter().any(|weight| *weight < 0.0) {
                continue;
            }
            let pixel_index = y as usize * width + x as usize;
            let candidate_depth = weights
                .iter()
                .zip(projected.iter())
                .map(|(weight, vertex)| *weight * vertex.depth)
                .sum::<f32>();
            if candidate_depth <= depth[pixel_index] {
                continue;
            }
            let uv = std::array::from_fn(|axis| {
                weights[0] * projected[0].uv[axis]
                    + weights[1] * projected[1].uv[axis]
                    + weights[2] * projected[2].uv[axis]
            });
            let Some(mut color) = sample(uv) else {
                continue;
            };
            if color[3] < 10 {
                continue;
            }
            color[0] = (f32::from(color[0]) * shade).round() as u8;
            color[1] = (f32::from(color[1]) * shade).round() as u8;
            color[2] = (f32::from(color[2]) * shade).round() as u8;
            depth[pixel_index] = candidate_depth;
            let target = pixel_index * 4;
            pixels[target..target + 4].copy_from_slice(&color);
        }
    }
}

fn project_hand(vertex: ActorVertex, pose: PlayerPreviewPose, left: bool) -> ProjectedVertex {
    let side = if left { -1.0 } else { 1.0 };
    let mut local = vertex.position;
    // Re-center the standard biped arm into a compact first-person carrier.
    // The two arms are uploaded into separate sprites, so each remains at the
    // edge of the view instead of inheriting the paper-doll's shoulder span.
    local[0] = (local[0] - side * 0.375) * 0.82 + side * 0.05;
    local[1] = (local[1] - 0.75) * 0.86 + 0.08;
    if pose.sneaking {
        local[1] -= 0.03;
    }
    let pitch = (-pose.pitch_degrees.to_radians() * 0.18)
        .clamp(-std::f32::consts::FRAC_PI_6, std::f32::consts::FRAC_PI_6);
    local = rotate_x(local, pitch - 0.34, [0.0, 0.18, 0.0]);
    // The first-person carrier exposes the top and inner faces of the arm.
    // Applying yaw before the inward roll preserves the cuboid silhouette;
    // the previous roll-only projection collapsed the depth axis and read as
    // a large flat rectangle even though the source geometry was 3-D.
    local = rotate_y(local, -side * 0.72, [0.0, 0.08, 0.0]);
    local = rotate_z(local, side * 0.24, [0.0, 0.08, 0.0]);
    ProjectedVertex {
        screen: [
            HAND_WIDTH as f32 * 0.5 + local[0] * 104.0,
            HAND_HEIGHT as f32 - 3.0 - local[1] * 55.0,
        ],
        depth: local[2],
        uv: vertex.uv,
        world: local,
    }
}

fn sneak_pose(mut point: [f32; 3], part: u32) -> [f32; 3] {
    match part {
        0 => {
            point[1] -= 0.22;
            point[2] += 0.16;
        }
        1..=3 => {
            point = rotate_x(point, -0.38, [0.0, 1.5, 0.0]);
            point[1] -= 0.04;
            point[2] += 0.08;
        }
        4 | 5 => {
            point[1] *= 0.92;
        }
        _ => {}
    }
    point
}

fn rotate_x(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let y = point[1] - pivot[1];
    let z = point[2] - pivot[2];
    let (sin, cos) = angle.sin_cos();
    point[1] = pivot[1] + y * cos - z * sin;
    point[2] = pivot[2] + y * sin + z * cos;
    point
}

fn rotate_y(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let x = point[0] - pivot[0];
    let z = point[2] - pivot[2];
    let (sin, cos) = angle.sin_cos();
    point[0] = pivot[0] + x * cos - z * sin;
    point[2] = pivot[2] + x * sin + z * cos;
    point
}

fn rotate_z(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let x = point[0] - pivot[0];
    let y = point[1] - pivot[1];
    let (sin, cos) = angle.sin_cos();
    point[0] = pivot[0] + x * cos - y * sin;
    point[1] = pivot[1] + x * sin + y * cos;
    point
}

fn edge(a: [f32; 2], b: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - a[0]) * (b[1] - a[1]) - (point[1] - a[1]) * (b[0] - a[0])
}

fn face_shade(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2])
        .sqrt()
        .max(f32::EPSILON);
    let light = [0.35, 0.8, 0.5];
    let dot = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length;
    (0.62 + dot.max(0.0) * 0.38).clamp(0.45, 1.0)
}

fn sample_skin(skin: &[u8], uv: [f32; 2]) -> Option<[u8; 4]> {
    let side = (skin.len() / 4).isqrt();
    if side == 0 || side * side * 4 != skin.len() || !uv.iter().all(|value| value.is_finite()) {
        return None;
    }
    let [x, y] = uv.map(|value| ((value * side as f32).floor() as usize).min(side - 1));
    let offset = (y * side + x) * 4;
    skin[offset..offset + 4].try_into().ok()
}
