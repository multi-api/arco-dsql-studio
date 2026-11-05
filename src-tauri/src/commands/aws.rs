//! AWS profiles, SSO sign-in, caller identity and cluster discovery.

use serde::Serialize;
use tauri::State;

use dsql_core::clusters::{self, ClusterInfo, DSQL_REGIONS};
use dsql_core::identity::{self, CallerIdentity};
use dsql_core::profiles::{self, AwsPaths, NewSsoProfile, ProfileInfo};
use dsql_core::sso::{self, SsoAccountRole};
use dsql_core::{AwsErrorKind, CoreError};

use crate::state::{AppState, PendingLogin};

/// Region used for global calls (STS, IAM) when a profile has none.
pub const FALLBACK_REGION: &str = "us-east-1";

/// What the user needs to approve a sign-in in the browser. The device code and client secret stay in the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SsoLoginStarted {
    pub login_id: String,
    pub verification_uri_complete: String,
    pub user_code: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClusterList {
    pub clusters: Vec<ClusterInfo>,
    /// Regions that failed, with why; `NotAvailableInRegion` ones are normal (opt-in regions, DSQL missing).
    pub failed: Vec<RegionFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegionFailure {
    pub region: String,
    pub error: CoreError,
}

/// The IAM role behind a profile, for the role wizard. `guessed` when `iam:GetRole` was denied and the ARN was built
/// from the SSO path pattern, so the UI asks the user to check it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IamRole {
    pub arn: String,
    pub guessed: bool,
}

#[tauri::command]
pub fn list_profiles() -> Result<Vec<ProfileInfo>, CoreError> {
    profiles::list_profiles(&AwsPaths::from_env())
}

#[tauri::command]
pub async fn sso_start_login(
    state: State<'_, AppState>,
    start_url: String,
    sso_region: String,
) -> Result<SsoLoginStarted, CoreError> {
    let device = sso::start_device_login(&start_url, &sso_region).await?;
    let started = SsoLoginStarted {
        login_id: state.new_id("login"),
        verification_uri_complete: device.verification_uri_complete.clone(),
        user_code: device.user_code.clone(),
        expires_in: device.expires_in,
    };
    state.logins.lock().expect("logins lock").insert(
        started.login_id.clone(),
        PendingLogin {
            start_url,
            sso_region,
            device,
            token: None,
        },
    );
    Ok(started)
}

