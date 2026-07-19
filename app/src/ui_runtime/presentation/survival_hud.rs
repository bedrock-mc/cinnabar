use assets::HudTextureRole;
use ui::{BoundedStat, UiNode, UiNodeId, UiVisual};

use super::{HudTexturePages, UiPresentationError, UiRuntime, rect};

const VANILLA_SURVIVAL_POINTS: u16 = 20;
const PINNED_CLASSIC_GUI_LOGICAL_SCALE: f32 = 2.0;
// `hud_screen.json`'s pinned `start_cap_image` authority specifies alpha 0.65.
// Converting that normalized channel to the renderer's byte channel rounds to 166.
const VANILLA_HOTBAR_CAP_ALPHA: u8 = 166;
const HOTBAR_ROLES: [HudTextureRole; 9] = [
    HudTextureRole::Hotbar0,
    HudTextureRole::Hotbar1,
    HudTextureRole::Hotbar2,
    HudTextureRole::Hotbar3,
    HudTextureRole::Hotbar4,
    HudTextureRole::Hotbar5,
    HudTextureRole::Hotbar6,
    HudTextureRole::Hotbar7,
    HudTextureRole::Hotbar8,
];

/// Responsive geometry from the pinned protocol-1001 official sample HUD authority.
///
/// The pack's `hud_screen.json` anchors the 182x22 hotbar to `bottom_middle`; its start cap,
/// nine slots, and end cap supply that source width. The pinned official sample classic profile
/// establishes a logical texture scale of two. Neither authority depends on window resolution.
#[derive(Clone, Copy)]
pub(super) struct ResponsiveSurvivalHudGeometry {
    logical_texture_scale: f32,
    hotbar_outer_left: f32,
}

impl ResponsiveSurvivalHudGeometry {
    pub(super) fn bottom_row_top(self, logical_height: f32) -> f32 {
        logical_height - 40.0 * self.logical_texture_scale
    }
}

pub(super) fn responsive_geometry(
    logical_width: f32,
    textures: &HudTexturePages,
) -> Option<ResponsiveSurvivalHudGeometry> {
    if !logical_width.is_finite() || logical_width <= 0.0 {
        return None;
    }

    let start = textures.sprite(HudTextureRole::HotbarStartCap).size;
    let end = textures.sprite(HudTextureRole::HotbarEndCap).size;
    let selected = textures.sprite(HudTextureRole::SelectedHotbarSlot).size;
    if start != [1, 22] || end != [1, 22] || selected != [24, 24] {
        return None;
    }
    let mut source_width = start[0];
    for role in HOTBAR_ROLES {
        let slot = textures.sprite(role).size;
        if slot != [20, 22] {
            return None;
        }
        source_width = source_width.checked_add(slot[0])?;
    }
    source_width = source_width.checked_add(end[0])?;

    let logical_outer_width = f32::from(source_width) * PINNED_CLASSIC_GUI_LOGICAL_SCALE;
    if logical_width < logical_outer_width {
        return None;
    }
    Some(ResponsiveSurvivalHudGeometry {
        logical_texture_scale: PINNED_CLASSIC_GUI_LOGICAL_SCALE,
        hotbar_outer_left: (logical_width - logical_outer_width) * 0.5,
    })
}

