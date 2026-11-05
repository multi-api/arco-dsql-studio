//! AWS credentials for a profile, who the caller is, and the IAM role ARN behind an SSO session.

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_sdk_sts::error::DisplayErrorContext;
use serde::Serialize;

use crate::error::{AwsErrorKind, CoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallerIdentity {
    pub account: String,
    pub arn: String,
    pub user_id: String,
}

/// Loads the profile's credentials (SSO, access keys, role chains, `credential_process`) and resolves them once, so an
/// expired SSO session fails here with `AwsErrorKind::SsoExpired` instead of on the first real call.
pub async fn sdk_config(profile: &str, region: &str) -> Result<SdkConfig, CoreError> {
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .profile_name(profile)
        .region(Region::new(region.to_string()))
        .load()
        .await;
    let provider = cfg
        .credentials_provider()
        .ok_or_else(|| CoreError::Config(format!("profile {profile} has no credentials")))?;
    aws_credential_types::provider::ProvideCredentials::provide_credentials(&provider)
        .await
        .map_err(aws_error)?;
    Ok(cfg)
}

pub async fn caller_identity(cfg: &SdkConfig) -> Result<CallerIdentity, CoreError> {
    let out = aws_sdk_sts::Client::new(cfg)
        .get_caller_identity()
        .send()
        .await
        .map_err(aws_error)?;
    Ok(CallerIdentity {
        account: out.account().unwrap_or_default().to_string(),
        arn: out.arn().unwrap_or_default().to_string(),
        user_id: out.user_id().unwrap_or_default().to_string(),
    })
}

/// The IAM role's ARN, path included (`iam:GetRole`). The caller falls back to `sso_role_arn_guess` when denied.
pub async fn iam_role_arn(cfg: &SdkConfig, role_name: &str) -> Result<String, CoreError> {
    let out = aws_sdk_iam::Client::new(cfg)
        .get_role()
        .role_name(role_name)
        .send()
        .await
        .map_err(aws_error)?;
    out.role()
        .map(|r| r.arn().to_string())
        .ok_or_else(|| CoreError::Aws {
            kind: AwsErrorKind::Other,
            message: format!("GetRole returned no role for {role_name}"),
        })
}

/// Turns any AWS SDK error into `CoreError::Aws`, keeping the whole error chain in the message.
pub(crate) fn aws_error(err: impl std::error::Error) -> CoreError {
    let message = DisplayErrorContext(&err).to_string();
    CoreError::Aws {
        kind: classify_sdk_error(&message),
        message,
    }
}

/// Sorts an AWS SDK error message into the kinds the UI can act on ("Sign in again", "missing permission", ...).
pub fn classify_sdk_error(msg: &str) -> AwsErrorKind {
    let msg = msg.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| msg.contains(n));
    if has(&[
        "sso session",
        "sso token",
        "token has expired",
        "token is expired",
        "refresh failed",
        "session token not found",
    ]) {
        AwsErrorKind::SsoExpired
    } else if has(&[
        "accessdenied",
        "access denied",
        "not authorized",
        "unauthorized",
    ]) {
        AwsErrorKind::AccessDenied
    } else if has(&[
        "dns error",
        "failed to lookup address",
        "unrecognizedclientexception",
        "not available in this region",
        "could not connect to the endpoint",
    ]) {
        AwsErrorKind::NotAvailableInRegion
    } else {
        AwsErrorKind::Other
    }
}

/// The IAM role name inside an STS assumed-role ARN: `arn:aws:sts::123:assumed-role/<role>/<session>` → `<role>`.
pub fn assumed_role_name(sts_arn: &str) -> Option<String> {
    let (prefix, resource) = sts_arn.split_once(":assumed-role/")?;
    if !prefix.starts_with("arn:") || !prefix.contains(":sts:") {
        return None;
    }
    let role = resource.split('/').next()?;
    (!role.is_empty()).then(|| role.to_string())
}

/// The IAM role ARN IAM Identity Center gives a permission set's role, for when `iam:GetRole` is not allowed.
/// Roles for an SSO instance in `us-east-1` have no region segment in their path (confirmed in Task 1).
pub fn sso_role_arn_guess(account: &str, role_name: &str, sso_region: &str) -> String {
    let region = if sso_region == "us-east-1" {
        String::new()
    } else {
        format!("{sso_region}/")
    };
    format!("arn:aws:iam::{account}:role/aws-reserved/sso.amazonaws.com/{region}{role_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_sdk_error_expired_sso() {
        for msg in [
            "The SSO session associated with this profile has expired or is otherwise invalid.",
            "failed to load credentials: Token has expired and refresh failed",
            "the SSO token is expired",
            "Error when retrieving token from sso: Token has expired and refresh failed",
            "failed to load SSO token: no token found in the cache",
            "UnauthorizedException: Session token not found or invalid",
        ] {
            assert_eq!(classify_sdk_error(msg), AwsErrorKind::SsoExpired, "{msg}");
        }
    }

    #[test]
    fn classify_access_denied() {
        for msg in [
            "AccessDeniedException: User: arn:aws:sts::1:assumed-role/x/y is not authorized to perform: dsql:ListClusters",
            "AccessDenied: User is not authorized to perform iam:GetRole",
        ] {
            assert_eq!(classify_sdk_error(msg), AwsErrorKind::AccessDenied, "{msg}");
        }
    }

    #[test]
    fn classify_not_available_in_region() {
        for msg in [
            "dispatch failure: io error: error trying to connect: dns error: failed to lookup address information",
            "UnrecognizedClientException: The security token included in the request is invalid",
            "The requested service is not available in this region",
        ] {
            assert_eq!(
                classify_sdk_error(msg),
                AwsErrorKind::NotAvailableInRegion,
                "{msg}"
            );
        }
        assert_eq!(
            classify_sdk_error("ThrottlingException: Rate exceeded"),
            AwsErrorKind::Other
        );
    }

    #[test]
    fn assumed_role_name_parses_sso_arn() {
        assert_eq!(
            assumed_role_name("arn:aws:sts::123:assumed-role/AWSReservedSSO_Admin_abc/me@x")
                .as_deref(),
            Some("AWSReservedSSO_Admin_abc")
        );
    }

    #[test]
    fn assumed_role_name_rejects_other_arns() {
        assert_eq!(assumed_role_name("arn:aws:iam::123:user/alice"), None);
        assert_eq!(assumed_role_name("arn:aws:sts::123:assumed-role/"), None);
        assert_eq!(assumed_role_name("not an arn"), None);
    }

    #[test]
    fn sso_role_arn_guess_us_east_1_has_no_region() {
        assert_eq!(
            sso_role_arn_guess("123456789012", "AWSReservedSSO_Admin_abc", "us-east-1"),
            "arn:aws:iam::123456789012:role/aws-reserved/sso.amazonaws.com/AWSReservedSSO_Admin_abc"
        );
    }

    #[test]
    fn sso_role_arn_guess_other_region_has_region() {
        assert_eq!(
            sso_role_arn_guess("123456789012", "AWSReservedSSO_Admin_abc", "eu-west-1"),
            "arn:aws:iam::123456789012:role/aws-reserved/sso.amazonaws.com/eu-west-1/AWSReservedSSO_Admin_abc"
        );
    }
}
