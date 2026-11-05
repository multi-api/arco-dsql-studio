//! IAM Identity Center (SSO) sign-in with the device authorization flow, and the AWS CLI's token cache.
//!
//! The token is written to `~/.aws/sso/cache/<key>.json` in the CLI's own format, so the app and the AWS CLI share
//! one session. Nothing here is ever sent to the UI: `DeviceLogin` and `SsoToken` hold secrets.

use std::time::Duration;

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_sdk_ssooidc::operation::create_token::CreateTokenError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{AwsErrorKind, CoreError};
use crate::identity::aws_error;
use crate::profiles::AwsPaths;

const CLIENT_NAME: &str = "arco-dsql-studio";
const SCOPES: &str = "sso:account:access";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Lowercase hex SHA-1 of the session name for `sso-session` profiles, or of the start URL for legacy SSO profiles.
pub fn sso_cache_key(session_name: Option<&str>, start_url: &str) -> String {
    let input = session_name.unwrap_or(start_url);
    Sha1::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A device authorization in progress. Holds the client secret, so it stays in the core.
#[derive(Debug, Clone)]
pub struct DeviceLogin {
    pub verification_uri_complete: String,
    pub user_code: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
    pub client_id: String,
    pub client_secret: String,
    pub client_secret_expires_at: i64,
}

/// One account and role the signed-in SSO user can reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SsoAccountRole {
    pub account_id: String,
    pub account_name: String,
    pub role_name: String,
}

async fn anonymous_config(sso_region: &str) -> SdkConfig {
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(sso_region.to_string()))
        .no_credentials()
        .load()
        .await
}

/// Registers a public OIDC client and starts the device flow. The UI opens `verification_uri_complete` in the
/// browser and shows `user_code` so the user can check it matches.
pub async fn start_device_login(
    start_url: &str,
    sso_region: &str,
) -> Result<DeviceLogin, CoreError> {
    let oidc = aws_sdk_ssooidc::Client::new(&anonymous_config(sso_region).await);
    let client = oidc
        .register_client()
        .client_name(CLIENT_NAME)
        .client_type("public")
        .scopes(SCOPES)
        .send()
        .await
        .map_err(aws_error)?;
    let client_id = client.client_id().unwrap_or_default().to_string();
    let client_secret = client.client_secret().unwrap_or_default().to_string();

    let auth = oidc
        .start_device_authorization()
        .client_id(&client_id)
        .client_secret(&client_secret)
        .start_url(start_url)
        .send()
        .await
        .map_err(aws_error)?;
    Ok(DeviceLogin {
        verification_uri_complete: auth
            .verification_uri_complete()
            .unwrap_or_default()
            .to_string(),
        user_code: auth.user_code().unwrap_or_default().to_string(),
        device_code: auth.device_code().unwrap_or_default().to_string(),
        interval: auth.interval().max(1) as u64,
        expires_in: auth.expires_in().max(1) as u64,
        client_id,
        client_secret,
        client_secret_expires_at: client.client_secret_expires_at(),
    })
}

/// Polls `CreateToken` until the user approves in the browser, honouring `authorization_pending` and `slow_down`.
pub async fn finish_device_login(
    login: &DeviceLogin,
    start_url: &str,
    sso_region: &str,
) -> Result<SsoToken, CoreError> {
    let oidc = aws_sdk_ssooidc::Client::new(&anonymous_config(sso_region).await);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(login.expires_in);
    let mut interval = Duration::from_secs(login.interval);
    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() > deadline {
            return Err(sign_in_failed(
                "the sign-in code expired before it was approved; start again",
            ));
        }
        let result = oidc
            .create_token()
            .client_id(&login.client_id)
            .client_secret(&login.client_secret)
            .grant_type(DEVICE_GRANT)
            .device_code(&login.device_code)
            .send()
            .await;
        let out = match result {
            Ok(out) => out,
            Err(err) => match err.as_service_error() {
                Some(CreateTokenError::AuthorizationPendingException(_)) => continue,
                Some(CreateTokenError::SlowDownException(_)) => {
                    interval += Duration::from_secs(5);
                    continue;
                }
                Some(CreateTokenError::ExpiredTokenException(_)) => {
                    return Err(sign_in_failed(
                        "AWS says the sign-in code has expired; start again",
                    ));
                }
                Some(CreateTokenError::AccessDeniedException(_)) => {
                    return Err(sign_in_failed("the sign-in was denied in the browser"));
                }
                _ => return Err(aws_error(err)),
            },
        };
        return Ok(SsoToken {
            start_url: start_url.to_string(),
            region: sso_region.to_string(),
            access_token: out.access_token().unwrap_or_default().to_string(),
            expires_at: Utc::now() + chrono::Duration::seconds(out.expires_in().into()),
            client_id: Some(login.client_id.clone()),
            client_secret: Some(login.client_secret.clone()),
            registration_expires_at: DateTime::from_timestamp(login.client_secret_expires_at, 0),
            refresh_token: out.refresh_token().map(str::to_string),
        });
    }
}

fn sign_in_failed(message: &str) -> CoreError {
    CoreError::Aws {
        kind: AwsErrorKind::SsoExpired,
        message: message.to_string(),
    }
}

