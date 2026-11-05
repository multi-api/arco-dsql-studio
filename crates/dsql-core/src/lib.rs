//! Core of Arco DSQL Studio: everything that talks to AWS and Aurora DSQL, testable without Tauri.

pub mod catalog;
pub mod clusters;
pub mod conn;
pub mod error;
pub mod identity;
pub mod profiles;
pub mod quote;
pub mod sso;
pub mod token;

pub use error::{AwsErrorKind, CoreError};
