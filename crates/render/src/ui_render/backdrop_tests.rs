//! Backdrop scheduling and retained-layer isolation on the validation device.

use super::*;
use bevy::{
    camera::{
        CameraMainTextureUsages, CameraOutputMode, ClearColor, ClearColorConfig, MsaaWriteback,
        NormalizedRenderTarget, RenderTarget,
    },
    core_pipeline::Core3d,
    ecs::{
        schedule::ScheduleLabel,
        system::{IntoSystem, RunSystemOnce, System},
    },
    math::{UVec2, UVec4},
    render::{
        camera::ExtractedCamera,
        texture::{OutputColorAttachment, TextureCache},
        view::{ViewTargetAttachments, prepare_view_targets},
    },
};
use render_model::{UI_BLEND_ALPHA, UiScissor, UiTextureCatalog, UiTexturePage};
use render_scene::{UiRenderSceneResource, UiRenderStatsResource};

const SIZE: [u32; 2] = [64, 32];

/// The UI renderer's validation world with one window view of [`SIZE`].
fn view_world() -> (World, Entity) {
    let mut world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>().clone();
    let output = device.create_texture(&TextureDescriptor {
        label: Some("backdrop fixture output"),
        size: Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Bgra8UnormSrgb,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target: NormalizedRenderTarget =
        RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
            .normalize(None)
            .unwrap();
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(output.create_view(&Default::default()), output.format()),
    );
    world.insert_resource(attachments);
    world.init_resource::<TextureCache>();
    world.init_resource::<ClearColor>();
    world.init_resource::<UiBackdropPipeline>();
    world.init_resource::<UiBackdropStore>();
    let size = UVec2::new(SIZE[0], SIZE[1]);
    let entity = world
        .spawn((
            CameraMainTextureUsages::default(),
            Msaa::Off,
            ExtractedCamera {
                target: Some(target),
                physical_viewport_size: Some(size),
                physical_target_size: Some(size),
                viewport: None,
                schedule: Core3d.intern(),
                order: 0,
                output_mode: CameraOutputMode::default(),
                msaa_writeback: MsaaWriteback::default(),
                clear_color: ClearColorConfig::default(),
                sorted_camera_index_for_target: 0,
                exposure: 1.0,
                hdr: false,
            },
            ExtractedView {
                retained_view_entity: bevy::render::view::RetainedViewEntity::new(
                    Entity::PLACEHOLDER.into(),
                    None,
                    0,
                ),
                clip_from_view: bevy::math::Mat4::IDENTITY,
                world_from_view: bevy::prelude::GlobalTransform::IDENTITY,
                clip_from_world: None,
                target_format: render_scene::SCENE_COLOR_FORMAT,
                viewport: UVec4::new(0, 0, SIZE[0], SIZE[1]),
                color_grading: default(),
                invert_culling: false,
            },
        ))
        .id();
    world.run_system_once(prepare_view_targets).unwrap();
    (world, entity)
}

/// Publishes one quad of `blend` at `revision`, then prepares UI resources.
fn publish(world: &mut World, revision: u64, blend: u32) {
    let vertex = |position| UiRenderVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [0.0, 1.0],
        color: [255; 4],
        style_flags: 0,
        alpha_cutoff: -1.0,
        model_light: 0.0,
        overlay_color: render_api::UiBackdropPaint {
            blur: 8.0,
            saturation: 1.0,
            brightness: 0.62,
        }
        .overlay(),
    };
    let input = UiRenderInput {
        revision,
        viewport_size: SIZE,
        safe_area: [0; 4],
        vertices: [[0.0, 0.0], [32.0, 0.0], [32.0, 32.0], [0.0, 32.0]]
            .map(vertex)
            .into(),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, SIZE[0], SIZE[1]),
            0,
            6,
            blend,
        )]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let mut scene = world.resource::<UiRenderSceneResource>().0.clone();
    scene
        .publish(input, world.resource::<UiRenderStatsResource>())
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene));
    world
        .run_system_once(resources::prepare_ui_resources)
        .unwrap();
}

#[test]
fn pyramid_halves_the_scene_and_stops_at_its_last_level_or_texel() {
    assert_eq!(pyramid_extent([1920, 1080]), ([960, 540], PYRAMID_LEVELS));
    assert_eq!(pyramid_extent([64, 16]), ([32, 8], 6));
    assert_eq!(pyramid_extent([1, 1]), ([1, 1], 1));
}