/// Every account the token can reach and every role in each, sorted by account name, then role.
pub async fn list_accounts_and_roles(token: &SsoToken) -> Result<Vec<SsoAccountRole>, CoreError> {
    let sso = aws_sdk_sso::Client::new(&anonymous_config(&token.region).await);
    let mut accounts = Vec::new();
    let mut next = None;
    loop {
        let page = sso
            .list_accounts()
            .access_token(&token.access_token)
            .set_next_token(next)
            .send()
            .await
            .map_err(aws_error)?;
        accounts.extend(page.account_list().iter().cloned());
        next = page.next_token().map(str::to_string);
        if next.is_none() {
            break;
        }
    }

    let mut out = Vec::new();
    for account in accounts {
        let account_id = account.account_id().unwrap_or_default().to_string();
        let account_name = account.account_name().unwrap_or_default().to_string();
        let mut next = None;
        loop {
            let page = sso
                .list_account_roles()
                .access_token(&token.access_token)
                .account_id(&account_id)
                .set_next_token(next)
                .send()
                .await
                .map_err(aws_error)?;
            for role in page.role_list() {
                out.push(SsoAccountRole {
                    account_id: account_id.clone(),
                    account_name: account_name.clone(),
                    role_name: role.role_name().unwrap_or_default().to_string(),
                });
            }
            next = page.next_token().map(str::to_string);
            if next.is_none() {
                break;
            }
        }
    }
    out.sort_by(|a, b| (&a.account_name, &a.role_name).cmp(&(&b.account_name, &b.role_name)));
    Ok(out)
}

/// The cached SSO token, with the AWS CLI's camelCase keys and `YYYY-MM-DDTHH:MM:SSZ` dates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoToken {
    pub start_url: String,
    pub region: String,
    pub access_token: String,
    #[serde(with = "cli_time")]
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "cli_time_opt"
    )]
    pub registration_expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// Writes `<sso_cache>/<key>.json` the way the AWS CLI does, replacing an older token for the same session. The file
/// is written next to its final name and renamed, so the CLI never reads half a token.
pub fn write_sso_cache(paths: &AwsPaths, key: &str, token: &SsoToken) -> Result<(), CoreError> {
    std::fs::create_dir_all(&paths.sso_cache)?;
    let path = paths.sso_cache.join(format!("{key}.json"));
    let tmp = paths.sso_cache.join(format!("{key}.json.tmp"));
    let json = serde_json::to_string(token).map_err(|e| CoreError::Io(e.to_string()))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read_sso_cache(paths: &AwsPaths, key: &str) -> Result<Option<SsoToken>, CoreError> {
    let path = paths.sso_cache.join(format!("{key}.json"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))
}

mod cli_time {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub const FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

    pub fn serialize<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(FORMAT).to_string())
    }

    /// Accepts the CLI's format and, leniently, any RFC 3339 timestamp (older tools wrote `+00:00` or `UTC`).
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(d)?;
        parse(&text).ok_or_else(|| serde::de::Error::custom(format!("bad timestamp {text:?}")))
    }

    pub fn parse(text: &str) -> Option<DateTime<Utc>> {
        if let Ok(t) = NaiveDateTime::parse_from_str(text, FORMAT) {
            return Some(t.and_utc());
        }
        let text = text
            .strip_suffix("UTC")
            .map(|t| format!("{}Z", t.trim_end()))
            .unwrap_or(text.to_string());
        DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|t| t.with_timezone(&Utc))
    }
}

mod cli_time_opt {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => super::cli_time::serialize(t, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(text) => super::cli_time::parse(&text)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("bad timestamp {text:?}"))),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/aws/sso_cache_sample.json")
    }

    fn sample_token() -> SsoToken {
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap()
    }

    #[test]
    fn sso_cache_key_session() {
        // printf my-sso | sha1sum
        assert_eq!(
            sso_cache_key(Some("my-sso"), "https://x.awsapps.com/start"),
            "0ad374308c5a4e22f723adf10145eafad7c4031c"
        );
    }

    #[test]
    fn sso_cache_key_legacy() {
        // printf https://x.awsapps.com/start | sha1sum
        assert_eq!(
            sso_cache_key(None, "https://x.awsapps.com/start"),
            "3947de8b16688092197fa616c34e01335f21d256"
        );
    }

    #[test]
    fn sso_cache_roundtrip_matches_cli_keys() {
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap();
        let ours = serde_json::to_value(sample_token()).unwrap();
        assert_eq!(ours, fixture);
        assert_eq!(ours["expiresAt"], "2026-11-05T01:05:41Z");
    }

    #[test]
    fn sso_cache_reads_lenient_timestamps() {
        let text = r#"{"startUrl":"u","region":"r","accessToken":"t","expiresAt":"2026-11-05T01:05:41UTC"}"#;
        let token: SsoToken = serde_json::from_str(text).unwrap();
        assert_eq!(token.expires_at.to_rfc3339(), "2026-11-05T01:05:41+00:00");
        assert_eq!(token.refresh_token, None);
    }

    #[test]
    fn write_then_read_sso_cache() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AwsPaths {
            config: dir.path().join("config"),
            credentials: dir.path().join("credentials"),
            sso_cache: dir.path().join("sso/cache"),
        };
        let key = sso_cache_key(Some("my-sso"), "https://x.awsapps.com/start");
        assert_eq!(read_sso_cache(&paths, &key).unwrap(), None);
        write_sso_cache(&paths, &key, &sample_token()).unwrap();
        assert!(paths.sso_cache.join(format!("{key}.json")).is_file());
        assert_eq!(read_sso_cache(&paths, &key).unwrap(), Some(sample_token()));
    }
}
