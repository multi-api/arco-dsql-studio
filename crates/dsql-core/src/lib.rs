//! Core of Arco DSQL Studio: everything that talks to AWS and Aurora DSQL, testable without Tauri.

pub mod error;

pub use error::{AwsErrorKind, CoreError};
