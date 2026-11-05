//! The single error type of `dsql-core`. It is `Serialize` so Tauri commands can return it as-is, and `Deserialize`
//! so the UI can send one back to be explained.

use serde::{Deserialize, Serialize};

/// What kind of AWS failure happened, so the UI can offer the right fix (for example "Sign in again").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AwsErrorKind {
    SsoExpired,
    AccessDenied,
    NotAvailableInRegion,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CoreError {
    #[error("AWS error ({kind:?}): {message}")]
    Aws { kind: AwsErrorKind, message: String },

    #[error("database error{}: {message}", sqlstate.as_deref().map(|s| format!(" {s}")).unwrap_or_default())]
    Db {
        sqlstate: Option<String>,
        message: String,
        detail: Option<String>,
        hint: Option<String>,
    },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("cancelled")]
    Cancelled,
}

impl From<std::io::Error> for CoreError {
    fn from(err: std::io::Error) -> Self {
        CoreError::Io(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_serializes_with_kind() {
        let err = CoreError::Aws {
            kind: AwsErrorKind::SsoExpired,
            message: "Token has expired".into(),
        };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value["Aws"]["kind"], "SsoExpired");
        assert_eq!(value["Aws"]["message"], "Token has expired");
    }

    #[test]
    fn db_error_display_includes_sqlstate() {
        let err = CoreError::Db {
            sqlstate: Some("40001".into()),
            message: "conflict".into(),
            detail: None,
            hint: None,
        };
        assert_eq!(err.to_string(), "database error 40001: conflict");
    }
}
