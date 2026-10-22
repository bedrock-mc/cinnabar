//! Equipped cosmetic effects simulated in the paper doll's isolated UI frame.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use particles::{EmptyWorld, ParticleSystem, ParticleView, SpawnRequest, TileRequest};
use render_model::{ActorEmitterTransform, ActorVertex, UiTexturePage, hats};
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

use super::super::{
    UiPresentationRuntime,
    player_preview::{self, Rig},
};

type Key = (cosmetics::Item, usize);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Menu,
    Hud(Option<(u64, u64, u64)>),
}

#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct PreviewParticles {
    assets: Option<Arc<assets::RuntimeParticleAssets>>,
    system: Option<ParticleSystem>,
    live: HashMap<Key, u64>,
    rejected: HashSet<Arc<str>>,
    owner: Option<Owner>,
    placements: Vec<(Key, ActorEmitterTransform)>,
    last: Option<f64>,
    patch_seq: Option<u64>,
    pub(in crate::ui_runtime::presentation) page: Option<UiTexturePage>,
    pub(super) mesh: Option<Arc<UiMesh>>,
}

impl UiPresentationRuntime {
    /// Supplies original particle textures; authored cosmetic sprites also work without a carrier.
    pub fn set_gui_particle_assets(&mut self, assets: Option<Arc<assets::RuntimeParticleAssets>>) {
        self.gui_models.particles = PreviewParticles {
            assets,
            ..Default::default()
        };
        self.rebuild_dynamic_textures();
        self.last_frame = None;
    }

    /// Advances only a painted player control, independently of the world particle system.
    pub(super) fn sync_player_particles(&mut self, shown: bool) {
        let previews = &mut self.gui_models.particles;
        let owner = if self.player_preview_view == player_preview::PreviewView::Hud {
            Owner::Hud(self.gui_models.live_player.emitter_owner)
        } else {
            Owner::Menu
        };
        if previews.owner != Some(owner) {
            previews.reset();
            previews.owner = Some(owner);
        }
        previews.placements.clear();
        if shown {
            let rig = Rig::new(
                self.player_preview_pose.unwrap_or_default(),
                self.player_preview_view,
                self.player_preview_bob,
                self.player_preview_gear
                    .hands
                    .each_ref()
                    .map(Option::is_some),
            );
            if self.player_preview_view == player_preview::PreviewView::Hud {
                for &(item, index, transform) in &self.gui_models.live_player.emitters {
                    previews.placements.push((
                        (item, index),
                        project_emitter(
                            &rig,
                            transform,
                            render_model::STANDARD_BIPED_BONES.len() as u32,
                        ),
                    ));
                }
            } else {
                for &item in &self.menu_preview_model.cosmetics {
                    let model = hats::rig(item);
                    let Some(part) = model.rest_vertices.first().map(|vertex| vertex.part) else {
                        continue;
                    };
                    let motion = self.gui_models.cosmetics.motion(item);
                    for index in 0..model.emitters().len() {
                        if let Some(transform) = model.preview_emitter_transform(index, motion) {
                            previews
                                .placements
                                .push(((item, index), project_emitter(&rig, transform, part)));
                        }
                    }
                }
            }
        }
        let page = (self.textures.dynamic_start() + render_model::UI_PARTICLE_PAGE_OFFSET) as u16;
        if previews.sync(self.menu_seconds, page) {
            self.rebuild_dynamic_textures();
        }
    }
}

impl PreviewParticles {
    /// Drops transient effects after hiding, changing owner or starting a different session.
    pub(in crate::ui_runtime::presentation) fn reset(&mut self) {
        if let Some(system) = &mut self.system {
            system.clear();
        }
        self.live.clear();
        self.last = None;
        self.mesh = None;
    }

