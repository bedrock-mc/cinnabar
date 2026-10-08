//! Bounded camera paths and their independent progress and rotation tracks.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    CameraInstructionOptionsSplineInstruction, CameraSplinePacket,
    SharedTypesv1260CameraSplineDefinition,
};

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
    wire: CameraInstructionOptionsSplineInstruction,
) -> Result<CameraSplineInstruction, WorldPacketError> {
    let kind = match wire.type_ {
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
        total_time_seconds: wire.total_time,
        kind,
        control_points: wire.curve.into_iter().map(|p| [p.x, p.y, p.z]).collect(),
        progress_key_frames: wire
            .progress_key_frames
            .into_iter()
            .map(|frame| {
                Ok(CameraSplineProgressKeyFrame {
                    progress: frame.keyframevalue,
                    time_seconds: frame.keyframetime,
                    ease_type: bounded_identifier(
                        frame.keyframeeasingfunc,
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
                        frame.keyframevalue.x,
                        frame.keyframevalue.y,
                        frame.keyframevalue.z,
                    ],
                    time_seconds: frame.keyframetime,
                    ease_type: bounded_identifier(
                        frame.keyframeeasingfunc,
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
    packet: CameraSplinePacket,
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

/// Both named curve identifiers are case-insensitive in the registry.
fn normalize_definition(
    wire: SharedTypesv1260CameraSplineDefinition,
) -> Result<CameraSpline, WorldPacketError> {
    let kind = if wire.spline_type.eq_ignore_ascii_case("catmullrom") {
        CameraSplineKind::CatmullRom
    } else if wire.spline_type.eq_ignore_ascii_case("linear") {
        CameraSplineKind::Linear
    } else {
        return Err(WorldPacketError::InvalidCameraField {
            field: "spline.kind",
        });
    };
    validate(
        CameraSpline {
            name: bounded_identifier(wire.name, "spline.name")?,
            total_time_seconds: wire.total_time,
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
                        progress: frame.progress,
                        time_seconds: frame.time,
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
                        time_seconds: frame.time,
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