#[test]
fn only_published_backdrops_allocate_a_pyramid_and_steady_frames_allocate_nothing() {
    let (mut world, view) = view_world();
    let mut prepare = IntoSystem::into_system(prepare_ui_backdrops);
    prepare.initialize(&mut world);

    publish(&mut world, 1, UI_BLEND_ALPHA);
    prepare.run((), &mut world).unwrap();
    let store = world.resource::<UiBackdropStore>();
    assert!(!store.allocated(), "ordinary UI must not blur the scene");
    assert!(store.pipelines.is_empty());

    publish(&mut world, 2, UI_BLEND_BACKDROP);
    prepare.run((), &mut world).unwrap();
    let store = world.resource::<UiBackdropStore>();
    let pyramid = &store.views[&view];
    assert_eq!(pyramid.scene, SIZE);
    assert_eq!(pyramid.levels.len(), 6);
    assert_eq!(pyramid.chain.len(), 5);
    assert_eq!(
        pyramid.sources.len(),
        2,
        "both main textures are bound ahead"
    );
    assert!(store.pipelines.contains_key(&view));
    let first = pyramid.levels[0].id();

    // An unchanged frame reuses every resource without allocating.
    prepare.run((), &mut world).unwrap();
    let allocated = test_alloc::thread_allocations();
    prepare.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations(), allocated);
    assert_eq!(
        world.resource::<UiBackdropStore>().views[&view].levels[0].id(),
        first
    );

    publish(&mut world, 3, UI_BLEND_ALPHA);
    prepare.run((), &mut world).unwrap();
    assert!(!world.resource::<UiBackdropStore>().allocated());
}

#[test]
fn only_published_filters_allocate_an_offscreen_group() {
    let (mut world, view) = view_world();
    let mut prepare = IntoSystem::into_system(prepare_ui_backdrops);
    prepare.initialize(&mut world);
    publish(&mut world, 1, render_model::UI_BLEND_FILTER);
    prepare.run((), &mut world).unwrap();
    let store = world.resource::<UiBackdropStore>();
    assert!(store.filters.contains_key(&view));
    assert!(!store.allocated(), "a filter alone never blurs the scene");
    let allocated = test_alloc::thread_allocations();
    prepare.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations(), allocated);
    publish(&mut world, 2, UI_BLEND_ALPHA);
    prepare.run((), &mut world).unwrap();
    assert!(world.resource::<UiBackdropStore>().filters.is_empty());
}

#[test]
fn backdrops_neither_enter_nor_damage_retained_layers() {
    let (mut world, _) = view_world();
    publish(&mut world, 1, UI_BLEND_BACKDROP);
    let input = world
        .resource::<UiRenderSceneResource>()
        .input
        .clone()
        .unwrap();
    let mut changed = (*input).clone();
    changed.revision = 2;
    for vertex in Arc::make_mut(&mut changed.vertices) {
        vertex.overlay_color[0] = 20.0;
        vertex.position[0] += 4.0;
    }
    assert_eq!(
        damage::plan_segment(&input, &changed, 0..1),
        damage::UiDamage::Unchanged,
        "a moving or re-blurred backdrop leaves the UI layer intact"
    );
    let plan = segments::plan_ui_passes(
        &input
            .batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| !is_backdrop(batch))
            .map(|(index, batch)| (index, batch, ()))
            .collect::<Vec<_>>(),
        true,
    );
    assert_eq!(plan.layer_count(), 0);
}