/// Waits until the user approves the sign-in in the browser (or it expires or is denied).
#[tauri::command]
pub async fn sso_finish_login(
    state: State<'_, AppState>,
    login_id: String,
) -> Result<(), CoreError> {
    let (device, start_url, sso_region) = {
        let logins = state.logins.lock().expect("logins lock");
        let login = pending(&logins, &login_id)?;
        (
            login.device.clone(),
            login.start_url.clone(),
            login.sso_region.clone(),
        )
    };
    let token = sso::finish_device_login(&device, &start_url, &sso_region).await;
    let mut logins = state.logins.lock().expect("logins lock");
    match token {
        Ok(token) => {
            if let Some(login) = logins.get_mut(&login_id) {
                login.token = Some(token);
            }
            Ok(())
        }
        Err(err) => {
            logins.remove(&login_id);
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn sso_list_accounts(
    state: State<'_, AppState>,
    login_id: String,
) -> Result<Vec<SsoAccountRole>, CoreError> {
    let token = {
        let logins = state.logins.lock().expect("logins lock");
        signed_in(pending(&logins, &login_id)?)?.clone()
    };
    sso::list_accounts_and_roles(&token).await
}

/// The exact lines `append_sso_profile` will add to `~/.aws/config`, shown to the user first.
#[tauri::command]
pub fn preview_sso_profile(new_profile: NewSsoProfile) -> String {
    profiles::render_sso_profile(&new_profile)
}

/// Appends the profile to `~/.aws/config` (never rewriting what is there) and stores the sign-in's token in the AWS
/// CLI's SSO cache under the new session, so the CLI and the app both use it.
#[tauri::command]
pub fn append_sso_profile(
    state: State<'_, AppState>,
    new_profile: NewSsoProfile,
    login_id: Option<String>,
) -> Result<(), CoreError> {
    let paths = AwsPaths::from_env();
    profiles::append_profile(&paths, &profiles::render_sso_profile(&new_profile))?;
    if let Some(login_id) = login_id {
        let login = state.logins.lock().expect("logins lock").remove(&login_id);
        if let Some(token) = login.and_then(|l| l.token) {
            let key = sso::sso_cache_key(Some(&new_profile.session), &token.start_url);
            sso::write_sso_cache(&paths, &key, &token)?;
        }
    }
    Ok(())
}

/// Saves an approved sign-in for an existing SSO profile to the AWS CLI's cache, like `aws sso login --profile`:
/// `sso_session` is the profile's `sso_session`, or `None` for a legacy profile keyed by its start URL.
#[tauri::command]
pub fn sso_store_login(
    state: State<'_, AppState>,
    login_id: String,
    sso_session: Option<String>,
) -> Result<(), CoreError> {
    let login = state
        .logins
        .lock()
        .expect("logins lock")
        .remove(&login_id)
        .ok_or_else(|| {
            CoreError::Config("this sign-in is no longer pending; start again".to_string())
        })?;
    let token = login.token.ok_or_else(|| {
        CoreError::Config("the sign-in has not been approved in the browser yet".to_string())
    })?;
    let key = sso::sso_cache_key(sso_session.as_deref(), &token.start_url);
    sso::write_sso_cache(&AwsPaths::from_env(), &key, &token)
}

/// `(cluster id, region)` from an endpoint typed by hand, or `None` when it is not a DSQL endpoint.
#[tauri::command]
pub fn parse_endpoint(host: String) -> Option<(String, String)> {
    clusters::parse_endpoint(&host)
}

#[tauri::command]
pub async fn caller_identity(
    profile: String,
    region: Option<String>,
) -> Result<CallerIdentity, CoreError> {
    let cfg = identity::sdk_config(&profile, region.as_deref().unwrap_or(FALLBACK_REGION)).await?;
    identity::caller_identity(&cfg).await
}

/// Clusters in `regions`, or in every DSQL region.
#[tauri::command]
pub async fn list_clusters(
    profile: String,
    regions: Option<Vec<String>>,
) -> Result<ClusterList, CoreError> {
    let cfg = identity::sdk_config(&profile, FALLBACK_REGION).await?;
    let regions: Vec<String> =
        regions.unwrap_or_else(|| DSQL_REGIONS.iter().map(|r| r.to_string()).collect());
    let regions: Vec<&str> = regions.iter().map(String::as_str).collect();
    let (clusters, failed) = clusters::list_clusters(&cfg, &regions).await;
    Ok(ClusterList {
        clusters,
        failed: failed
            .into_iter()
            .map(|(region, error)| RegionFailure { region, error })
            .collect(),
    })
}

#[tauri::command]
pub async fn iam_role_for_profile(
    profile: String,
    region: Option<String>,
) -> Result<IamRole, CoreError> {
    let cfg = identity::sdk_config(&profile, region.as_deref().unwrap_or(FALLBACK_REGION)).await?;
    let me = identity::caller_identity(&cfg).await?;
    let role_name = identity::assumed_role_name(&me.arn).ok_or_else(|| {
        CoreError::Config(format!(
            "{} is not signed in through an IAM role; enter the role ARN to map",
            me.arn
        ))
    })?;
    match identity::iam_role_arn(&cfg, &role_name).await {
        Ok(arn) => Ok(IamRole {
            arn,
            guessed: false,
        }),
        Err(CoreError::Aws {
            kind: AwsErrorKind::AccessDenied,
            ..
        }) => {
            let sso_region = profiles::list_profiles(&AwsPaths::from_env())?
                .into_iter()
                .find(|p| p.name == profile)
                .and_then(|p| p.sso_region)
                .unwrap_or_else(|| FALLBACK_REGION.to_string());
            Ok(IamRole {
                arn: identity::sso_role_arn_guess(&me.account, &role_name, &sso_region),
                guessed: true,
            })
        }
        Err(err) => Err(err),
    }
}

fn pending<'a>(
    logins: &'a std::collections::HashMap<String, PendingLogin>,
    login_id: &str,
) -> Result<&'a PendingLogin, CoreError> {
    logins.get(login_id).ok_or_else(|| {
        CoreError::Config("this sign-in is no longer pending; start again".to_string())
    })
}

fn signed_in(login: &PendingLogin) -> Result<&sso::SsoToken, CoreError> {
    login.token.as_ref().ok_or_else(|| {
        CoreError::Config("the sign-in has not been approved in the browser yet".to_string())
    })
}
