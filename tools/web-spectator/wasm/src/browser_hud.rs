//! Read-only streamed POV data through Cinnabar's authored JSON-UI HUD.
//! Native stat painters, font metrics, icon atlases and render publication are shared.

use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

use assets::{
    HudTextureRole, RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog, RuntimeUiAssets,
};
use json_ui::{
    Catalog, Context, Draw, DrawNode, HudModel, HudSlot, LayoutEnv, Sidebar, TextAlign,
    TextMeasure, TextureMeta, TextureSource, Timed, ViewState,
};
use render_model::{NametagScene, UiRenderInput, UiRenderTextureArray};
use view_presentation::{
    nametag_atlas::NametagAtlas,
    nametags::NametagAnchor,
    ui_adapter::UiRenderViewport,
    ui_atlas::HudTexturePages,
};
use ui::native_hud::{HudEffect, HudPaint, HudPaintTarget, SheetSprite, StatusPaintInput};
use ui::{
    BoundedStat, DpiScale, IconRef, SafeArea, TextLayoutCache, TextMetrics, UiNode, UiNodeId, UiPoint,
    UiRect, UiScale, UiTree, UiVisual,
};

use super::browser_model::{Fighter, Item};

pub(super) struct BrowserHud {
    catalog: Catalog,
    assets: RuntimeUiAssets,
    font: Arc<RuntimeFontCatalog>,
    icons: RuntimeIconCatalog,
    icon_refs: Box<[IconRef]>,
    textures: Arc<UiRenderTextureArray>,
    hud_pages: HudTexturePages,
    ui_first_page: u16,
    solid_page: u16,
    layouts: RefCell<TextLayoutCache>,
    cached: Option<(HudModel, [u32; 2], Vec<DrawNode>)>,
    animator: json_ui::Animator,
    title_request: Option<(String, String, u64)>,
    generation: u64,
    health: Option<(String, f32)>,
    last_health_drop_millis: Option<u64>,
    nametag_atlas: NametagAtlas,
}

impl BrowserHud {
    pub(super) fn new(
        json_ui_bytes: &[u8],
        hud_bytes: &[u8],
        icon_bytes: &[u8],
        font_bytes: &[u8],
    ) -> Result<Self, String> {
        let assets = RuntimeUiAssets::decode(json_ui_bytes).map_err(|e| e.to_string())?;
        let font = Arc::new(
            RuntimeFontCatalog::decode(
                font_bytes,
                assets::canonical_source_manifest_sha256(include_bytes!(
                    "../../../../assets/cinnangles-sans-source.json"
                )),
            )
                .map_err(|e| e.to_string())?,
        );
        let hud = RuntimeHudCatalog::decode(hud_bytes).map_err(|e| e.to_string())?;
        let icons = RuntimeIconCatalog::decode(icon_bytes).map_err(|e| e.to_string())?;
        if icons.source_manifest_sha256() != assets.source_manifest_sha256() {
            return Err("item icons do not belong to the JSON-UI source manifest".into());
        }
        let mut catalog = Catalog::from_files(
            assets
                .ui_files()
                .iter()
                .map(|file| (file.path.as_ref(), file.bytes.as_ref())),
        )
        .map_err(|e| format!("JSON-UI catalog: {e:?}"))?;
        catalog.apply_pack(
            ui::native_hud::JAVA_HUD_PACK
                .iter()
                .map(|(path, _, bytes)| (*path, *bytes)),
        );
        let (textures, solid_page, hud_pages, icon_refs) =
            view_presentation::ui_atlas::font_texture_array_with_hud_and_icons(&font, Some(&hud), Some(&icons))
                .map_err(|e| format!("native HUD atlas: {e:?}"))?;
        let (textures, ui_first_page) = view_presentation::ui_atlas::with_ui_pages(&textures, &assets)
            .map_err(|e| format!("JSON-UI atlas: {e:?}"))?;
        Ok(Self {
            catalog,
            assets,
            font,
            icons,
            icon_refs: icon_refs.ok_or("native item atlas did not produce icon placements")?,
            hud_pages: hud_pages.ok_or("native HUD atlas did not produce role placements")?,
            textures: Arc::new(textures),
            ui_first_page,
            solid_page,
            layouts: RefCell::new(TextLayoutCache::new(
                ui::DEFAULT_TEXT_CACHE_ENTRIES,
                ui::DEFAULT_TEXT_CACHE_BYTES,
            )),
            cached: None,
            animator: json_ui::Animator::default(),
            title_request: None,
            generation: 0,
            health: None,
            last_health_drop_millis: None,
            nametag_atlas: NametagAtlas::default(),
        })
    }