#[test]
fn backdrops_and_filters_are_rejected_outside_screen_space_alpha_batches() {
    let (mut world, _) = view_world();
    publish(&mut world, 1, UI_BLEND_BACKDROP);
    let input = world
        .resource::<UiRenderSceneResource>()
        .input
        .clone()
        .unwrap();
    let with = |batch: UiRenderBatch| {
        let mut input = (*input).clone();
        input.batches = Arc::from([batch]);
        input.validate()
    };
    for batch in [
        input.batches[0].with_world_projection(true),
        UiRenderBatch {
            depth_write: 1,
            isolated_depth_scope: Some(4),
            ..input.batches[0]
        },
    ] {
        let alpha = UiRenderBatch {
            blend_mode: UI_BLEND_ALPHA,
            ..batch
        };
        assert!(with(alpha).is_ok(), "{alpha:?}");
        assert!(with(batch).is_err(), "{batch:?}");
    }
    for invalid in [
        UiRenderBatch {
            blend_mode: render_model::UI_BLEND_FILTER + 1,
            ..input.batches[0]
        },
        UiRenderBatch {
            blend_mode: render_model::UI_BLEND_FILTER,
            ..input.batches[0].with_world_projection(true)
        },
        input.batches[0].with_filter_group(Some(3)),
        UiRenderBatch {
            blend_mode: UI_BLEND_ALPHA,
            ..input.batches[0]
                .with_filter_group(Some(3))
                .with_world_projection(true)
        },
    ] {
        assert!(with(invalid).is_err(), "{invalid:?}");
    }
    assert!(
        with(UiRenderBatch {
            blend_mode: UI_BLEND_ALPHA,
            ..input.batches[0].with_filter_group(Some(3))
        })
        .is_ok()
    );
    assert!(input.validate().is_ok());
}

/// Parses and validates a production shader.
fn validated(source: &str) -> (naga::Module, naga::valid::ModuleInfo) {
    let module = naga::front::wgsl::parse_str(source).expect("shader parses");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("shader validates");
    (module, info)
}

/// The `(group, binding)` slots each entry point reads.
fn slots(source: &str) -> Vec<(String, Vec<(u32, u32)>)> {
    let (module, info) = validated(source);
    module
        .entry_points
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let used = module
                .global_variables
                .iter()
                .filter(|(handle, _)| !info.get_entry_point(index)[*handle].is_empty())
                .filter_map(|(_, global)| global.binding.as_ref())
                .map(|slot| (slot.group, slot.binding))
                .collect();
            (entry.name.clone(), used)
        })
        .collect()
}

#[test]
fn shaders_read_only_what_their_layouts_bind() {
    let pipeline = UiBackdropPipeline::from_world(&mut World::new());
    let bindings = |layout: &BindGroupLayoutDescriptor| -> Vec<u32> {
        layout.entries.iter().map(|entry| entry.binding).collect()
    };
    let (filter, apply) = (
        bindings(&pipeline.filter_layout),
        bindings(&pipeline.apply_layout),
    );
    for (entry, used) in slots(include_str!("../ui_backdrop_filter.wgsl")) {
        assert!(
            used.iter()
                .all(|(group, binding)| *group == 0 && filter.contains(binding)),
            "{entry} reads {used:?}"
        );
    }
    for (entry, used) in slots(include_str!("../ui_backdrop.wgsl")) {
        assert!(
            used.iter().all(|(group, binding)| match group {
                0 => *binding == 0,
                1 => apply.contains(binding),
                _ => false,
            }),
            "{entry} reads {used:?}"
        );
    }
}

#[test]
fn backdrop_vertices_read_the_shared_ui_viewport_layout() {
    let layout = |source: &str| {
        let (module, _) = validated(source);
        let (_, viewport) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("UiViewport"))
            .expect("viewport uniform");
        let naga::TypeInner::Struct { members, span } = &viewport.inner else {
            panic!("viewport must be a struct");
        };
        let members: Vec<_> = members
            .iter()
            .map(|member| (member.name.clone(), member.offset))
            .collect();
        (members, *span)
    };
    assert_eq!(
        layout(include_str!("../ui_backdrop.wgsl")),
        layout(&shader::source(include_str!("../ui.wgsl")))
    );
}

/// Admission refreshes the cached blur strength when materials change without retaining old work.
#[test]
fn backdrop_levels_follow_the_accepted_publication_and_drop_non_backdrop_work() {
    let (mut world, _) = view_world();
    publish(&mut world, 1, UI_BLEND_BACKDROP);
    assert_eq!(world.resource::<UiGpu>().backdrop_blur, 8.);
    publish(&mut world, 2, UI_BLEND_ALPHA);
    assert_eq!(world.resource::<UiGpu>().backdrop_blur, 0.);
    publish(&mut world, 3, UI_BLEND_BACKDROP);
    assert_eq!(world.resource::<UiGpu>().backdrop_blur, 8.);
}
