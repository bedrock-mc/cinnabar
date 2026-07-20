use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    UnsupportedOptionalExpression,
    UnreferencedDefinition,
    UnsupportedGeometryBinding,
    IncompleteAnimationReferences,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MissingRequiredReference,
    AmbiguousRequiredReference,
    MissingGeometryReference,
    AmbiguousGeometryReference,
    MissingRenderControllerReference,
    AmbiguousRenderControllerReference,
    MissingAnimationReference,
    AmbiguousAnimationReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum CompileReferenceOutcome<T> {
    Resolved(T),
    OptionalStaticFallback {
        source: u32,
        symbol: u32,
        reason: FallbackReason,
    },
    RequiredRigRejected {
        source: u32,
        symbol: u32,
        reason: RejectReason,
    },
}