    /// Retires hidden previews and updates emitter placement before advancing caller-owned time.
    fn sync(&mut self, now: f64, page: u16) -> bool {
        self.mesh = None;
        if self.placements.is_empty() {
            self.reset();
            return false;
        }
        let system = self.system.get_or_insert_with(|| {
            self.assets
                .as_deref()
                .map_or_else(ParticleSystem::default, ParticleSystem::from_assets)
        });
        self.live.retain(|key, id| {
            if self.placements.iter().any(|(active, _)| active == key) && system.is_emitting(*id) {
                true
            } else {
                system.stop(*id);
                false
            }
        });
        for &(key @ (item, index), transform) in &self.placements {
            if let Some(&id) = self.live.get(&key) {
                system.set_transform(id, transform.position, transform.basis);
                continue;
            }
            let emitter = &hats::rig(item).emitters()[index];
            if self.rejected.contains(&emitter.identifier) {
                continue;
            }
            if !system.has_effect(&emitter.identifier) && !system.register_effect(&emitter.effect) {
                self.rejected.insert(Arc::clone(&emitter.identifier));
                continue;
            }
            let request = SpawnRequest {
                effect: emitter.identifier.to_string(),
                position: transform.position,
                basis: Some(transform.basis),
                seed: u64::from(item.index()) << 8 | index as u64,
                tile: emitter.sprite.as_ref().map(|sprite| TileRequest {
                    key: sprite.key,
                    size: sprite.side,
                    pixels: Arc::clone(&sprite.pixels),
                }),
                ..Default::default()
            };
            if let Some(id) = system.spawn(&request) {
                self.live.insert(key, id);
            }
        }
        let dt = self
            .last
            .replace(now)
            .map_or(0.0, |last| (now - last).max(0.0) as f32);
        system.tick(dt, &EmptyWorld);
        // Preview effects never play audio into the live session.
        system.take_sounds();
        let view = ParticleView {
            position: [0.0, 1.0, 16.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.5,
        };
        let draw = system.build_draw(&view, &EmptyWorld);
        self.mesh = particle_mesh(&draw, page, system.atlas().side());
        let seq = system.atlas().patch_seq();
        if self.patch_seq == Some(seq) {
            return false;
        }
        let side = system.atlas().side();
        self.page = UiTexturePage::owned([side; 2], Arc::from(system.atlas().pixels())).ok();
        self.patch_seq = Some(seq);
        true
    }
}

/// Applies the exact preview part and camera transforms to an origin and its three scaled axes.
fn project_emitter(
    rig: &Rig,
    transform: ActorEmitterTransform,
    part: u32,
) -> ActorEmitterTransform {
    let project = |position| {
        rig.project(ActorVertex {
            position,
            part,
            uv: [0.0; 2],
        })
        .world
    };
    let position = project(transform.position);
    let basis = transform.basis.map(|axis| {
        let end = project(std::array::from_fn(|i| transform.position[i] + axis[i]));
        std::array::from_fn(|i| end[i] - position[i])
    });
    ActorEmitterTransform { position, basis }
}

/// Preserves authored billboard axes, flipbook UVs, tint, blending and depth within the player mesh.
fn particle_mesh(draw: &particles::DrawLists, page: u16, side: u32) -> Option<Arc<UiMesh>> {
    let mut vertices = Vec::new();
    let mut batches = Vec::new();
    for (instances, opaque, additive) in [
        (&draw.opaque, true, false),
        (&draw.blend, false, false),
        (&draw.add, false, true),
    ] {
        let start = u32::try_from(vertices.len()).ok()?;
        for instance in instances {
            for [x, y] in [
                [-1.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [-1.0, -1.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ] {
                let point: [f32; 3] = std::array::from_fn(|i| {
                    instance.center_light[i] + instance.axis_x[i] * x + instance.axis_y[i] * y
                });
                let uv = [
                    instance.uv[0] + (x * 0.5 + 0.5) * instance.uv[2],
                    instance.uv[1] + (0.5 - y * 0.5) * instance.uv[3],
                ]
                .map(|v| v * side as f32);
                vertices.push(UiMeshVertex {
                    position: [
                        (player_preview::PREVIEW_WIDTH as f32 * 0.5
                            + point[0] * player_preview::PREVIEW_PIXELS_PER_BLOCK)
                            / player_preview::PREVIEW_WIDTH as f32,
                        (player_preview::PREVIEW_FEET_Y
                            - point[1] * player_preview::PREVIEW_PIXELS_PER_BLOCK)
                            / player_preview::PREVIEW_HEIGHT as f32,
                    ],
                    clip_z: point[2],
                    clip_w: 1.0,
                    uv,
                    color: instance
                        .color
                        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
                    model_light: 1.0,
                    overlay_color: instance.texture_bounds.map(|value| value * side as f32),
                    style_flags: u16::try_from(
                        render_model::UI_STYLE_PARTICLE
                            | if additive {
                                render_model::UI_STYLE_ADDITIVE
                            } else {
                                0
                            },
                    )
                    .expect("UI particle styles fit vertex flags"),
                    alpha_test: instance.axis_x[3] > 0.5,
                });
            }
        }
        let end = u32::try_from(vertices.len()).ok()?;
        if end > start {
            batches.push(UiMeshBatch {
                texture_page: page,
                index_range: start..end,
                blend: UiBlendMode::Alpha,
                depth_test: true,
                depth_write: opaque,
                alpha_cutoff: None,
            });
        }
    }
    if vertices.is_empty() {
        return None;
    }
    UiMesh::new(
        vertices.into(),
        (0..batches.last()?.index_range.end)
            .collect::<Vec<_>>()
            .into(),
        batches.into(),
    )
    .ok()
    .map(Arc::new)
}

#[cfg(test)]
mod tests;
