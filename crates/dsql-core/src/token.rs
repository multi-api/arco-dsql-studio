//! IAM authentication tokens for Aurora DSQL. Generated locally by the SDK's signer; no network call.

use aws_config::{Region, SdkConfig};
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::identity::classify_sdk_error;

/// Token lifetime in seconds. A token only matters while a connection is being opened.
pub const TOKEN_EXPIRES_IN: u64 = 900;

/// The Postgres role a connection signs in as. `admin` needs the admin token (`dsql:DbConnectAdmin`); every other
/// role uses the normal token (`dsql:DbConnect`) and an `AWS IAM GRANT` mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbRole {
    Admin,
    Custom(String),
}

impl DbRole {
    pub fn user(&self) -> &str {
        match self {
            DbRole::Admin => "admin",
            DbRole::Custom(name) => name,
        }
    }
}

/// A fresh token for one new connection: the admin variant for `admin`, the normal one for any other role.
pub async fn auth_token(
    cfg: &SdkConfig,
    host: &str,
    region: &str,
    role: &DbRole,
) -> Result<String, CoreError> {
    let token_error = |e: &dyn std::fmt::Display| {
        let message = format!("could not create a DSQL auth token: {e}");
        CoreError::Aws {
            kind: classify_sdk_error(&message),
            message,
        }
    };
    let config = Config::builder()
        .hostname(host)
        .region(Region::new(region.to_string()))
        .expires_in(TOKEN_EXPIRES_IN)
        .build()
        .map_err(|e| token_error(&e))?;
    let generator = AuthTokenGenerator::new(config);
    let token = match role {
        DbRole::Admin => generator.db_connect_admin_auth_token(cfg).await,
        DbRole::Custom(_) => generator.db_connect_auth_token(cfg).await,
    }
    .map_err(|e| token_error(&e))?;
    Ok(token.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_role_user() {
        assert_eq!(DbRole::Admin.user(), "admin");
        assert_eq!(DbRole::Custom("readonly".into()).user(), "readonly");
    }
}
