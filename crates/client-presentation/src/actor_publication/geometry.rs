//! Installs session artwork and batches geometry shared by body and hand publication.
use super::*;
use crate::presentation::equipment::StagedSessionIcons;

/// Rebuilds session artwork and item routes, or restores startup artwork after disconnect.
pub(super) fn apply_session_pack(
    scene: &mut ActorRenderScene,
    resources: &crate::prepared_actor_artwork::PreparedSessionResources,
    pack: Option<&assets::SessionEntityPack>,
    session_icons: SessionIconInput<'_>,
    geometry_ready: &mut SessionGeometryReady,
    equipment: Option<&mut EquipmentRuntime>,
    profiler: Option<&RuntimeStageProfiler>,
) -> (
    Option<render_model::ActorArtworkPages>,
    Option<StagedSessionIcons>,
    Vec<Option<render_model::ActorArtworkLocation>>,
) {
    let equipment_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorEquipmentSetup));
    let mut pages = resources.pages.clone();
    // A pack without attachables still owns the bodies of the actors it rigs.
    let layer = pack.map(|pack| {
        (
            Arc::clone(&pack.assets),
            pack.equipment.clone(),
            resources.locations.clone(),
        )
    });
    if let Some(equipment) = equipment {
        equipment.set_pack_layer(layer);
        equipment.set_block_entity_artwork(resources.held_block_entities.as_ref());
        equipment.set_block_items(resources.block_items.as_ref());
    }
    let mut icon_locations = Vec::new();
    let session_icons = match session_icons {
        SessionIconInput::Placed(placed) => {
            pages = placed.pages.clone();
            icon_locations = placed.locations.clone();
            Some(placed.staged.clone())
        }
        SessionIconInput::Staged(staged) => {
            if let Some(icons) = &staged {
                let (extended, locations) = pages.with_equipment_rasters(icons.rasters());
                pages = extended;
                icon_locations = locations;
            }
            staged
        }
    };
    drop(equipment_timer);
    if !geometry_ready.entities || !geometry_ready.equipment {
        apply_session_geometry(scene, resources, geometry_ready, profiler);
    }
    scene.configure_artwork(pages.clone());
    let effective = (pack.is_some()
        || session_icons.is_some()
        || resources.held_block_entities.is_some()
        || resources.block_items.is_some())
    .then_some(pages);
    (effective, session_icons, icon_locations)
}

/// Session item icons a pack worker already placed, or icons the frame stages itself.
pub(super) enum SessionIconInput<'a> {
    Placed(&'a crate::prepared_actor_artwork::PlacedSessionIcons),
    Staged(Option<StagedSessionIcons>),
}

/// Each accepted namespace can survive item/artwork refreshes independently.
#[derive(Default)]
pub(super) struct SessionGeometryReady {
    pub(super) entities: bool,
    pub(super) equipment: bool,
}

/// Retries rejected namespaces while retaining geometry that already published successfully.
fn apply_session_geometry(
    scene: &mut ActorRenderScene,
    resources: &crate::prepared_actor_artwork::PreparedSessionResources,
    ready: &mut SessionGeometryReady,
    profiler: Option<&RuntimeStageProfiler>,
) {
    let geometry_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorGeometrySetup));
    let _span = bevy::log::info_span!(
        "actor.session_geometry_commit",
        entities = resources.entities.len(),
        equipment = resources.equipment.len()
    )
    .entered();
    let (entities, equipment) = match (ready.entities, ready.equipment) {
        (false, false) => scene.replace_prepared_session_geometries(
            resources.entities.clone(),
            resources.equipment.clone(),
        ),
        (false, true) => (
            scene.replace_pack_entity_geometries(resources.entities.clone()),
            Ok(()),
        ),
        (true, false) => (
            Ok(()),
            scene.replace_pack_equipment(resources.equipment.clone()),
        ),
        (true, true) => return,
    };
    ready.entities = entities.is_ok();
    ready.equipment = equipment.is_ok();
    if let Err(error) = entities {
        bevy::log::warn!(?error, "server pack entity geometry was not applied");
    }
    if let Err(error) = equipment {
        bevy::log::warn!(?error, "server pack equipment geometry was not applied");
    }
    drop(geometry_timer);
}

/// Registers new skin models and item meshes in both rig catalogs with one rebuild each; if a
/// batch is refused, each is tried alone so a rejected mesh only leaves that model undrawn.
pub(super) fn register_geometries(
    hand: &mut ActorRigFrameBuilder,
    scene: &mut ActorRenderScene,
    geometries: Vec<render_model::ActorRigGeometry>,
    profiler: Option<&RuntimeStageProfiler>,
) {
    let _rig_build = profiler.map(|profiler| profiler.time(RuntimeStage::ActorRigBuild));
    if geometries.is_empty() {
        return;
    }
    if hand.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries.iter().cloned() {
            let _ = hand.insert_geometry(geometry);
        }
    }
    if scene.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries {
            let _ = scene.insert_geometry(geometry);
        }
    }
}

impl HandRigBuilder {
    /// Builds the first-person rig catalog from validated startup geometry.
    pub fn from_runtime_assets(assets: &assets::RuntimeEntityAssets) -> anyhow::Result<Self> {
        ActorRigFrameBuilder::from_runtime_assets(assets)
            .map(Self)
            .map_err(|error| {
                anyhow::anyhow!("prepare validated first-person hand rig geometry: {error:?}")
            })
    }
}
