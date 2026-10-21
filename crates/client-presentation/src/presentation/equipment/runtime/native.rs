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

/// Recognized models retain whether their attachable artwork is a complete sprite.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeArtwork {
    kind: NativeItem,
    texture_is_sprite: bool,
    startup_texture_is_sprite: bool,
}

impl NativeItem {
    /// Limits the presentation choice to the supported built-in weapon families.
    fn from_identifier(identifier: &str) -> Option<Self> {
        let traits = assets::facts::ItemFacts::builtin()
            .ok()?
            .by_identifier(identifier)?
            .traits;
        if traits.contains(assets::facts::ItemTraits::BOW) {
            Some(Self::Bow)
        } else if traits.contains(assets::facts::ItemTraits::CROSSBOW) {
            Some(Self::Crossbow)
        } else if traits.contains(assets::facts::ItemTraits::TRIDENT)
            || traits.contains(assets::facts::ItemTraits::SPEAR)
        {
            Some(Self::Spear)
        } else {
            None
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

    /// Resolves declared artwork or the built-in stage path omitted by older carriers.
    fn texture_path(self, binding: &EquipmentBinding, alias: &str) -> Option<Box<str>> {
        binding
            .resources
            .textures
            .get(alias)
            .cloned()
            .or_else(|| (alias == "default").then(|| binding.texture.identifier.clone()))
            .or_else(|| {
                (binding.resources.textures.is_empty()
                    && alias != "default"
                    && matches!(self, Self::Bow | Self::Crossbow))
                .then(|| {
                    let texture = if alias == "crossbow_rocket" {
                        "crossbow_firework"
                    } else {
                        alias
                    };
                    format!("textures/items/{texture}").into_boxed_str()
                })
            })
    }

    /// Selects startup animation icons when the matching stage texture is unavailable.
    fn icon_key(self, item: &WornItem, frame: u32) -> (&str, u32) {
        match (self, frame) {
            (Self::Bow, 1..) => ("minecraft:bow_pulling", frame - 1),
            (Self::Crossbow, 1..) => ("minecraft:crossbow_pulling", frame - 1),
            (Self::Spear, _) => (&item.identifier, item.metadata),
            _ => (&item.identifier, 0),
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

/// Recognizes stock weapon models and full-texture planes once per reload.
pub(super) fn recognize(
    pack: &RuntimeEntityAssets,
    catalog: Option<&RuntimeEquipmentCatalog>,
    base: &RuntimeEntityAssets,
    startup: Option<&RuntimeEquipmentCatalog>,
) -> BTreeMap<Box<str>, NativeArtwork> {
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
                    kind.texture_path(binding, kind.alias(frame))
                        .is_some_and(|path| catalog.texture(&path).is_some())
                })
            })
    });
    catalog
        .bindings()
        .iter()
        .chain(inherited)
        .filter_map(|binding| {
            let (candidate_assets, candidates) = if catalog.binding(&binding.identifier).is_some() {
                (pack, catalog)
            } else {
                (base, startup)
            };
            // An item-wide shortcut cannot represent owner-selected definitions.
            if candidates.bindings_for(&binding.identifier).len() != 1
                || binding.attachable.is_some()
                || candidate_assets
                    .attachable_rig_binding(binding.attachable_identifier())
                    .is_some_and(|rig| {
                        candidate_assets.rig_bindings()[rig]
                            .item_condition
                            .is_some()
                    })
            {
                return None;
            }
            let kind = NativeItem::from_identifier(&binding.identifier)?;
            let stock = startup.binding(&binding.identifier)?;
            let mut references = geometry_names(base, stock);
            let startup_texture_is_sprite = references.iter().all(|name| {
                find_geometry_index(base, name).is_some_and(|index| {
                    render_model::resolve_geometry_bones(base, index as usize).is_ok_and(|bones| {
                        sprite_artwork(&bones)
                            || full_texture_plane(&base.geometries()[index as usize], &bones)
                    })
                })
            });
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
            let mut texture_is_sprite = true;
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
                    let plane = full_texture_plane(geometry, &bones);
                    texture_is_sprite &= sprite_artwork(&bones) || plane;
                    plane
                        || stock_models.iter().any(|(width, height, stock_bones)| {
                            *width == geometry.texture_width
                                && *height == geometry.texture_height
                                && *stock_bones == bones
                        })
                });
            recognized.then(|| {
                (
                    binding.identifier.clone(),
                    NativeArtwork {
                        kind,
                        texture_is_sprite,
                        startup_texture_is_sprite,
                    },
                )
            })
        })
        .collect()
}

/// Only texture-mesh models without cubes sample a complete sprite instead of a UV atlas.
fn sprite_artwork(bones: &[assets::EntityGeometryBone]) -> bool {
    bones.iter().all(|bone| bone.cubes.is_empty())
        && bones.iter().any(|bone| !bone.texture_meshes.is_empty())
}

