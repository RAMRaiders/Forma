//! The error every command returns: a code a theme can branch on, and a message it can show.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    NotFound,
    PermissionDenied,
    Invalid,
    /// A game is already running.
    Busy,
    /// Not available on this machine yet (e.g. power control outside Batocera).
    Unsupported,
    Launch,
    /// A service could not be reached and nothing was cached.
    Network,
    /// A feature needs setting up first; the message says how.
    NotConfigured,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<forma_core::Error> for ApiError {
    fn from(e: forma_core::Error) -> Self {
        use forma_core::Error as E;
        let code = match &e {
            E::Launch(_) => ErrorCode::Launch,
            E::View(_) | E::Invalid(_) => ErrorCode::Invalid,
            E::Unsupported(_) => ErrorCode::Unsupported,
            E::Network(_) => ErrorCode::Network,
            E::NotConfigured(_) => ErrorCode::NotConfigured,
            E::Io { .. } | E::Parse { .. } | E::Db(_) => ErrorCode::Internal,
        };
        Self::new(code, e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
