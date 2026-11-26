use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidArgument,
    NotFound,
    AlreadyExists,
    Conflict,
    Unauthenticated,
    AccessDenied,
    Throttled,
    NotImplemented,
    Internal,
}

impl ErrorKind {
    pub fn http_status(self) -> u16 {
        match self {
            ErrorKind::InvalidArgument => 400,
            ErrorKind::NotFound => 404,
            ErrorKind::AlreadyExists | ErrorKind::Conflict => 409,
            ErrorKind::Unauthenticated => 401,
            ErrorKind::AccessDenied => 403,
            ErrorKind::Throttled => 429,
            ErrorKind::NotImplemented => 501,
            ErrorKind::Internal => 500,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudError {
    pub kind: ErrorKind,
    pub code: String,
    pub message: String,
    pub http_status: u16,
    pub retryable: bool,
    #[serde(default)]
    pub details: Value,
}

impl CloudError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        CloudError {
            kind,
            code: String::new(),
            message: message.into(),
            http_status: kind.http_status(),
            retryable: matches!(kind, ErrorKind::Throttled | ErrorKind::Internal),
            details: Value::Null,
        }
    }
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }
    pub fn with_status(mut self, status: u16) -> Self {
        self.http_status = status;
        self
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, msg)
    }
    pub fn already_exists(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::AlreadyExists, msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, msg)
    }
    pub fn access_denied(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::AccessDenied, msg)
    }
    pub fn throttled(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Throttled, msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, msg)
    }
    pub fn not_implemented(service: &str, op: &str) -> Self {
        Self::new(
            ErrorKind::NotImplemented,
            format!("MCLE: {service}.{op} is not implemented (fidelity L0). See STATUS.md."),
        )
    }
}

impl fmt::Display for CloudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}({}): {}", self.kind, self.code, self.message)
    }
}

impl std::error::Error for CloudError {}
