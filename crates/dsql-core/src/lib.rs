//! Core of Arco DSQL Studio: everything that talks to AWS and Aurora DSQL, testable without Tauri.

pub mod clusters;
pub mod error;
pub mod identity;
pub mod profiles;
pub mod sso;

pub use error::{AwsErrorKind, CoreError};
