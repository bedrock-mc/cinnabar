use super::super::super::player_preview::PreviewView;
use super::*;

/// Prepares the source skin and one equipped emitting item without a world or artwork worker.
fn runtime() -> UiPresentationRuntime {
    let mut runtime = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    runtime.set_gui_particle_assets(Some(super::super::test_support::particle_fixture_assets()));
    runtime.gui_models.skin =
        Some(UiTexturePage::owned([64; 2], vec![255; 64 * 64 * 4].into()).unwrap());
    let item = hats::fixture::glowing_crown();
    runtime.set_preview_cosmetics(&[item]);
    runtime.menu_artwork.refs.insert(
        item.texture_key(),
        ui::IconRef {
            page: 3,
            uv: [0, 0, 64, 64],
            glint: false,
        },
    );
    runtime
}

#[test]
fn paper_dolls_draw_authored_particles_without_a_world() {
    for view in [
        PreviewView::Live { offset: [0.0; 2] },
        PreviewView::Doll {
            yaw: 30.0,
            tilt: -10.0,
        },
        PreviewView::SkinSelector {
            yaw: 30.0,
            tilt: -10.0,
            offset: [0.0; 2],
        },
    ] {
        let mut runtime = runtime();
        runtime.player_preview_view = view;
        let before = runtime.gui_player_mesh().unwrap();
        runtime.sync_player_particles(true);
        for frame in 1..=4 {
            runtime.menu_seconds = f64::from(frame) / 8.0;
            runtime.sync_player_particles(true);
        }
        let mesh = runtime.gui_player_mesh().unwrap();
        assert!(
            mesh.vertices().len() > before.vertices().len(),
            "missing particles in {view:?}"
        );
        let page =
            (runtime.textures.dynamic_start() + render_model::UI_PARTICLE_PAGE_OFFSET) as u16;
        let particles = mesh
            .batches()
            .iter()
            .find(|batch| batch.texture_page == page)
            .unwrap();
        assert!(particles.depth_test && particles.depth_write);
        assert!(mesh.vertices()[particles.index_range.start as usize].alpha_test);
        let texture = runtime.gui_models.particles.page.as_ref().unwrap();
        let vertex = &mesh.vertices()[particles.index_range.start as usize];
        let uv = render_model::ui_particle_uv(vertex.uv, vertex.overlay_color);
        let offset = (uv[1] as usize * texture.dimensions()[0] as usize + uv[0] as usize) * 4;
        assert_eq!(&texture.pixels()[offset..offset + 4], &[255, 80, 20, 255]);
        let texture = texture.clone();
        runtime.menu_seconds += 0.125;
        runtime.sync_player_particles(true);
        assert_eq!(
            texture.identity(),
            runtime
                .gui_models
                .particles
                .page
                .as_ref()
                .unwrap()
                .identity(),
            "animation must retain texture pages"
        );
        runtime.set_preview_cosmetics(&[]);
        runtime.sync_player_particles(true);
        assert!(runtime.gui_models.particles.mesh.is_none());
        assert_eq!(
            runtime
                .gui_models
                .particles
                .system
                .as_ref()
                .unwrap()
                .live_particles(),
            0
        );
    }
}

#[test]
fn hidden_previews_drop_effects_and_resume_without_hidden_time() {
    let mut runtime = runtime();
    runtime.sync_player_particles(true);
    runtime.menu_seconds = 0.25;
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_some());
    runtime.sync_visible_cosmetic_previews(&[]);
    assert!(runtime.gui_models.particles.mesh.is_none());
    runtime.menu_seconds = 1000.0;
    runtime.sync_player_particles(true);
    assert!(
        runtime.gui_models.particles.mesh.is_none(),
        "hidden time does not emit a burst"
    );
    runtime.menu_seconds += 0.25;
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_some());
}

#[test]
fn hud_particles_follow_the_captured_actor_attachment() {
    let mut runtime = runtime();
    let item = hats::fixture::glowing_crown();
    runtime.player_preview_view = PreviewView::Hud;
    let mut transform = hats::rig(item)
        .preview_emitter_transform(0, hats::Motion::default())
        .unwrap();
    runtime
        .gui_models
        .live_player
        .emitters
        .push((item, 0, transform));
    runtime.sync_player_particles(true);
    runtime.menu_seconds = 0.25;
    runtime.sync_player_particles(true);
    let first = runtime
        .gui_models
        .particles
        .mesh
        .as_ref()
        .unwrap()
        .vertices()[0]
        .position;
    // Newly emitted particles follow a changed actor pose; existing world-space particles drain.
    transform.position[0] += 0.5;
    runtime.gui_models.live_player.emitters[0].2 = transform;
    runtime.menu_seconds = 0.5;
    runtime.sync_player_particles(true);
    let mesh = runtime.gui_models.particles.mesh.as_ref().unwrap();
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| (vertex.position[0] - first[0]).abs() > 0.1)
    );
    runtime.gui_models.live_player.emitters.clear();
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_none());
}

