//! Reports the entity catalog's arenas and nested geometry without copying or retaining them.

use crate::compiled::{BufferVisitor, RetainedBuffers, visit_slice};

use super::*;

impl RetainedBuffers for RuntimeEntityAssets {
    /// Reports shared arenas by address so cloned catalogs count their storage once.
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            item_use_profiles,
            seat_layouts,
            interaction_traits,
            carrier_identity: _,
            source_manifest_sha256: _,
            block_visual_count: _,
            sources,
            symbols,
            geometries,
            geometry_parents,
            locator_catalog,
            animation_clips,
            animation_channels,
            animation_keyframes,
            molang_symbols,
            molang_query_bindings,
            molang_expressions,
            molang_ops,
            molang_collections,
            molang_collection_items,
            controllers,
            controller_states,
            controller_animations,
            controller_transitions,
            rig_bindings,
            attachable_rigs,
            rig_geometries,
            rig_animations,
            rig_controllers,
            item_visuals,
            item_visual_aliases,
            render,
            multitexture_rigs,
        } = self;
        visit_slice(item_use_profiles, visit);
        visit_slice(seat_layouts, visit);
        visit_slice(interaction_traits, visit);
        visit_slice(sources, visit);
        visit_slice(symbols, visit);
        visit_slice(geometries, visit);
        visit_slice(geometry_parents, visit);
        visit_slice(locator_catalog, visit);
        visit_slice(animation_clips, visit);
        visit_slice(animation_channels, visit);
        visit_slice(animation_keyframes, visit);
        visit_slice(molang_symbols, visit);
        visit_slice(molang_query_bindings, visit);
        visit_slice(molang_expressions, visit);
        visit_slice(molang_ops, visit);
        visit_slice(molang_collections, visit);
        visit_slice(molang_collection_items, visit);
        visit_slice(controllers, visit);
        visit_slice(controller_states, visit);
        visit_slice(controller_animations, visit);
        visit_slice(controller_transitions, visit);
        visit_slice(rig_bindings, visit);
        visit_slice(attachable_rigs, visit);
        visit_slice(rig_geometries, visit);
        visit_slice(rig_animations, visit);
        visit_slice(rig_controllers, visit);
        visit_slice(item_visuals, visit);
        visit_slice(item_visual_aliases, visit);
        visit_slice(multitexture_rigs, visit);
        for source in sources.iter() {
            visit_slice(source.path.as_bytes(), visit);
        }
        for symbol in symbols.iter() {
            visit_slice(symbol.identifier.as_bytes(), visit);
            visit_slice(&symbol.dependencies, visit);
            for dependency in &symbol.dependencies {
                visit_slice(dependency.identifier.as_bytes(), visit);
            }
        }
        for geometry in geometries.iter() {
            geometry.visit_buffers(visit);
        }
        for catalog in locator_catalog.iter() {
            for (name, binding) in catalog {
                visit_slice(name.as_bytes(), visit);
                visit_slice(std::slice::from_ref(binding), visit);
                visit_slice(binding.locator.name.as_bytes(), visit);
            }
        }
        for channel in animation_channels.iter() {
            if let Some(name) = &channel.bone_name {
                visit_slice(name.as_bytes(), visit);
            }
        }
        for symbol in molang_symbols.iter() {
            visit_slice(symbol.identifier.as_bytes(), visit);
        }
        for item in item_visuals.iter() {
            visit_slice(item.key.identifier.as_bytes(), visit);
        }
        for alias in item_visual_aliases.iter() {
            visit_slice(alias.key.identifier.as_bytes(), visit);
        }
        for descriptor in item_use_profiles.iter() {
            visit_slice(descriptor.identifier.as_bytes(), visit);
            if let item_use::ItemUseProfile::Provisional {
                cooldown,
                duration_alias,
                ..
            } = &descriptor.profile
            {
                if let Some(cooldown) = cooldown {
                    visit_slice(cooldown.category.as_bytes(), visit);
                }
                if let Some(alias) = duration_alias {
                    visit_slice(alias.as_bytes(), visit);
                }
            }
        }
        for descriptor in seat_layouts.iter() {
            visit_slice(descriptor.identifier.as_bytes(), visit);
            if let seats::EntitySeatAdmission::Provisional { layouts, .. } = &descriptor.admission {
                visit_slice(layouts, visit);
                for layout in layouts {
                    visit_slice(&layout.seats, visit);
                }
            }
        }
        for descriptor in interaction_traits.iter() {
            visit_slice(descriptor.identifier.as_bytes(), visit);
        }
        render.visit_buffers(visit);
    }
}

impl RetainedBuffers for EntityGeometry {
    /// Includes each bone's cube, locator and texture-mesh storage beside its enclosing slice.
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            identifier,
            inherits,
            source_index: _,
            texture_width: _,
            texture_height: _,
            bones,
            animation_no_head_bob: _,
            visible_bounds: _,
        } = self;
        visit_slice(identifier.as_bytes(), visit);
        if let Some(parent) = inherits {
            visit_slice(parent.identifier.as_bytes(), visit);
        }
        visit_slice(bones, visit);
        for bone in bones {
            let EntityGeometryBone {
                locators,
                name,
                parent,
                pivot: _,
                rotation: _,
                bind_pose_rotation: _,
                mirror: _,
                inflate: _,
                never_render: _,
                reset: _,
                binding,
                texture_meshes,
                cubes,
            } = bone;
            visit_slice(name.as_bytes(), visit);
            for text in [parent, binding].into_iter().flatten() {
                visit_slice(text.as_bytes(), visit);
            }
            visit_slice(locators, visit);
            for locator in locators {
                visit_slice(locator.name.as_bytes(), visit);
            }
            visit_slice(texture_meshes, visit);
            for mesh in texture_meshes {
                visit_slice(mesh.texture.as_bytes(), visit);
            }
            visit_slice(cubes, visit);
        }
    }
}

impl RetainedBuffers for EntityRenderData {
    /// Includes controller tables and their separately allocated visibility patterns.
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            layers,
            slots,
            candidates,
            visibility,
            geometries,
        } = self;
        visit_slice(layers, visit);
        visit_slice(slots, visit);
        visit_slice(candidates, visit);
        visit_slice(visibility, visit);
        visit_slice(geometries, visit);
        for rule in visibility {
            visit_slice(rule.pattern.as_bytes(), visit);
        }
    }
}