    /// The native billboard atlas shares the same compiled font and layout cache as the HUD.
    pub(super) fn nametag_scene(&mut self, anchors: &[NametagAnchor]) -> NametagScene {
        let font = &self.font;
        view_presentation::nametags::build_nametag_scene(
            anchors,
            font,
            &mut self.layouts.borrow_mut(),
            &mut self.nametag_atlas,
            &|page| view_presentation::nametag_atlas::font_page(font, page),
        )
    }

    pub(super) fn update(
        &mut self,
        fighter: Option<&Fighter>,
        viewport: [u32; 2],
        now_millis: u64,
        sampled_at_millis: u64,
    ) -> Result<UiRenderInput, String> {
        self.generation = self.generation.wrapping_add(1).max(1);
        let viewport = [viewport[0].max(1), viewport[1].max(1)];
        let dpi = DpiScale::new(1.0).map_err(|e| format!("HUD DPI: {e:?}"))?;
        let metrics = TextMetrics::for_viewport(viewport, dpi, None);
        let px = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let mut drawn_icons = Vec::new();
        let Some((fighter, pov)) =
            fighter.and_then(|fighter| fighter.pov.as_ref().map(|pov| (fighter, pov)))
        else {
            self.health = None;
            self.last_health_drop_millis = None;
            self.title_request = None;
            self.animator.end_frame();
            return self.publish(Vec::new(), viewport);
        };
        if self.health.as_ref().is_none_or(|(id, _)| id != &fighter.id) {
            self.cached = None;
            self.animator = json_ui::Animator::default();
        }
        if self
            .health
            .as_ref()
            .is_some_and(|(id, health)| id == &fighter.id && fighter.health < *health)
        {
            self.last_health_drop_millis = Some(now_millis);
        } else if self
            .health
            .as_ref()
            .is_some_and(|(id, _)| id != &fighter.id)
        {
            self.last_health_drop_millis = None;
        }
        self.health = Some((fighter.id.clone(), fighter.health));
        let slot = |item: Option<&Item>, selected: bool, drawn: &mut Vec<IconRef>| -> HudSlot {
            let icon = item.and_then(|item| {
                self.icons
                    .lookup_index(&item.name, item.meta.max(0) as u32)
                    .and_then(|index| self.icon_refs.get(index).copied())
                    .map(|icon| icon.with_glint(item.enchanted))
            });
            let icon = icon.map(|icon| {
                drawn.push(icon);
                drawn.len() - 1
            });
            HudSlot {
                icon,
                count: item.map_or(0, |item| item.count.max(0) as u32),
                selected,
                durability: item
                    .filter(|item| item.max_durability > 0 && item.durability >= 0)
                    .map(|item| {
                        (f64::from(item.durability) / f64::from(item.max_durability))
                            .clamp(0.0, 1.0)
                    }),
            }
        };
        let mut model = HudModel {
            survival_ui: true,
            armor_visible: pov.armour_points > 0.0,
            hotbar_visible: true,
            xp_bar: true,
            exp_progress: f64::from(pov.experience_progress.clamp(0.0, 1.0)),
            level: pov.experience_level.max(0) as u32,
            hotbar: pov
                .hotbar
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    slot(item.as_ref(), index == pov.selected_slot, &mut drawn_icons)
                })
                .collect(),
            offhand: fighter
                .equipment
                .as_ref()
                .and_then(|equipment| equipment.off_hand.as_ref())
                .map(|item| slot(Some(item), false, &mut drawn_icons)),
            bubbles_visible: pov
                .air_ticks
                .zip(pov.max_air_ticks)
                .is_some_and(|(current, maximum)| current < maximum),
            effects_visible: !pov.effects.is_empty(),
            sidebar: pov.hud.scoreboard.as_ref().map(|board| Sidebar {
                title: board.title.clone(),
                rows: board
                    .lines
                    .iter()
                    .map(|line| (line.clone(), String::new()))
                    .collect(),
                background_opacity: 0.3,
                title_background_opacity: 0.4,
            }),
            actionbar: pov.hud.action_bar.as_ref().map(|bar| Timed {
                text: bar.text.clone(),
                born: timestamp_seconds(&bar.updated_at, now_millis),
            }),
            // The authored selected-item label provides the native compact popup surface.
            item_name: pov.hud.popup.as_ref().map(|popup| Timed {
                text: popup.text.clone(),
                born: timestamp_seconds(&popup.updated_at, now_millis),
            }),
            ..HudModel::default()
        };
        model.title = pov.hud.title.as_ref().map(|title| {
            let creation_id = self.title_request.as_ref()
                .filter(|(id, updated, _)| id == &fighter.id && updated == &title.updated_at)
                .map_or(self.generation, |(_, _, sequence)| *sequence);
            self.title_request = Some((fighter.id.clone(), title.updated_at.clone(), creation_id));
            json_ui::HudTitle {
            creation_id,
            title: title.text.clone(),
            subtitle: title.subtitle.clone(),
            fade_in: f64::from(title.fade_in_ticks.max(0)) / 20.0,
            stay: f64::from(title.stay_ticks.max(0)) / 20.0,
            fade_out: f64::from(title.fade_out_ticks.max(0)) / 20.0,
            background_alpha: 0.0,
            born: timestamp_seconds(&title.updated_at, now_millis),
            }
        });
        if model.title.is_none() {
            self.title_request = None;
        }
        let context = json_ui::hud_context(&Context::retail(false));
        if self
            .cached
            .as_ref()
            .is_none_or(|(cached, size, _)| cached != &model || *size != viewport)
        {
            let measure = Measure {
                layouts: &self.layouts,
                font: &self.font,
                metrics,
                px,
            };
            let source = Textures {
                assets: &self.assets,
                hud: &self.hud_pages,
                ui_first_page: self.ui_first_page,
            };
            let env = LayoutEnv {
                text: &measure,
                textures: &source,
            };
            let root = [
                f64::from(viewport[0]) / f64::from(px),
                f64::from(viewport[1]) / f64::from(px),
            ];
            let mut nodes = Vec::new();
            for (reference, data) in [
                (json_ui::HUD_SCREEN, json_ui::hud_data_source(&model)),
                (json_ui::CROSSHAIR_SCREEN, json_ui::DataSource::new()),
            ] {
                let screen = json_ui::render_screen(
                    reference,
                    &self.catalog,
                    &context,
                    &data,
                    root,
                    &env,
                    &ViewState::default(),
                )
                .ok_or_else(|| format!("authored HUD screen {reference} is missing"))?;
                nodes.extend(screen.nodes);
            }
            self.cached = Some((model.clone(), viewport, nodes));
        }
        let tick = now_millis / 50;
        let effects = pov
            .effects
            .iter()
            .map(|effect| HudEffect {
                effect_id: effect.id,
                amplifier: effect.level,
                ambient: false,
                particles: !effect.particles_hidden,
                expires_at_tick: (!effect.infinite).then_some(
                    (sampled_at_millis / 50).saturating_add(effect.duration_ticks.max(0) as u64),
                ),
            })
            .collect::<Vec<_>>();
        let variant = ui::native_hud::heart_variant(&effects, Some(tick), 0.0);
        let input = StatusPaintInput {
            health: bounded(fighter.health, fighter.max_health),
            absorption: bounded(pov.absorption, pov.absorption.max(1.0)),
            armor: bounded(pov.armour_points, 20.0),
            hunger: bounded(pov.food as f32, 20.0),
            air: pov
                .air_ticks
                .zip(pov.max_air_ticks)
                .and_then(|(value, maximum)| bounded(value as f32, maximum as f32)),
            heart_variant: variant,
            regenerating: ui::native_hud::regeneration_active(&effects, Some(tick)),
            hardcore: false,
            hunger_effect: ui::native_hud::hunger_effect_active(&effects, Some(tick)),
            saturation_empty: false,
            effects: &effects,
            now_tick: Some(tick),
            now_millis,
            last_health_drop_millis: self.last_health_drop_millis,
            first_person: true,
            hotbar_allowed: true,
            survival_stats_visible: true,
            crosshair_blend: ui::UiBlendMode::Alpha,
            mount_health: None,
            mount_jump: None,
        };
        let sheet = |role| SheetSprite {
            page: self.hud_pages.page,
            uv: self.hud_pages.sprite(role).uv,
        };
        let paint = ui::native_hud::capture_status_hud(&input, Some(&sheet));
        let clocks = json_ui::hud_clocks(&model);
        let mut painter = Painter {
            textures: Textures {
                assets: &self.assets,
                hud: &self.hud_pages,
                ui_first_page: self.ui_first_page,
            },
            font: &self.font,
            layouts: &self.layouts,
            metrics,
            px,
            solid_page: self.solid_page,
            icons: &drawn_icons,
            animator: &mut self.animator,
            nodes: Vec::new(),
            clip: [0.0, 0.0, viewport[0] as f32, viewport[1] as f32],
            next: 1,
        };
        for node in &self.cached.as_ref().expect("HUD layout installed").2 {
            painter.paint(node, &paint, &clocks, now_millis as f64 / 1_000.0)?;
        }
        painter.animator.take_events();
        painter.animator.end_frame();
        let nodes = painter.nodes;
        self.publish(nodes, viewport)
    }

    fn publish(&self, nodes: Vec<UiNode>, viewport: [u32; 2]) -> Result<UiRenderInput, String> {
        let mut tree = UiTree::new(nodes).map_err(|e| format!("HUD retained tree: {e:?}"))?;
        tree.layout(
            rect([0.0, 0.0, viewport[0] as f32, viewport[1] as f32])?,
            UiScale::new_display(1.0).map_err(|e| format!("HUD scale: {e:?}"))?,
            SafeArea::default(),
        )
        .map_err(|e| format!("HUD retained layout: {e:?}"))?;
        let mut draw = tree
            .build_draw_list()
            .map_err(|e| format!("HUD retained draw: {e:?}"))?;
        draw.revision = self.generation;
        view_presentation::ui_adapter::adapt_ui_draw_list(
            &draw,
            Arc::clone(&self.textures),
            UiRenderViewport {
                physical_size: viewport,
                dpi_scale: DpiScale::new(1.0).map_err(|e| format!("HUD DPI: {e:?}"))?,
                safe_area: SafeArea::default(),
            },
        )
        .map_err(|e| e.to_string())
    }
}

