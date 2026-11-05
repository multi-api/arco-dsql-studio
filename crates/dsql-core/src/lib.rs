//! Core of Arco DSQL Studio: everything that talks to AWS and Aurora DSQL, testable without Tauri.

pub mod error;
pub mod profiles;

pub use error::{AwsErrorKind, CoreError};
