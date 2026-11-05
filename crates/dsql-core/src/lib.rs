//! Core of Arco DSQL Studio: everything that talks to AWS and Aurora DSQL, testable without Tauri.

pub mod catalog;
pub mod classify;
pub mod clusters;
pub mod conn;
pub mod error;
pub mod exec;
pub mod explain;
pub mod identity;
pub mod profiles;
pub mod quote;
pub mod rules;
pub mod split;
pub mod sso;
pub mod store;
pub mod token;

pub use error::{AwsErrorKind, CoreError};