struct Textures<'a> {
    assets: &'a RuntimeUiAssets,
    hud: &'a HudTexturePages,
    ui_first_page: u16,
}
impl Textures<'_> {
    fn sprite(&self, path: &str, uv: json_ui::UvRect, color: [u8; 4], filter: json_ui::SpriteFilter) -> Option<UiVisual> {
        let path = path.trim_end_matches(".png");
        let (page, [x0, y0, x1, y1]) = if let Some(texture) = self.assets.texture(path) {
            (
                self.ui_first_page.checked_add(texture.page)?,
                [
                    texture.x,
                    texture.y,
                    texture.x.checked_add(texture.width)?,
                    texture.y.checked_add(texture.height)?,
                ],
            )
        } else {
            let role = HudTextureRole::ALL
                .iter()
                .copied()
                .find(|role| role.source_path().trim_end_matches(".png") == path)?;
            (self.hud.page, self.hud.sprite(role).uv)
        };
        let pixel = |origin: u16, end: u16, value: f32| {
            (f32::from(origin) + f32::from(end - origin) * value).round() as u16
        };
        let uv = [
                pixel(x0, x1, uv.u0),
                pixel(y0, y1, uv.v0),
                pixel(x0, x1, uv.u1),
                pixel(y0, y1, uv.v1),
            ];
        let style = (u8::from(filter.grayscale) * ui::UI_STYLE_GRAYSCALE)
            | (u8::from(filter.bilinear) * ui::UI_STYLE_BILINEAR);
        Some(if style == 0 {
            UiVisual::Sprite { texture_page: page, uv, color }
        } else {
            UiVisual::StyledSprite { texture_page: page, uv, color, style }
        })
    }
}
impl TextureSource for Textures<'_> {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        let path = path.trim_end_matches(".png");
        let size = self
            .assets
            .texture(path)
            .map(|texture| [texture.width, texture.height])
            .or_else(|| {
                HudTextureRole::ALL
                    .iter()
                    .copied()
                    .find(|role| role.source_path().trim_end_matches(".png") == path)
                    .map(|role| self.hud.sprite(role).size)
            })?.map(f64::from);
        Some(match self.assets.sidecar(path) {
            Some(sidecar) => TextureMeta {
                base_size: if sidecar.base_size == [0.0; 2] { size } else { sidecar.base_size.map(f64::from) },
                pixels: size,
                nineslice: sidecar.nineslice.map(|slice| json_ui::NineSlice {
                    left: f64::from(slice.left),
                    top: f64::from(slice.top),
                    right: f64::from(slice.right),
                    bottom: f64::from(slice.bottom),
                }),
            },
            None => TextureMeta::plain(size),
        })
    }
}

