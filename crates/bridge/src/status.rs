use std::path::Path;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use crate::endpoint::EndpointKind;
use crate::{BridgeError, FramedStream};

const CONTROL_MAX_FRAME_LEN: usize = 64 * 1024;
const STATUS_REQUEST_ID: u64 = 1;
const STATUS_SCHEMA_VERSION: u32 = 1;

/// Process lifecycle reported by the local core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Starting,
    Running,
    Stopping,
}

/// Kind of upstream resource-pack offer observed by the core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackOffer {
    None,
    Optional,
    Required,
}

/// Result of acquiring the offered resource packs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackAcquisition {
    None,
    Complete,
    Failed,
    Cancelled,
}

/// Result of applying resource-pack policy to the downstream session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackDownstreamOutcome {
    None,
    StrippedOptional,
    RejectedRequired,
}

/// Resource-pack application capability of this client build.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackApplication {
    Unavailable,
}

/// Secret-safe summary of the newest resource-pack admission attempt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackAdmission {
    pub attempt_id: u64,
    pub offer: PackOffer,
    pub pack_count: u32,
    pub total_bytes: u64,
    pub acquisition: PackAcquisition,
    pub cache_loads: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_stores: u64,
    pub cache_errors: u64,
    pub downstream_outcome: PackDownstreamOutcome,
    pub application: PackApplication,
}

/// Complete Status v1 result returned by the local core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StatusV1 {
    pub schema_version: u32,
    pub lifecycle: Lifecycle,
    pub pack_admission: PackAdmission,
}

#[derive(Serialize)]
struct StatusRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusResponse {
    jsonrpc: String,
    id: u64,
    #[serde(default)]
    result: Option<StatusV1>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcError {
    code: i64,
    message: String,
}

/// Connects to the local read-only control endpoint and reads one Status v1 response.
pub async fn read_status(socket_dir: &Path) -> Result<StatusV1, BridgeError> {
    let stream = crate::endpoint::connect(socket_dir, EndpointKind::Control).await?;
    let mut framed = FramedStream::with_max(stream, CONTROL_MAX_FRAME_LEN);
    let request = serde_json::to_vec(&status_request())?;
    framed.send(Bytes::from(request)).await?;
    let response = framed.next().await.ok_or(BridgeError::ControlClosed)??;
    parse_status_response(&response)
}

fn status_request() -> StatusRequest {
    StatusRequest {
        jsonrpc: "2.0",
        id: STATUS_REQUEST_ID,
        method: "status.v1",
    }
}

fn parse_status_response(payload: &[u8]) -> Result<StatusV1, BridgeError> {
    let response: StatusResponse = serde_json::from_slice(payload)?;
    if response.jsonrpc != "2.0" {
        return invalid("jsonrpc must be exactly 2.0");
    }
    if response.id != STATUS_REQUEST_ID {
        return invalid("response id does not match the request");
    }
    match (response.result, response.error) {
        (Some(status), None) => {
            if status.schema_version != STATUS_SCHEMA_VERSION {
                return invalid("unsupported status schema version");
            }
            Ok(status)
        }
        (None, Some(error)) => Err(BridgeError::ControlRpc {
            code: error.code,
            message: error.message,
        }),
        (Some(_), Some(_)) => invalid("response contains both result and error"),
        (None, None) => invalid("response contains neither result nor error"),
    }
}

fn invalid<T>(reason: &'static str) -> Result<T, BridgeError> {
    Err(BridgeError::InvalidControlResponse { reason })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_RESULT: &str = r#"{
        "jsonrpc":"2.0",
        "id":1,
        "result":{
            "schema_version":1,
            "lifecycle":"running",
            "pack_admission":{
                "attempt_id":42,
                "offer":"required",
                "pack_count":3,
                "total_bytes":8192,
                "acquisition":"complete",
                "cache_loads":3,
                "cache_hits":1,
                "cache_misses":2,
                "cache_stores":2,
                "cache_errors":0,
                "downstream_outcome":"rejected_required",
                "application":"unavailable"
            }
        }
    }"#;