#[test]
fn additive_and_translucent_particles_keep_their_materials_and_uvs() {
    let instance = particles::ParticleInstance {
        center_light: [0.0, 1.0, 0.0, 0.0],
        axis_x: [0.1, 0.0, 0.0, 0.0],
        axis_y: [0.0, 0.1, 0.0, 0.0],
        uv: [0.25, 0.5, 0.125, 0.25],
        color: [0.5, 1.0, 0.0, 0.75],
        ..Default::default()
    };
    let draw = particles::DrawLists {
        blend: vec![instance],
        add: vec![instance],
        ..Default::default()
    };
    let mesh = particle_mesh(&draw, 3, 1024).unwrap();
    assert_eq!(mesh.vertices()[0].uv, [256.0, 768.0]);
    assert_eq!(mesh.vertices()[2].uv, [384.0, 512.0]);
    assert_eq!(mesh.vertices()[0].color, [128, 255, 0, 191]);
    assert_eq!(
        mesh.vertices()[6].style_flags,
        (render_model::UI_STYLE_ADDITIVE | render_model::UI_STYLE_PARTICLE) as u16
    );
    assert!(
        mesh.batches()
            .iter()
            .all(|batch| batch.depth_test && !batch.depth_write && batch.alpha_cutoff.is_none())
    );
}

#[test]
fn cosmetic_sprites_render_without_an_installed_particle_carrier() {
    let mut runtime = runtime();
    runtime.set_gui_particle_assets(None);
    let item = hats::fixture::sprite_crown();
    runtime.set_preview_cosmetics(&[item]);
    runtime.sync_player_particles(true);
    runtime.menu_seconds = 0.25;
    runtime.sync_player_particles(true);
    let particles = &runtime.gui_models.particles;
    let mesh = particles.mesh.as_ref().unwrap();
    let page = particles.page.as_ref().unwrap();
    let vertex = mesh.vertices()[0];
    let uv = render_model::ui_particle_uv(vertex.uv, vertex.overlay_color);
    let at = (uv[1] as usize * page.dimensions()[0] as usize + uv[0] as usize) * 4;
    let sprite = hats::rig(item).emitters()[0].sprite.as_ref().unwrap();
    let side = hats::COSMETIC_SPRITE_SIDE as usize;
    // The first triangle corner samples the lower-left source texel, including original alpha.
    assert_eq!(
        &page.pixels()[at..at + 4],
        &sprite.pixels[(side - 1) * side * 4..(side - 1) * side * 4 + 4]
    );
    let installed = particles.page.as_ref().unwrap().identity();
    runtime.menu_seconds = 0.5;
    runtime.sync_player_particles(true);
    assert_eq!(
        installed,
        runtime
            .gui_models
            .particles
            .page
            .as_ref()
            .unwrap()
            .identity()
    );
}

#[test]
fn changing_hud_actor_identity_restarts_preview_effects() {
    let mut runtime = runtime();
    let item = hats::fixture::glowing_crown();
    runtime.player_preview_view = PreviewView::Hud;
    runtime.gui_models.live_player.emitter_owner = Some((1, 2, 3));
    runtime.gui_models.live_player.emitters.push((
        item,
        0,
        hats::rig(item)
            .preview_emitter_transform(0, hats::Motion::default())
            .unwrap(),
    ));
    runtime.sync_player_particles(true);
    runtime.menu_seconds = 0.25;
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_some());
    runtime.gui_models.live_player.emitter_owner = Some((1, 2, 4));
    runtime.menu_seconds = 0.5;
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_none());
    runtime.menu_seconds = 0.75;
    runtime.sync_player_particles(true);
    assert!(runtime.gui_models.particles.mesh.is_some());
}

#[test]
fn particle_sprite_edges_survive_mesh_projection_until_fragment_sampling() {
    let instance = particles::ParticleInstance {
        center_light: [0.0, 1.0, 0.0, 0.0],
        axis_x: [0.1, 0.0, 0.0, 0.0],
        axis_y: [0.0, 0.1, 0.0, 0.0],
        uv: [0.25, 0.5, 16.0 / 1024.0, 16.0 / 1024.0],
        texture_bounds: [0.25, 0.5, 16.0 / 1024.0, 16.0 / 1024.0],
        color: [1.0; 4],
    };
    let mesh = particle_mesh(
        &particles::DrawLists {
            blend: vec![instance],
            ..Default::default()
        },
        3,
        1024,
    )
    .unwrap();
    assert_eq!(mesh.vertices()[0].uv, [256.0, 528.0]);
    assert_eq!(mesh.vertices()[2].uv, [272.0, 512.0]);
    assert_eq!(mesh.vertices()[0].overlay_color, [256.0, 512.0, 16.0, 16.0]);
}

#[test]
fn a_ground_item_has_gallery_art_but_adds_no_equipped_menu_mesh_or_particles() {
    let mut runtime = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    runtime.gui_models.skin =
        Some(UiTexturePage::owned([64; 2], vec![255; 64 * 64 * 4].into()).unwrap());
    runtime.player_preview_view = PreviewView::Doll {
        yaw: 30.0,
        tilt: -10.0,
    };
    let before = runtime.gui_player_mesh().unwrap();
    let item = hats::fixture::ground_effect();
    let gallery = hats::thumbnail::render(item, 64, 64).unwrap();
    assert!(gallery.rgba8.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
    runtime.menu_artwork.refs.insert(
        item.texture_key(),
        ui::IconRef {
            page: 3,
            uv: [0, 0, 64, 32],
            glint: false,
        },
    );
    runtime.set_preview_cosmetics(&[item]);
    runtime.sync_player_particles(true);
    let after = runtime.gui_player_mesh().unwrap();
    assert_eq!(after.vertices().len(), before.vertices().len());
    assert_eq!(
        after
            .vertices()
            .iter()
            .map(|v| v.position)
            .collect::<Vec<_>>(),
        before
            .vertices()
            .iter()
            .map(|v| v.position)
            .collect::<Vec<_>>()
    );
    assert!(runtime.gui_models.particles.mesh.is_none());
}