struct Measure<'a> {
    layouts: &'a RefCell<TextLayoutCache>,
    font: &'a RuntimeFontCatalog,
    metrics: TextMetrics,
    px: f32,
}
impl TextMeasure for Measure<'_> {
    fn extent(&self, text: &str) -> [f64; 2] {
        self.wrapped(text, 65_536.0 / f64::from(self.px))
    }
    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        if text.is_empty() {
            return [0.0, 0.0];
        }
        let request =
            self.metrics
                .request(text, width64(max_width * f64::from(self.px)), self.font);
        self.layouts
            .borrow_mut()
            .layout(request)
            .map(|layout| {
                layout
                    .size_64()
                    .map(|size| f64::from(size) / 64.0 / f64::from(self.px))
            })
            .unwrap_or([0.0; 2])
    }
}

struct Painter<'a> {
    textures: Textures<'a>,
    font: &'a RuntimeFontCatalog,
    layouts: &'a RefCell<TextLayoutCache>,
    metrics: TextMetrics,
    px: f32,
    solid_page: u16,
    icons: &'a [IconRef],
    animator: &'a mut json_ui::Animator,
    nodes: Vec<UiNode>,
    clip: [f32; 4],
    next: u32,
}
impl Painter<'_> {
    fn paint(
        &mut self,
        node: &DrawNode,
        hud: &HudPaint,
        clocks: &BTreeMap<String, f64>,
        seconds: f64,
    ) -> Result<(), String> {
        if !node.shown(&ViewState::default()) {
            return Ok(());
        }
        let drawn = node.animate(self.animator, seconds, Some(clocks), Some(&self.textures));
        let physical = |r: &json_ui::RectOut| {
            [
                r.x as f32 * self.px,
                r.y as f32 * self.px,
                (r.x + r.w) as f32 * self.px,
                (r.y + r.h) as f32 * self.px,
            ]
        };
        let dest = physical(&drawn.dest);
        self.clip = physical(&drawn.clip);
        let opacity = drawn.opacity.clamp(0.0, 1.0);
        if drawn.hidden || opacity <= 0.0 || self.clip[2] <= self.clip[0] || self.clip[3] <= self.clip[1]
            || dest[2] <= dest[0] || dest[3] <= dest[1] {
            return Ok(());
        }
        let alpha = |mut color: [u8; 4]| {
            color[3] = (f32::from(color[3]) * opacity).round() as u8;
            color
        };
        match &node.draw {
            Draw::Solid { color } => self.solid(dest, alpha(*color)),
            Draw::Sprite { texture, uv, color, filter } => {
                let uv = drawn.uv.unwrap_or(*uv);
                let color = drawn.color.unwrap_or(*color);
                if let Some(visual) = self.textures.sprite(texture, uv, alpha(color), *filter) {
                    self.push(visual, dest);
                }
            }
            Draw::Text {
                text,
                color,
                shadow,
                align,
                scale,
                ..
            } => {
                let mut request =
                    self.metrics
                        .request(text, width64(f64::from(dest[2] - dest[0])), self.font);
                request.scale = UiScale::new_display(self.metrics.scale.get() * *scale)
                    .map_err(|e| format!("HUD text scale: {e:?}"))?;
                let layout = self
                    .layouts
                    .borrow_mut()
                    .layout(request)
                    .map_err(|e| format!("HUD text: {e:?}"))?;
                let size = layout.size_64().map(|value| value as f32 / 64.0);
                let slack = (dest[2] - dest[0] - size[0]).max(0.0);
                let offset = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => slack * 0.5,
                    TextAlign::Right => slack,
                };
                self.push(
                    UiVisual::Text {
                        layout,
                        color: alpha(*color),
                        shadow: if *shadow {
                            self.metrics.shadow()
                        } else {
                            ui::TextShadow::None
                        },
                    },
                    [
                        dest[0] + offset,
                        dest[1],
                        dest[0] + offset + size[0].max(1.0),
                        dest[1] + size[1],
                    ],
                );
            }
            Draw::Custom { renderer, data } => {
                let number = |name| data.get(name).and_then(serde_json::Value::as_f64);
                let index = number("#collection_index").unwrap_or(0.0).clamp(0.0, 8.0) as usize;
                let notches = number("#bar_notches").unwrap_or(0.0).clamp(0.0, 64.0) as u32;
                if ui::native_hud::paint(self, hud, renderer, index, notches, dest, &alpha) {
                    return Ok(());
                }
                match renderer.as_str() {
                    "inventory_item_renderer" => {
                        if let Some(icon) = number("#item_renderer_data")
                            .and_then(|index| self.icons.get(index as usize))
                        {
                            self.push(icon.visual(alpha([255; 4])), dest);
                        }
                    }
                    "progress_bar_renderer" => {
                        if let Some(paint) = view_presentation::progress::capture_progress(data, dest, self.px) {
                            for rectangle in paint.rects() {
                                self.solid(rectangle.bounds, alpha(rectangle.color));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
impl HudPaintTarget for Painter<'_> {
    fn gui_pixel_scale(&self) -> f32 {
        self.px
    }
    fn visible_bounds(&self) -> [f32; 4] {
        self.clip
    }
    fn sprite(&self, path: &str, color: [u8; 4]) -> Option<UiVisual> {
        self.textures.sprite(path, json_ui::UvRect::full(), color, Default::default())
    }
    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]) {
        let Ok(clip) = rect(self.clip) else {
            return;
        };
        let Ok(bounds) = rect([
            bounds[0] - self.clip[0],
            bounds[1] - self.clip[1],
            bounds[2] - self.clip[0],
            bounds[3] - self.clip[1],
        ]) else {
            return;
        };
        let parent = UiNodeId::new(self.next);
        self.next = self.next.saturating_add(1);
        self.nodes
            .push(UiNode::new(parent, None, clip).with_clip_children(true));
        let id = UiNodeId::new(self.next);
        self.next = self.next.saturating_add(1);
        self.nodes
            .push(UiNode::new(id, Some(parent), bounds).with_visual(visual));
    }
    fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]) {
        self.push(
            UiVisual::Solid {
                texture_page: self.solid_page,
                color,
            },
            bounds,
        );
    }
}

fn bounded(current: f32, maximum: f32) -> Option<BoundedStat> {
    if !current.is_finite() || !maximum.is_finite() || maximum <= 0.0 {
        return None;
    }
    BoundedStat::new_scaled(
        (current.clamp(0.0, maximum) * 10.0)
            .ceil()
            .min(u16::MAX as f32) as u16,
        (maximum * 10.0).ceil().clamp(1.0, u16::MAX as f32) as u16,
        10,
    )
}
fn rect(bounds: [f32; 4]) -> Result<UiRect, String> {
    UiRect::new(
        UiPoint::new(bounds[0], bounds[1]).map_err(|e| format!("HUD point: {e:?}"))?,
        UiPoint::new(bounds[2], bounds[3]).map_err(|e| format!("HUD point: {e:?}"))?,
    )
    .map_err(|e| format!("HUD rect: {e:?}"))
}
fn width64(value: f64) -> u32 {
    (value.clamp(1.0, 65_536.0) * 64.0).ceil() as u32
}
fn timestamp_seconds(value: &str, fallback: u64) -> f64 {
    let millis = js_sys::Date::parse(value);
    if millis.is_finite() {
        millis / 1_000.0
    } else {
        fallback as f64 / 1_000.0
    }
}
