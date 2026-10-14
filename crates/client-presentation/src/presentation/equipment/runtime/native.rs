//! Texture-only weapon attachables use native sprite meshes and hand placement.

use actor_animation::AttachableAnimationInput;
use assets::{EntityAssetKind, EquipmentBinding};
use render_model::equipment::{ItemDisplay, attach_to_bone, held_sprite_display};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeItem {
    Bow,
    Crossbow,
    Spear,
}

impl NativeItem {
    /// Limits the presentation choice to the supported built-in weapon families.
    fn from_identifier(identifier: &str) -> Option<Self> {
        match identifier.strip_prefix("minecraft:")? {
            "bow" => Some(Self::Bow),
            "crossbow" => Some(Self::Crossbow),
            "trident" => Some(Self::Spear),
            name if name.ends_with("_spear") => Some(Self::Spear),
            _ => None,
        }
    }

    /// Selects authored aliases from the native use frame, including loaded projectiles.
    fn alias(self, frame: u32) -> &'static str {
        match (self, frame) {
            (Self::Bow, 1) => "bow_pulling_0",
            (Self::Bow, 2) => "bow_pulling_1",
            (Self::Bow, 3..) => "bow_pulling_2",
            (Self::Crossbow, 1) => "crossbow_pulling_0",
            (Self::Crossbow, 2) => "crossbow_pulling_1",
            (Self::Crossbow, 3) => "crossbow_pulling_2",
            (Self::Crossbow, 4) => "crossbow_arrow",
            (Self::Crossbow, 5..) => "crossbow_rocket",
            (Self::Spear, 1..) => "raised",
            _ => "default",
        }
    }

    /// Offhand sprites stay idle; main-hand art follows admitted use timing and loaded state.
    fn frame(self, item: &WornItem, input: AttachableAnimationInput<'_>) -> u32 {
        if input.off_hand {
            return if self == Self::Crossbow {
                inventory::crossbow_animation_frame(None, 0, item.charged_projectile, false)
            } else {
                0
            };
        }
        match self {
            Self::Bow => inventory::ranged_animation_frame(
                assets::item_use::ItemUseAnimation::Bow,
                input.use_elapsed_ticks,
                input.max_use_ticks,
            ),
            Self::Crossbow => {
                if input.first_person || input.animation_frame > 0 {
                    input.animation_frame
                } else {
                    inventory::crossbow_animation_frame(
                        input.use_elapsed_ticks,
                        input.max_use_ticks,
                        item.charged_projectile,
                        false,
                    )
                }
            }
            Self::Spear => u32::from(input.use_elapsed_ticks.is_some()),
        }
    }
}

/// Compares every declared model with startup weapon or stock sprite geometry once per reload.
pub(super) fn recognize(
    pack: &RuntimeEntityAssets,
    catalog: Option<&RuntimeEquipmentCatalog>,
    base: &RuntimeEntityAssets,
    startup: Option<&RuntimeEquipmentCatalog>,
) -> BTreeMap<Box<str>, NativeItem> {
    let Some(startup) = startup else {
        return BTreeMap::new();
    };
    let Some(catalog) = catalog else {
        return BTreeMap::new();
    };
    let inherited = startup.bindings().iter().filter(|binding| {
        catalog.binding(&binding.identifier).is_none()
            && NativeItem::from_identifier(&binding.identifier).is_some_and(|kind| {
                (0..=5).any(|frame| {
                    let alias = kind.alias(frame);
                    binding
                        .resources
                        .textures
                        .get(alias)
                        .or_else(|| (alias == "default").then_some(&binding.texture.identifier))
                        .is_some_and(|path| catalog.texture(path).is_some())
                })
            })
    });
    catalog
        .bindings()
        .iter()
        .chain(inherited)
        .filter_map(|binding| {
            let candidate_assets = if catalog.binding(&binding.identifier).is_some() {
                pack
            } else {
                base
            };
            let kind = NativeItem::from_identifier(&binding.identifier)?;
            let stock = startup.binding(&binding.identifier)?;
            let mut references = geometry_names(base, stock);
            references.retain(|name| match kind {
                NativeItem::Bow => name.starts_with("geometry.bow_"),
                NativeItem::Crossbow => name.starts_with("geometry.crossbow_"),
                NativeItem::Spear => matches!(*name, "geometry.spear" | "geometry.trident"),
            });
            references.push("geometry.item_sprite");
            let stock_models = references
                .into_iter()
                .filter_map(|name| {
                    let index = find_geometry_index(base, name)? as usize;
                    let geometry = &base.geometries()[index];
                    Some((
                        geometry.texture_width,
                        geometry.texture_height,
                        render_model::resolve_geometry_bones(base, index).ok()?,
                    ))
                })
                .collect::<Vec<_>>();
            let names = geometry_names(candidate_assets, binding);
            let recognized = !names.is_empty()
                && names.into_iter().all(|name| {
                    let Some(index) =
                        find_geometry_index(candidate_assets, name).map(|index| index as usize)
                    else {
                        return false;
                    };
                    let geometry = &candidate_assets.geometries()[index];
                    let Ok(bones) = render_model::resolve_geometry_bones(candidate_assets, index)
                    else {
                        return false;
                    };
                    stock_models.iter().any(|(width, height, stock_bones)| {
                        *width == geometry.texture_width
                            && *height == geometry.texture_height
                            && *stock_bones == bones
                    })
                });
            recognized.then(|| (binding.identifier.clone(), kind))
        })
        .collect()
}