    #[test]
    fn request_is_the_exact_parameterless_status_v1_call() {
        let encoded = serde_json::to_string(&status_request()).expect("encode request");
        assert_eq!(encoded, r#"{"jsonrpc":"2.0","id":1,"method":"status.v1"}"#);
    }

    #[test]
    fn parses_complete_strict_status() {
        let status = parse_status_response(VALID_RESULT.as_bytes()).expect("valid response");
        assert_eq!(status.schema_version, 1);
        assert_eq!(status.lifecycle, Lifecycle::Running);
        assert_eq!(status.pack_admission.attempt_id, 42);
        assert_eq!(status.pack_admission.offer, PackOffer::Required);
        assert_eq!(status.pack_admission.pack_count, 3);
        assert_eq!(status.pack_admission.total_bytes, 8192);
        assert_eq!(status.pack_admission.acquisition, PackAcquisition::Complete);
        assert_eq!(status.pack_admission.cache_loads, 3);
        assert_eq!(status.pack_admission.cache_hits, 1);
        assert_eq!(status.pack_admission.cache_misses, 2);
        assert_eq!(status.pack_admission.cache_stores, 2);
        assert_eq!(status.pack_admission.cache_errors, 0);
        assert_eq!(
            status.pack_admission.downstream_outcome,
            PackDownstreamOutcome::RejectedRequired
        );
        assert_eq!(
            status.pack_admission.application,
            PackApplication::Unavailable
        );
    }

    #[test]
    fn rejects_wrong_rpc_identity_and_schema_version() {
        for (from, to) in [
            (r#""jsonrpc":"2.0""#, r#""jsonrpc":"1.0""#),
            (r#""id":1"#, r#""id":2"#),
            (r#""schema_version":1"#, r#""schema_version":2"#),
        ] {
            let payload = VALID_RESULT.replacen(from, to, 1);
            assert!(matches!(
                parse_status_response(payload.as_bytes()),
                Err(BridgeError::InvalidControlResponse { .. })
            ));
        }
    }

    #[test]
    fn rejects_unknown_fields_at_every_object_layer() {
        for (from, to) in [
            (r#""id":1,"#, r#""id":1,"extra":true,"#),
            (
                r#""schema_version":1,"#,
                r#""schema_version":1,"extra":true,"#,
            ),
            (r#""attempt_id":42,"#, r#""attempt_id":42,"extra":true,"#),
        ] {
            let payload = VALID_RESULT.replacen(from, to, 1);
            assert!(matches!(
                parse_status_response(payload.as_bytes()),
                Err(BridgeError::ControlJson(_))
            ));
        }
    }

    #[test]
    fn rejects_unknown_enums_and_unsigned_overflow() {
        for (from, to) in [
            (r#""lifecycle":"running""#, r#""lifecycle":"paused""#),
            (r#""offer":"required""#, r#""offer":"secret""#),
            (r#""pack_count":3"#, r#""pack_count":4294967296"#),
            (r#""total_bytes":8192"#, r#""total_bytes":-1"#),
            (
                r#""application":"unavailable""#,
                r#""application":"available""#,
            ),
        ] {
            let payload = VALID_RESULT.replacen(from, to, 1);
            assert!(matches!(
                parse_status_response(payload.as_bytes()),
                Err(BridgeError::ControlJson(_))
            ));
        }
    }

    #[test]
    fn validates_result_error_exclusivity_and_surfaces_rpc_error() {
        let error = parse_status_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        )
        .expect_err("RPC error must fail");
        assert!(matches!(
            error,
            BridgeError::ControlRpc { code: -32601, ref message }
                if message == "Method not found"
        ));

        let both = VALID_RESULT.replacen(
            r#""result":"#,
            r#""error":{"code":-1,"message":"bad"},"result":"#,
            1,
        );
        assert!(matches!(
            parse_status_response(both.as_bytes()),
            Err(BridgeError::InvalidControlResponse { .. })
        ));
        assert!(matches!(
            parse_status_response(br#"{"jsonrpc":"2.0","id":1}"#),
            Err(BridgeError::InvalidControlResponse { .. })
        ));
    }
}
