//! Original, service-free paper-doll inputs for the offline snapshot harness.
use super::*;
use render_model::ActorVertex;

/// Projects an original emitting cosmetic through the production player-preview path.
/// Returned pages retain their logical indices; no installed game assets are required.
pub fn cosmetic_particle_preview(
    view: player_preview::PreviewView,
    seconds: f64,
    emitting: bool,
) -> (Arc<UiMesh>, Vec<(u16, UiTexturePage)>) {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    runtime.set_gui_particle_assets(Some(particle_fixture_assets()));
    let skin: Vec<u8> = (0..64 * 64)
        .flat_map(|i| {
            if i / 64 < 16 {
                [210, 165, 116, 255]
            } else {
                [38, 126, 174, 255]
            }
        })
        .collect();
    runtime.gui_models.skin = Some(UiTexturePage::owned([64; 2], skin.into()).unwrap());
    let item = render_model::hats::fixture::sparkling_crown();
    let model = render_model::hats::rig(item);
    runtime.set_preview_cosmetics(&[item]);
    runtime.player_preview_view = view;
    let texture = &model.texture;
    let icon = IconRef {
        page: 1,
        uv: [0, 0, texture.width as u16, texture.height as u16],
        glint: false,
    };
    runtime.menu_artwork.refs.insert(item.texture_key(), icon);
    if view == player_preview::PreviewView::Hud {
        runtime.gui_models.live_player.hat_vertices.insert(
            item,
            model
                .rest_vertices
                .iter()
                .map(|vertex| ActorVertex {
                    part: render_model::STANDARD_BIPED_BONES.len() as u32,
                    ..*vertex
                })
                .collect(),
        );
        runtime.gui_models.live_player.emitters.push((
            item,
            0,
            model
                .preview_emitter_transform(0, model.preview_motion(0.0))
                .unwrap(),
        ));
    }
    let frames = (seconds * 60.0).ceil() as u32;
    for frame in 0..=frames {
        runtime.menu_seconds = (f64::from(frame) / 60.0).min(seconds);
        runtime.sync_cosmetic_previews(true);
        runtime.sync_player_particles(emitting);
    }
    let view = runtime.player_preview_view;
    let mesh = runtime
        .project_gui_player_mesh(
            &runtime.gui_player_sources(view).unwrap(),
            [None, None],
            view,
        )
        .unwrap();
    let first = runtime.textures.dynamic_start();
    let mut pages = vec![
        ((first + SKIN_PAGE) as u16, runtime.gui_models.skin.unwrap()),
        (
            icon.page,
            UiTexturePage::owned([texture.width, texture.height], Arc::clone(&texture.rgba8))
                .unwrap(),
        ),
    ];
    if let Some(page) = runtime.gui_models.particles.page {
        pages.push(((first + render_model::UI_PARTICLE_PAGE_OFFSET) as u16, page));
    }
    (mesh, pages)
}

/// Decodes a small original amber particle texture through normal carrier admission.
pub(in crate::ui_runtime::presentation::gui_models) fn particle_fixture_assets()
-> Arc<assets::RuntimeParticleAssets> {
    let effect: serde_json::Value =
        serde_json::from_str(render_model::hats::fixture::EMITTER_EFFECT).unwrap();
    let path = effect["particle_effect"]["description"]["basic_render_parameters"]["texture"]
        .as_str()
        .unwrap();
    let texture = assets::ParticleTexture {
        path: path.into(),
        width: 16,
        height: 16,
        rgba8: [255, 80, 20, 255].repeat(16 * 16).into(),
    };
    Arc::new(
        assets::RuntimeParticleAssets::decode(
            &assets::encode_particle_catalog([1; 32], &[texture], &[]).unwrap(),
        )
        .unwrap(),
    )
}