/// Includes declared aliases and compiled candidates, including older startup carriers.
fn geometry_names<'a>(
    assets: &'a RuntimeEntityAssets,
    binding: &'a EquipmentBinding,
) -> Vec<&'a str> {
    let mut names = binding
        .resources
        .geometry
        .values()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>();
    names.push(&binding.geometry.identifier);
    for rig in assets.rig_bindings().iter().filter(|rig| {
        let symbol = &assets.symbols()[rig.entity_symbol as usize];
        symbol.kind == EntityAssetKind::Attachable && symbol.identifier == binding.identifier
    }) {
        names.extend(
            assets.rig_geometries()[rig.first_geometry as usize..]
                [..usize::from(rig.geometry_count)]
                .iter()
                .map(|candidate| {
                    assets.geometries()[candidate.geometry as usize]
                        .identifier
                        .as_ref()
                }),
        );
    }
    names.sort_unstable();
    names.dedup();
    names
}

impl EquipmentRuntime {
    /// Whether the current pack's complete model set is recognized as texture-only.
    pub fn uses_native_item_mesh(&self, identifier: &str) -> bool {
        self.pack
            .as_ref()
            .is_some_and(|pack| pack.native_items.contains_key(identifier))
    }

    /// Generates the selected native sprite, falling back to the matching startup item frame.
    pub(super) fn held_mesh_with_use(
        &mut self,
        item: &WornItem,
        input: AttachableAnimationInput<'_>,
    ) -> Option<(EntityRigId, ActorArtworkLocation, bool, [f32; 2])> {
        let Some(kind) = self
            .pack
            .as_ref()
            .and_then(|pack| pack.native_items.get(item.identifier.as_ref()))
            .copied()
        else {
            return self.held_mesh(item, true);
        };
        let frame = kind.frame(item, input);
        if let Some((texture, location)) = self.native_texture(item, kind.alias(frame)) {
            let key = (location.page(), location.layer());
            let mesh = if let Some(mesh) = self.native_meshes.get(&key) {
                *mesh
            } else {
                let vertices = held_sprite_vertices(
                    usize::from(texture.width),
                    usize::from(texture.height),
                    &texture.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )?;
                let mesh = self.build_item_mesh(|id| {
                    ActorRigGeometry::new(id, vertices, vec![[0.0; 3]]).ok()
                })?;
                self.native_meshes.insert(key, mesh);
                mesh
            };
            return Some((mesh, location, false, [1.0; 2]));
        }
        let mut fallback = item.clone();
        fallback.metadata = if kind == NativeItem::Spear {
            item.metadata
        } else {
            frame
        };
        // Missing stages use startup icons; a session's default icon must not hide that stage.
        fallback.kind = HeldKind::Sprite;
        let index = self.held_icon_index(&fallback)?;
        let placement = self.placements.get(index).copied().flatten()?;
        let location = self
            .atlas_locations
            .get(placement.layer)
            .copied()
            .flatten()?;
        let mesh = self.mesh_for(MeshKey::Sprite(index), index, placement)?;
        let [u0, v0, u1, v1] = placement.uv_rect();
        Some((mesh, location, false, [u1 - u0, v1 - v0]))
    }

