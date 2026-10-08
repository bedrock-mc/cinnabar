use serde_json::json;

use super::{Frame, FrameMotion};

fn frame(position: [f32; 3], yaw: f32, pitch: f32) -> Frame {
    Frame::parse(
        &json!({
            "id": "duel", "arenaId": "arena", "mode": "nodebuff",
            "updatedAt": "2026-10-02T00:00:00Z", "roundActive": true,
            "teamWins": [0, 0],
            "players": [{
                "id": "fighter", "name": "Fighter", "position": position,
                "yaw": yaw, "pitch": pitch, "health": 20.0,
                "maxHealth": 20.0, "dead": false
            }]
        })
        .to_string(),
    )
    .expect("valid committed frame")
}

#[test]
fn position_and_orbit_center_sample_the_same_committed_movement() {
    let mut old = frame([2.0, 4.0, 6.0], 0.0, 0.0);
    let mut current = frame([6.0, 8.0, 10.0], 0.0, 0.0);
    let mut old_second = old.fighters[0].clone();
    old_second.id = "second".into();
    old_second.position = [10.0, 8.0, 6.0];
    let mut second = old_second.clone();
    second.position = [14.0, 12.0, 10.0];
    old.fighters.push(old_second);
    current.fighters.push(second);

    let motion = FrameMotion::new(&current, Some(&old), 0.5);
    assert_eq!(motion.position(&current.fighters[0]), [4.0, 6.0, 8.0]);
    assert_eq!(motion.position(&current.fighters[1]), [12.0, 10.0, 8.0]);
    assert_eq!(motion.center(), Some([8.0, 8.0, 8.0]));
}

#[test]
fn pov_turning_crosses_the_yaw_seam_without_a_full_rotation() {
    for (start, end, middle) in [(179.0, -179.0, -180.0), (359.0, 1.0, 0.0)] {
        let old = frame([0.0; 3], start, -20.0);
        let current = frame([0.0; 3], end, 20.0);
        let motion = FrameMotion::new(&current, Some(&old), 0.5);
        assert_eq!(motion.angles(&current.fighters[0]), [middle, 0.0]);
    }
}

#[test]
fn late_or_invalid_fractions_never_extrapolate_past_committed_poses() {
    let old = frame([1.0; 3], 0.0, -40.0);
    let current = frame([3.0; 3], 80.0, 40.0);
    for (fraction, position, angles) in [
        (-1.0, [1.0; 3], [0.0, -40.0]),
        (0.0, [1.0; 3], [0.0, -40.0]),
        (1.0, [3.0; 3], [80.0, 40.0]),
        (10.0, [3.0; 3], [80.0, 40.0]),
        (f32::NAN, [3.0; 3], [80.0, 40.0]),
        (f32::INFINITY, [3.0; 3], [80.0, 40.0]),
    ] {
        let motion = FrameMotion::new(&current, Some(&old), fraction);
        assert_eq!(motion.position(&current.fighters[0]), position);
        assert_eq!(motion.angles(&current.fighters[0]), angles);
    }
}

#[test]
fn first_frames_and_new_fighters_use_their_current_pose_immediately() {
    let old = frame([0.0; 3], 0.0, 0.0);
    let mut current = frame([20.0, 8.0, 30.0], 110.0, 30.0);
    let first = FrameMotion::new(&current, None, 0.0);
    assert_eq!(first.position(&current.fighters[0]), [20.0, 8.0, 30.0]);
    assert_eq!(first.angles(&current.fighters[0]), [110.0, 30.0]);
    current.fighters[0].id = "new-fighter".into();
    let new = FrameMotion::new(&current, Some(&old), 0.0);
    assert_eq!(new.position(&current.fighters[0]), [20.0, 8.0, 30.0]);
    assert_eq!(new.angles(&current.fighters[0]), [110.0, 30.0]);
}

#[test]
fn vertical_camera_endpoints_pass_through_level_instead_of_wrapping() {
    for (start, end) in [(-90.0, 90.0), (90.0, -90.0)] {
        let old = frame([0.0; 3], 0.0, start);
        let current = frame([0.0; 3], 0.0, end);
        let motion = FrameMotion::new(&current, Some(&old), 0.5);
        assert_eq!(motion.angles(&current.fighters[0]), [0.0, 0.0]);
    }
}

#[test]
fn duel_arena_and_round_boundaries_cannot_borrow_old_camera_positions() {
    let old = frame([0.0; 3], 0.0, 0.0);
    let current = frame([50.0, 8.0, 60.0], 120.0, 30.0);
    let mut boundaries = Vec::new();
    let mut changed = current.clone();
    changed.id = "another-duel".into();
    boundaries.push(changed);
    let mut changed = current.clone();
    changed.arena_id = "another-arena".into();
    boundaries.push(changed);
    let mut changed = current.clone();
    changed.round_active = false;
    boundaries.push(changed);
    let mut changed = current.clone();
    changed.team_wins = vec![1, 0];
    boundaries.push(changed);
    let mut changed = current.clone();
    changed.replay_epoch = 1;
    boundaries.push(changed);
    for current in boundaries {
        let motion = FrameMotion::new(&current, Some(&old), 0.0);
        assert!(motion.previous().is_none());
        assert_eq!(motion.position(&current.fighters[0]), [50.0, 8.0, 60.0]);
        assert_eq!(motion.angles(&current.fighters[0]), [120.0, 30.0]);
    }
}

#[test]
fn revived_fighter_does_not_sweep_from_the_eliminated_pose() {
    let mut old = frame([0.0; 3], 0.0, 0.0);
    old.fighters[0].dead = true;
    let current = frame([50.0, 8.0, 60.0], 120.0, 30.0);
    let motion = FrameMotion::new(&current, Some(&old), 0.0);
    assert_eq!(motion.position(&current.fighters[0]), [50.0, 8.0, 60.0]);
    assert_eq!(motion.angles(&current.fighters[0]), [120.0, 30.0]);
}

#[test]
fn empty_frame_has_no_orbit_target() {
    let mut current = frame([0.0; 3], 0.0, 0.0);
    current.fighters.clear();
    assert_eq!(FrameMotion::new(&current, None, 0.5).center(), None);
}
