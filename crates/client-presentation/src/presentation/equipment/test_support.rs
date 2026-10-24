//! Offline session-item frames using the production mesh, pose, and artwork routes.

use std::sync::Arc;

use assets::{RuntimeEntityAssets, RuntimeIconCatalog};
use bevy::math::Vec3;
use render_api::PoseGeneration;
use render_model::{
    ACTOR_LAYER_BODY, ActorArtworkPages, ActorRenderIdentity, ActorRigFrameBuilder,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, RenderBoneTransform,
};
use sha2::{Digest, Sha256};

use super::{
    EquipmentRuntime, FirstPersonHand, StagedSessionIcons, WornItem,
    frame_raster::{HEIGHT, Image, Target, WIDTH, draw_mesh, rows_to_mat},
};
use crate::session_assets::SessionItems;

const BACKGROUND: [u8; 4] = [120, 165, 230, 255];

/// Pixels and draw evidence from the native resting first-person item route.
pub struct FirstPersonFrame {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    /// Whether a drawable item reached the camera-space frame.
    pub item_drawn: bool,
    /// Vertices of the generated item mesh admitted by the rig frame builder.
    pub vertex_count: usize,
    /// Pixels covered by the projected item after depth and alpha testing.
    pub visible_pixels: usize,
}

/// Draws the admitted `item` from the session atlas at the production resting hand pose.
/// Missing artwork returns the same blank backdrop; no substitute item is synthesized.
pub fn render_first_person_session_item(items: &SessionItems, item: &WornItem) -> FirstPersonFrame {
    let entities = fixture_entities();
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&assets::encode_icon_catalog([1; 32], &[], &[]).unwrap())
            .unwrap(),
    );
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        entities,
        None,
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let staged = StagedSessionIcons::stage(Some(items));
    let (pages, locations) = match &staged {
        Some(icons) => pages.with_equipment_rasters(icons.rasters()),
        None => (pages, Vec::new()),
    };
    runtime.set_session_items(Some(items), staged, locations);
    let body = fixture_body(&mut runtime);
    let mut target = Target {
        color: BACKGROUND.repeat(WIDTH * HEIGHT),
        depth: vec![f32::INFINITY; WIDTH * HEIGHT],
    };
    let Some(held) = runtime.first_person_item(
        &body,
        item,
        FirstPersonHand {
            swing: 0.,
            equip: 1.,
            consume: None,
            aspect: 1.,
        },
    ) else {
        return frame(target, 0);
    };
    assert!(
        held.camera_space,
        "ordinary session items use the camera-space route"
    );
    let mut builder = ActorRigFrameBuilder::new(runtime.take_pending_geometries()).unwrap();
    let rig_frame = builder.build(1., None, [held.presentation.submission]);
    assert_eq!(
        rig_frame.instances.len(),
        1,
        "generated item mesh must be admitted"
    );
    let instance = rig_frame.instances[0];
    let span = rig_frame.geometry_spans[instance.geometry_id as usize];
    let vertices = rig_frame
        .geometry_vertices
        .span(span)
        .expect("admitted mesh vertices");
    let location = held.presentation.location;
    let page = pages
        .pages()
        .get(usize::from(location.page()).checked_sub(1).unwrap())
        .expect("session artwork page");
    let (width, height) = page.dimensions();
    let layer_bytes = usize::from(width) * usize::from(height) * 4;
    let start = location.layer() as usize * layer_bytes;
    let texture = Image {
        width: usize::from(width),
        height: usize::from(height),
        rgba8: page.pixels()[start..start + layer_bytes].to_vec(),
    };
    let bones = &rig_frame.current_bones[instance.current_bone_base as usize..];
    let placement = rows_to_mat(instance.world_from_actor);
    draw_mesh(
        &mut target,
        vertices,
        |vertex| {
            let bone = rows_to_mat(bones[vertex.bone_index as usize]);
            placement.transform_point3(bone.transform_point3(Vec3::from(vertex.position)))
        },
        &texture,
    );
    frame(target, vertices.len())
}

/// Keeps pixel coverage beside the generated vertex count, including a failed icon lookup.
fn frame(target: Target, vertex_count: usize) -> FirstPersonFrame {
    let visible_pixels = target
        .depth
        .iter()
        .filter(|depth| depth.is_finite())
        .count();
    FirstPersonFrame {
        width: WIDTH as u32,
        height: HEIGHT as u32,
        rgba8: target.color,
        item_drawn: vertex_count > 0 && visible_pixels > 0,
        vertex_count,
        visible_pixels,
    }
}

/// Supplies only validated catalog provenance; the registered skin bone owns item placement.
fn fixture_entities() -> Arc<RuntimeEntityAssets> {
    let assets = assets::CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 0,
        sources: vec![assets::EntityAssetSource {
            path: "entity/session_frame.entity.json".into(),
            source_bytes: 2,
            source_sha256: Sha256::digest(b"{}").into(),
        }]
        .into(),
        symbols: vec![assets::EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: assets::EntityAssetKind::Entity,
            identifier: "fixture:session_frame".into(),
            source_index: 0,
            dependencies: Box::new([]),
        }]
        .into(),
        item_use_profiles: Box::new([]),
        seat_layouts: Box::new([]),
        interaction_traits: Box::new([]),
        geometries: Box::new([]),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    }
    .with_unresolved_item_use()
    .with_unresolved_admissions();
    Arc::new(RuntimeEntityAssets::from_compiled(assets).unwrap())
}

/// Registers one original skin bone; ordinary held items replace it with their native pose.
fn fixture_body(runtime: &mut EquipmentRuntime) -> ActorRigSubmission {
    let rig = render_model::skin_rig_id(0);
    runtime.register_skin_rig(rig, vec!["rightItem".into()]);
    let bone = RenderBoneTransform {
        rotation: [0., 0., 0., 1.],
        translation_scale: [0., 0., 0., 1.],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let pose = Arc::<[RenderBoneTransform]>::from([bone]);
    ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 1,
                spawn_revision: std::num::NonZeroU64::MIN,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 1,
                pose_generation: PoseGeneration::new(1),
                layer: ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 1,
            reset_generation: 1,
        },
        world_from_actor: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}