/// A single uninflated plane covering its complete texture is sprite artwork, not a UV atlas.
fn full_texture_plane(
    geometry: &assets::EntityGeometry,
    bones: &[assets::EntityGeometryBone],
) -> bool {
    use assets::{EntityGeometryScalar as Scalar, EntityGeometryUv};

    if bones.iter().any(|bone| {
        !bone.texture_meshes.is_empty()
            || bone.inflate.is_some_and(|inflate| inflate != Scalar::ZERO)
            || bone.mirror == Some(true)
            || bone.never_render == Some(true)
    }) {
        return false;
    }
    let mut cubes = bones.iter().flat_map(|bone| bone.cubes.iter());
    let Some(cube) = cubes.next() else {
        return false;
    };
    cubes.next().is_none()
        && cube.size.map(Scalar::get)
            == [
                f32::from(geometry.texture_width),
                f32::from(geometry.texture_height),
                0.0,
            ]
        && cube.uv == EntityGeometryUv::Box([Scalar::ZERO; 2])
        && cube.inflate == Scalar::ZERO
        && !cube.mirror
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
            .and_then(|pack| pack.native_items.get(identifier))
            .is_some_and(|artwork| {
                artwork.texture_is_sprite
                    || self.session_sprite(identifier, 0).is_some()
                    || self
                        .icons
                        .lookup_index(identifier, 0)
                        .and_then(|index| self.placements.get(index).copied().flatten())
                        .and_then(|placement| self.atlas_locations.get(placement.layer))
                        .is_some_and(Option::is_some)
            })
    }

    /// Generates the selected native sprite, falling back to the matching startup item frame.
    pub(super) fn held_mesh_with_use(
        &mut self,
        item: &WornItem,
        input: AttachableAnimationInput<'_>,
    ) -> Option<(EntityRigId, ActorArtworkLocation, bool, [f32; 2])> {
        let Some(artwork) = self
            .pack
            .as_ref()
            .and_then(|pack| pack.native_items.get(item.identifier.as_ref()))
            .copied()
        else {
            return self.held_mesh(item, true);
        };
        if !artwork.texture_is_sprite {
            let sprite = WornItem {
                kind: HeldKind::Sprite,
                ..item.clone()
            };
            return self.held_mesh(&sprite, true);
        }
        let kind = artwork.kind;
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
        // Missing stages use startup icons; a session's default icon must not hide that stage.
        let (identifier, metadata) = kind.icon_key(item, frame);
        let index = self.icons.lookup_index(identifier, metadata)?;
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
        let artwork = pack.native_items.get(item.identifier.as_ref())?;
        if !artwork.texture_is_sprite {
            return None;
        }
        let startup = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.binding(&item.identifier));
        let binding = pack
            .catalog
            .as_ref()?
            .binding(&item.identifier)
            .or(startup)?;
        for identifier in artwork.kind.texture_path(binding, alias).into_iter().chain(
            startup
                .filter(|_| artwork.startup_texture_is_sprite)
                .and_then(|binding| artwork.kind.texture_path(binding, alias)),
        ) {
            if let Some(texture) = pack.catalog.as_ref()?.texture(&identifier)
                && let Some(location) = pack.texture_locations.get(&identifier)
            {
                return Some((texture.clone(), location.location));
            }
            if artwork.startup_texture_is_sprite
                && let Some(texture) = self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.texture(&identifier))
                && let Some(location) = self.texture_locations.get(&identifier)
            {
                return Some((texture.clone(), location.location));
            }
        }
        // A raised spear may reuse its normal texture when the pack has no raised alias.
        (alias == "raised")
            .then(|| self.native_texture(item, "default"))
            .flatten()
    }

    /// Native offhand placement uses the selected image's aspect ratio.
    pub(super) fn native_sprite_dimensions(&self, item: &WornItem) -> Option<[u16; 2]> {
        let artwork = *self
            .pack
            .as_ref()?
            .native_items
            .get(item.identifier.as_ref())?;
        if !artwork.texture_is_sprite {
            let sprite = self
                .session_sprite(&item.identifier, item.metadata)
                .and_then(|(index, _, _)| self.session_sprite_pixels(index))
                .or_else(|| self.icons.sprites().get(self.held_icon_index(item)?))?;
            return Some([sprite.width, sprite.height]);
        }
        let kind = artwork.kind;
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
        let (identifier, metadata) = kind.icon_key(item, frame);
        let sprite = self.icons.lookup(identifier, metadata)?;
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
        let mut display = if java.is_some() {
            let grip = if item.identifier.as_ref() == assets::facts::VanillaItem::Bow.identifier() {
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
        display.scale *= input.main_hand_scale();
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