    /// Resolves exact stage artwork; absent stages never fall back to the pack's standby image.
    fn native_texture(
        &self,
        item: &WornItem,
        alias: &str,
    ) -> Option<(assets::EquipmentTexture, ActorArtworkLocation)> {
        let pack = self.pack.as_ref()?;
        let startup = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.binding(&item.identifier));
        let binding = pack
            .catalog
            .as_ref()?
            .binding(&item.identifier)
            .or(startup)?;
        let path = |binding: &EquipmentBinding| {
            binding
                .resources
                .textures
                .get(alias)
                .cloned()
                .or_else(|| (alias == "default").then(|| binding.texture.identifier.clone()))
        };
        for identifier in path(binding).into_iter().chain(startup.and_then(path)) {
            if let Some(texture) = pack.catalog.as_ref()?.texture(&identifier)
                && let Some(location) = pack.texture_locations.get(&identifier)
            {
                return Some((texture.clone(), *location));
            }
            if let Some(texture) = self
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.texture(&identifier))
                && let Some(location) = self.texture_locations.get(&identifier)
            {
                return Some((texture.clone(), *location));
            }
        }
        // A raised spear may reuse its normal texture when the pack has no raised alias.
        (alias == "raised")
            .then(|| self.native_texture(item, "default"))
            .flatten()
    }

    /// Native offhand placement uses the selected image's aspect ratio.
    pub(super) fn native_sprite_dimensions(&self, item: &WornItem) -> Option<[u16; 2]> {
        let kind = *self
            .pack
            .as_ref()?
            .native_items
            .get(item.identifier.as_ref())?;
        let frame = kind.frame(
            item,
            AttachableAnimationInput {
                off_hand: true,
                ..Default::default()
            },
        );
        if let Some((texture, _)) = self.native_texture(item, kind.alias(frame)) {
            return Some([texture.width, texture.height]);
        }
        let mut fallback = item.clone();
        fallback.metadata = if kind == NativeItem::Spear {
            item.metadata
        } else {
            frame
        };
        let sprite = self.icons.sprites().get(self.held_icon_index(&fallback)?)?;
        Some([sprite.width, sprite.height])
    }

    /// Places the native weapon on its actual hand or Java arm without attachable channels.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_native_held(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        bones: &BodyBones,
        java: Option<JavaGrip>,
        input: AttachableAnimationInput<'_>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some((mesh, location, _, span)) = self.held_mesh_with_use(item, input) else {
            return;
        };
        let java = java.filter(|_| !input.off_hand);
        let hand = if java.is_some() {
            bones.right_arm
        } else if input.off_hand {
            bones.left_item
        } else {
            bones.right_item
        };
        let Some(hand) = hand else { return };
        let display = if java.is_some() {
            let grip = if item.identifier.as_ref() == "minecraft:bow" {
                render_model::java_animation::JavaHeldItem::Bow
            } else {
                render_model::java_animation::JavaHeldItem::Flat
            };
            ItemDisplay::from_matrix(render_model::java_animation::third_person_item(
                grip,
                render_model::java_animation::JavaItemMesh::Sprite,
            ))
        } else {
            held_sprite_display(item.grip.upright().unwrap_or(false))
        };
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(hand),
            body.input.current_bones.get(hand),
        ) else {
            return;
        };
        let (Some(previous), Some(current)) = (
            attach_to_bone(*previous, display),
            attach_to_bone(*current, display),
        ) else {
            return;
        };
        let poses = self.poses.share(body, layer, [&[previous], &[current]]);
        layers.push(push::held_glint(
            layer_presentation(body, layer, mesh, poses, location, 0),
            item,
            span,
        ));
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
