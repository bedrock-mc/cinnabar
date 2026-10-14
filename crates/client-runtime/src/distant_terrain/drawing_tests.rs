//! Drawing follows the Video setting: an open store meshes for the renderer, selection
//! starts from nothing drawn and starts again when the renderer drops what it drew.

use render_api::{DistantTerrainQuality, DistantTerrainTier};
use render_scene::distant_terrain::{
    DistantTerrainLink, DistantTerrainRenderSettings, DistantTerrainRenderStats,
};

use super::{
    drawing::{DistantTerrainDrawing, DrawCamera, DrawFrame},
    tests::{open_writer, stream, target, update, update_until},
    *,
};

const SETTINGS: DistantTerrainRenderSettings = DistantTerrainRenderSettings {
    quality: DistantTerrainQuality::Medium,
    distance_chunks: 64,
};

fn draw(
    drawing: &mut DistantTerrainDrawing,
    stream: &mut WorldStream,
    settings: Option<DistantTerrainRenderSettings>,
) {
    drawing.update(DrawFrame {
        stream: Some(stream),
        settings,
        camera: Some(DrawCamera {
            eye: [8.0, 70.0, 8.0],
            viewport_height_px: 720.0,
            vertical_fov_radians: 1.2,
        }),
        uptime: Duration::from_secs(1),
        frame_seconds: 1.0 / 60.0,
    });
}

#[test]
fn an_open_store_meshes_only_while_drawing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    sessions.set_drawing(true);
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    update(&mut sessions, &mut stream, Some(target(1)));
    assert!(stream.distant_terrain_mesh_stats().is_some());
    sessions.set_drawing(false);
    update(&mut sessions, &mut stream, Some(target(1)));
    assert!(stream.distant_terrain_mesh_stats().is_none());
    sessions.set_drawing(true);
    update(&mut sessions, &mut stream, Some(target(1)));
    assert!(stream.distant_terrain_mesh_stats().is_some());
    update(&mut sessions, &mut stream, None);
    assert!(
        stream.distant_terrain_mesh_stats().is_none(),
        "closing the store stops meshing"
    );
}

#[test]
fn the_store_cap_stays_within_the_total_cap() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    sessions.set_store_cap(256 << 20);
    assert_eq!(sessions.quotas().store_bytes, 256 << 20);
    sessions.set_store_cap(u64::MAX);
    assert_eq!(
        sessions.quotas().store_bytes,
        sessions.quotas().global_bytes
    );
}

#[test]
fn selection_starts_from_nothing_drawn_and_restarts_after_a_renderer_reset() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    let link = DistantTerrainLink::default();
    let mut drawing = DistantTerrainDrawing::new(link.clone());
    assert!(drawing.idle());
    draw(&mut drawing, &mut stream, Some(SETTINGS));
    assert!(!drawing.drawing(), "no mesher before the store opens");
    assert!(!drawing.idle(), "the renderer builds while the store opens");
    sessions.set_drawing(true);
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    update(&mut sessions, &mut stream, Some(target(1)));
    draw(&mut drawing, &mut stream, Some(SETTINGS));
    assert!(drawing.drawing());
    assert!(stream.distant_terrain_selection_stats().is_some());
    let inbox = link.take();
    assert_eq!(inbox.settings, Some(SETTINGS));
    assert_eq!(
        inbox.generation, 1,
        "the renderer starts from nothing drawn"
    );
    assert!(inbox.resident.is_some(), "the resident columns went along");

    draw(&mut drawing, &mut stream, Some(SETTINGS));
    assert_eq!(link.take().generation, 1, "a steady frame resets nothing");

    // Fog follows only terrain the renderer draws, to the distance its device tier allows.
    assert_eq!(drawing.drawn_distance_chunks(), None, "nothing drawn yet");
    link.publish(DistantTerrainRenderStats {
        ready: true,
        sections: 1,
        ..Default::default()
    });
    assert_eq!(
        drawing.drawn_distance_chunks(),
        Some(SETTINGS.distance_chunks)
    );
    let far = DistantTerrainRenderSettings {
        distance_chunks: u32::MAX,
        ..SETTINGS
    };
    link.publish(DistantTerrainRenderStats {
        ready: true,
        sections: 1,
        tier: Some(DistantTerrainTier::LOW_END),
        ..Default::default()
    });
    draw(&mut drawing, &mut stream, Some(far));
    assert_eq!(
        drawing.drawn_distance_chunks(),
        Some(DistantTerrainTier::LOW_END.max_distance_chunks)
    );
    draw(&mut drawing, &mut stream, Some(SETTINGS));
    let _ = link.take();

    link.publish(DistantTerrainRenderStats {
        resets_requested: 1,
        ..Default::default()
    });
    draw(&mut drawing, &mut stream, Some(SETTINGS));
    assert_eq!(
        link.take().generation,
        2,
        "selection restarted for the new pool"
    );

    assert!(!drawing.idle(), "the renderer still holds its settings");
    draw(&mut drawing, &mut stream, None);
    assert!(drawing.idle(), "an off frame leaves nothing to do");
    assert!(!drawing.drawing());
    assert!(stream.distant_terrain_selection_stats().is_none());
    assert_eq!(link.take().settings, None);

    link.publish(DistantTerrainRenderStats {
        supported: Some(false),
        resets_requested: 1,
        ..Default::default()
    });
    draw(&mut drawing, &mut stream, Some(SETTINGS));
    assert!(drawing.unsupported());
    assert!(
        !drawing.drawing(),
        "a device that cannot draw never selects"
    );
}
