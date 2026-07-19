use assets::HudTextureRole;
use ui::{BoundedStat, DpiScale, UiNode, UiNodeId, UiVisual};

use super::{HudTexturePages, UiPresentationError, UiRuntime, rect};

const VANILLA_SURVIVAL_POINTS: u16 = 20;

/// Geometry measured from the owned protocol-1001 client at the exact witnessed viewport.
/// Other viewport/settings combinations fail closed until independently measured.
#[derive(Clone, Copy)]
pub(super) struct MeasuredSurvivalHudGeometry {
    logical_texture_scale: f32,
    hotbar_outer_left: f32,
}

impl MeasuredSurvivalHudGeometry {
    pub(super) fn bottom_row_top(self, logical_height: f32) -> f32 {
        logical_height - 40.0 * self.logical_texture_scale
    }
}

pub(super) fn measured_geometry(
    physical_size: [u32; 2],
    dpi_scale: DpiScale,
) -> Option<MeasuredSurvivalHudGeometry> {
    (physical_size == [3433, 1385] && dpi_scale.get().to_bits() == 1.5_f32.to_bits()).then_some(
        MeasuredSurvivalHudGeometry {
            logical_texture_scale: 2.0,
            hotbar_outer_left: 962.0,
        },
    )
}

pub(super) fn append(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    runtime: &UiRuntime,
    height: f32,
    textures: &HudTexturePages,
    geometry: MeasuredSurvivalHudGeometry,
) -> Result<(), UiPresentationError> {
    let scale = geometry.logical_texture_scale;
    let outer_left = geometry.hotbar_outer_left;
    let inner_left = outer_left + scale;
    if let Some(health) = runtime.hud().health()
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
    if let Some(hunger) = runtime.hud().hunger()
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
    if let Some(armor) = runtime.hud().armor()
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
    if let Some(air) = runtime.hud().air()
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

    if let Some(equipment) = runtime.local_selected_equipment() {
        let hotbar_y = (height - 23.0 * scale).max(0.0);
        let roles = [
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
        let selected = usize::from(equipment.event.selected_slot);
        if selected >= roles.len() {
            return Ok(());
        }
        append_sprite(
            nodes,
            next_id,
            textures,
            HudTextureRole::HotbarStartCap,
            [outer_left, hotbar_y],
            [255, 255, 255, 166],
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
            [255, 255, 255, 166],
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
