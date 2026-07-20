mod engine;

pub use engine::{
    ComparisonManifest, ComparisonReport, ComparisonRequest, Crop, EvidenceError, EvidenceKind,
    LabelledSample, LabelledSampleReport, MAX_CROP_IDENTITY_BYTES, MAX_IMAGE_DIMENSION,
    MAX_INPUT_BYTES, MAX_LABELLED_SAMPLES, MAX_SAMPLE_LABEL_BYTES, Thresholds, compare,
    compare_files,
};
