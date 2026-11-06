//! Session request parsing and validation (start-request.md, providers/text.md).

pub mod alibaba_live;
pub mod endpoint;
pub mod request;
pub mod route;
pub mod secret;
pub mod validate;

use thiserror::Error;

pub use request::StartRequest;
pub use validate::{validate, SessionPlan, TextPlan, ValidationError};

/// Request JSON larger than this many bytes is rejected before parsing.
pub const MAX_REQUEST_BYTES: usize = 65_536;

/// Outcome of `validate`/`start` request checks, in ABI order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestError {
    #[error("request is larger than the limit")]
    TooLarge,
    #[error("request does not parse as a session request")]
    Parse,
    #[error("request is not valid: {0}")]
    Invalid(ValidationError),
}

impl RequestError {
    /// ABI error code. Parse failures and validation failures are distinct codes.
    pub fn code(self) -> &'static str {
        match self {
            Self::TooLarge => "request_too_large",
            Self::Parse => "invalid_configuration",
            Self::Invalid(_) => "configuration_invalid",
        }
    }
}

/// Parses a request without validating it. Parser messages are discarded because they
/// may echo request content.
pub fn parse_request(json: &str) -> Result<StartRequest, RequestError> {
    if json.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    serde_json::from_str(json).map_err(|_| RequestError::Parse)
}

/// Parses and validates a request (`sheng_validate` semantics; no network, no system resources).
pub fn check_request(json: &str) -> Result<SessionPlan, RequestError> {
    let request = parse_request(json)?;
    validate(&request).map_err(RequestError::Invalid)
}
