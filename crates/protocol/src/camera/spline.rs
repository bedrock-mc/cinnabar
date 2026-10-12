//! Bounded camera paths and their independent progress and rotation tracks.

use std::sync::Arc;

use bedrock_protocol_1_26_51::{packets, types};

use super::{CameraEvent, bounded_identifier, validate_count, validate_finite, validate_position};
use crate::WorldPacketError;

/// Limits retained path data without changing ordinary camera paths.
pub const MAX_CAMERA_SPLINE_POINTS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSplineKind {
    CatmullRom,
    Linear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraSplineProgressKeyFrame {
    pub progress: f32,
    pub time_seconds: f32,
    pub ease_type: Arc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraSplineRotationKeyFrame {
    pub rotation_degrees: [f32; 3],
    pub time_seconds: f32,
    pub ease_type: Arc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraSpline {
    pub name: Arc<str>,
    pub total_time_seconds: f32,
    pub kind: CameraSplineKind,
    pub control_points: Arc<[[f32; 3]]>,
    pub progress_key_frames: Arc<[CameraSplineProgressKeyFrame]>,
    pub rotation_key_frames: Arc<[CameraSplineRotationKeyFrame]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraSplineInstruction {
    pub spline: CameraSpline,
    pub load_from_json: bool,
}

/// Rejects unusable path data before it reaches camera evaluation.
fn validate(spline: CameraSpline, named: bool) -> Result<CameraSpline, WorldPacketError> {
    validate_finite(spline.total_time_seconds, "spline.total_time")?;
    for (field, count) in [
        ("spline.control_points", spline.control_points.len()),
        ("spline.progress", spline.progress_key_frames.len()),
        ("spline.rotation", spline.rotation_key_frames.len()),
    ] {
        validate_count(count, MAX_CAMERA_SPLINE_POINTS, field)?;
    }
    if !named && (spline.total_time_seconds <= 0.0 || spline.control_points.len() < 2) {
        return Err(WorldPacketError::InvalidCameraField {
            field: "spline.path",
        });
    }
    for &point in &*spline.control_points {
        validate_position(point, "spline.control_point")?;
    }
    for frame in &*spline.progress_key_frames {
        validate_finite(frame.progress, "spline.progress.value")?;
        validate_finite(frame.time_seconds, "spline.progress.time")?;
    }
    for frame in &*spline.rotation_key_frames {
        validate_position(frame.rotation_degrees, "spline.rotation.value")?;
        validate_finite(frame.time_seconds, "spline.rotation.time")?;
    }
    Ok(spline)
}

/// Converts inline paths and named path references without resolving registry state.
pub(super) fn normalize_instruction(
    wire: types::CameraSplineInstruction,
) -> Result<CameraSplineInstruction, WorldPacketError> {
    let kind = match wire.type_.0 {
        0 => CameraSplineKind::CatmullRom,
        1 => CameraSplineKind::Linear,
        _ => {
            return Err(WorldPacketError::InvalidCameraField {
                field: "spline.kind",
            });
        }
    };
    let spline = CameraSpline {
        name: bounded_identifier(wire.spline_identifier, "spline.name")?,
        total_time_seconds: wire.total_time.0,
        kind,
        control_points: wire.curve.into_iter().map(|p| [p.x, p.y, p.z]).collect(),
        progress_key_frames: wire
            .progress_key_frames
            .into_iter()
            .map(|frame| {
                Ok(CameraSplineProgressKeyFrame {
                    progress: frame.key_frame_value.0,
                    time_seconds: frame.key_frame_time.0,
                    ease_type: bounded_identifier(
                        frame.key_frame_easing_func,
                        "spline.progress.ease",
                    )?,
                })
            })
            .collect::<Result<Arc<[_]>, WorldPacketError>>()?,
        rotation_key_frames: wire
            .rotation_option
            .into_iter()
            .map(|frame| {
                Ok(CameraSplineRotationKeyFrame {
                    rotation_degrees: [
                        frame.key_frame_value.x,
                        frame.key_frame_value.y,
                        frame.key_frame_value.z,
                    ],
                    time_seconds: frame.key_frame_time.0,
                    ease_type: bounded_identifier(
                        frame.key_frame_easing_func,
                        "spline.rotation.ease",
                    )?,
                })
            })
            .collect::<Result<Arc<[_]>, WorldPacketError>>()?,
    };
    Ok(CameraSplineInstruction {
        spline: validate(spline, wire.load_from_json)?,
        load_from_json: wire.load_from_json,
    })
}

/// Retains the server's named paths in packet order.
pub(crate) fn normalize_registry(
    packet: packets::CameraSpline,
) -> Result<CameraEvent, WorldPacketError> {
    validate_count(
        packet.camera_data_splines.len(),
        super::MAX_CAMERA_PRESETS,
        "spline.registry",
    )?;
    packet
        .camera_data_splines
        .into_iter()
        .map(normalize_definition)
        .collect::<Result<Arc<[_]>, _>>()
        .map(CameraEvent::Splines)
}

/// The curve type is one of the two exact lowercase names the codec's pattern admits.
fn normalize_definition(
    wire: types::CameraSplineDefinition,
) -> Result<CameraSpline, WorldPacketError> {
    let kind = match wire.spline_type.as_bytes() {
        b"catmullrom" => CameraSplineKind::CatmullRom,
        b"linear" => CameraSplineKind::Linear,
        _ => {
            return Err(WorldPacketError::InvalidCameraField {
                field: "spline.kind",
            });
        }
    };
    validate(
        CameraSpline {
            name: bounded_identifier(wire.name, "spline.name")?,
            total_time_seconds: wire.total_time.0,
            kind,
            control_points: wire
                .control_points
                .into_iter()
                .map(|p| [p.position.x, p.position.y, p.position.z])
                .collect(),
            progress_key_frames: wire
                .progress_key_frames
                .into_iter()
                .map(|frame| {
                    Ok(CameraSplineProgressKeyFrame {
                        progress: frame.progress.0,
                        time_seconds: frame.time.0,
                        ease_type: bounded_identifier(
                            frame.easing.unwrap_or_default(),
                            "spline.progress.ease",
                        )?,
                    })
                })
                .collect::<Result<Arc<[_]>, WorldPacketError>>()?,
            rotation_key_frames: wire
                .rotation_key_frames
                .into_iter()
                .map(|frame| {
                    Ok(CameraSplineRotationKeyFrame {
                        rotation_degrees: [frame.rotation.x, frame.rotation.y, frame.rotation.z],
                        time_seconds: frame.time.0,
                        ease_type: bounded_identifier(
                            frame.easing.unwrap_or_default(),
                            "spline.rotation.ease",
                        )?,
                    })
                })
                .collect::<Result<Arc<[_]>, WorldPacketError>>()?,
        },
        false,
    )
}