pub(super) fn append(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    runtime: &UiRuntime,
    height: f32,
    textures: &HudTexturePages,
    geometry: ResponsiveSurvivalHudGeometry,
) -> Result<(), UiPresentationError> {
    let scale = geometry.logical_texture_scale;
    let outer_left = geometry.hotbar_outer_left;
    let inner_left = outer_left + scale;
    if runtime.survival_stats_visible()
        && let Some(health) = runtime.hud().health()
        && let Some(half_units) = standard_survival_half_units(health)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [inner_left - scale, (height - 40.0 * scale).max(0.0)],
            false,
            half_units,
            HudTextureRole::HeartBackground,
            HudTextureRole::HeartFull,
            HudTextureRole::HeartHalf,
            scale,
        )?;
    }
    if runtime.survival_stats_visible()
        && let Some(hunger) = runtime.hud().hunger()
        && let Some(half_units) = standard_survival_half_units(hunger)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [inner_left + 171.0 * scale, (height - 40.0 * scale).max(0.0)],
            true,
            half_units,
            HudTextureRole::HungerBackground,
            HudTextureRole::HungerFull,
            HudTextureRole::HungerHalf,
            scale,
        )?;
    }
    if runtime.survival_stats_visible()
        && let Some(armor) = runtime.hud().armor()
        && armor.current() > 0
        && let Some(half_units) = standard_survival_half_units(armor)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [inner_left - scale, (height - 50.0 * scale).max(0.0)],
            false,
            half_units,
            HudTextureRole::ArmorEmpty,
            HudTextureRole::ArmorFull,
            HudTextureRole::ArmorHalf,
            scale,
        )?;
    }
    if runtime.survival_stats_visible()
        && let Some(air) = runtime.hud().air()
        && air.current() < air.maximum()
    {
        let filled = u32::from(air.current())
            .saturating_mul(10)
            .div_ceil(u32::from(air.maximum()))
            .min(10) as usize;
        for index in 0..10 {
            let role = if index < filled {
                HudTextureRole::BubbleFull
            } else {
                HudTextureRole::BubbleEmpty
            };
            append_sprite(
                nodes,
                next_id,
                textures,
                role,
                [
                    inner_left + (171.0 - index as f32 * 8.0) * scale,
                    (height - 50.0 * scale).max(0.0),
                ],
                [255; 4],
                scale,
            )?;
        }
    }

    if let Some(selected) = runtime.selected_hotbar_slot() {
        let hotbar_y = (height - 23.0 * scale).max(0.0);
        let roles = HOTBAR_ROLES;
        let selected = usize::from(selected);
        if selected >= roles.len() {
            return Ok(());
        }
        append_sprite(
            nodes,
            next_id,
            textures,
            HudTextureRole::HotbarStartCap,
            [outer_left, hotbar_y],
            [255, 255, 255, VANILLA_HOTBAR_CAP_ALPHA],
            scale,
        )?;
        for (index, role) in roles.into_iter().enumerate() {
            append_sprite(
                nodes,
                next_id,
                textures,
                role,
                [inner_left + index as f32 * 20.0 * scale, hotbar_y],
                [255; 4],
                scale,
            )?;
        }
        append_sprite(
            nodes,
            next_id,
            textures,
            HudTextureRole::HotbarEndCap,
            [outer_left + 181.0 * scale, hotbar_y],
            [255, 255, 255, VANILLA_HOTBAR_CAP_ALPHA],
            scale,
        )?;
        append_sprite(
            nodes,
            next_id,
            textures,
            HudTextureRole::SelectedHotbarSlot,
            [
                outer_left + (selected as f32 * 20.0 - 2.0) * scale,
                hotbar_y - 2.0 * scale,
            ],
            [255; 4],
            scale,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_icon_row(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    textures: &HudTexturePages,
    origin: [f32; 2],
    reverse: bool,
    half_units: u16,
    background: HudTextureRole,
    full: HudTextureRole,
    half: HudTextureRole,
    scale: f32,
) -> Result<(), UiPresentationError> {
    for index in 0..10 {
        let direction = if reverse { -1.0 } else { 1.0 };
        let position = [
            origin[0] + direction * index as f32 * 8.0 * scale,
            origin[1],
        ];
        append_sprite(
            nodes, next_id, textures, background, position, [255; 4], scale,
        )?;
        let remaining = half_units.saturating_sub(index as u16 * 2);
        let foreground = if remaining >= 2 {
            Some(full)
        } else if remaining == 1 {
            Some(half)
        } else {
            None
        };
        if let Some(role) = foreground {
            append_sprite(nodes, next_id, textures, role, position, [255; 4], scale)?;
        }
    }
    Ok(())
}

fn append_sprite(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    textures: &HudTexturePages,
    role: HudTextureRole,
    position: [f32; 2],
    color: [u8; 4],
    scale: f32,
) -> Result<(), UiPresentationError> {
    let sprite = textures.sprite(role);
    let size = [
        f32::from(sprite.size[0]) * scale,
        f32::from(sprite.size[1]) * scale,
    ];
    nodes.push(
        UiNode::new(
            UiNodeId::new(*next_id),
            None,
            rect(
                position[0],
                position[1],
                position[0] + size[0],
                position[1] + size[1],
            )?,
        )
        .with_visual(UiVisual::Sprite {
            texture_page: textures.page,
            uv: sprite.uv,
            color,
        }),
    );
    *next_id = (*next_id).saturating_add(1);
    Ok(())
}

fn standard_survival_half_units(stat: BoundedStat) -> Option<u16> {
    let maximum = u32::from(stat.maximum());
    let scale = u32::from(stat.scale());
    if maximum != u32::from(VANILLA_SURVIVAL_POINTS) * scale {
        return None;
    }
    u16::try_from(u32::from(stat.current()).div_ceil(scale))
        .ok()
        .map(|value| value.min(VANILLA_SURVIVAL_POINTS))
}
