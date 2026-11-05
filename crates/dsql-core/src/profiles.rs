//! AWS profiles from `~/.aws/config` and `~/.aws/credentials`, and appending new SSO profiles.
//!
//! The app only reads these files and appends whole blocks to `config`; it never rewrites existing lines.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProfileKind {
    Sso,
    AccessKeys,
    AssumeRole,
    Process,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileInfo {
    pub name: String,
    pub kind: ProfileKind,
    pub sso_session: Option<String>,
    pub sso_start_url: Option<String>,
    pub sso_region: Option<String>,
    pub sso_account_id: Option<String>,
    pub sso_role_name: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsPaths {
    pub config: PathBuf,
    pub credentials: PathBuf,
    pub sso_cache: PathBuf,
}

impl AwsPaths {
    /// The AWS CLI's locations, honouring `AWS_CONFIG_FILE` and `AWS_SHARED_CREDENTIALS_FILE`.
    pub fn from_env() -> AwsPaths {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        AwsPaths::resolve(
            var("AWS_CONFIG_FILE"),
            var("AWS_SHARED_CREDENTIALS_FILE"),
            std::env::home_dir(),
        )
    }

    fn resolve(
        config: Option<PathBuf>,
        credentials: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> AwsPaths {
        let aws_dir = home.unwrap_or_default().join(".aws");
        AwsPaths {
            config: config.unwrap_or_else(|| aws_dir.join("config")),
            credentials: credentials.unwrap_or_else(|| aws_dir.join("credentials")),
            sso_cache: aws_dir.join("sso").join("cache"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct NewSsoProfile {
    pub profile: String,
    pub session: String,
    pub start_url: String,
    pub sso_region: String,
    pub account_id: String,
    pub role_name: String,
    pub region: String,
}

/// Profiles in `config` order, then profiles that exist only in `credentials`. Missing files mean no profiles.
pub fn list_profiles(paths: &AwsPaths) -> Result<Vec<ProfileInfo>, CoreError> {
    let config = read_optional(&paths.config)?;
    let credentials = read_optional(&paths.credentials)?;
    let config_sections = parse_ini(&config);
    let credential_sections = parse_ini(&credentials);

    let sessions: Vec<(&str, &Section)> = config_sections
        .iter()
        .filter_map(|s| {
            s.header
                .strip_prefix("sso-session ")
                .map(|name| (name.trim(), s))
        })
        .collect();

    let mut profiles: Vec<ProfileInfo> = Vec::new();
    for section in &config_sections {
        let Some(name) = config_profile_name(&section.header) else {
            continue;
        };
        let keys = credential_sections
            .iter()
            .find(|c| c.header == name)
            .is_some_and(has_access_key);
        let session = section
            .get("sso_session")
            .and_then(|s| sessions.iter().find(|(n, _)| *n == s));
        let info = ProfileInfo {
            name: name.to_string(),
            kind: profile_kind(section, keys),
            sso_session: section.get("sso_session").map(str::to_string),
            sso_start_url: session
                .and_then(|(_, s)| s.get("sso_start_url"))
                .or_else(|| section.get("sso_start_url"))
                .map(str::to_string),
            sso_region: session
                .and_then(|(_, s)| s.get("sso_region"))
                .or_else(|| section.get("sso_region"))
                .map(str::to_string),
            sso_account_id: section.get("sso_account_id").map(str::to_string),
            sso_role_name: section.get("sso_role_name").map(str::to_string),
            region: section.get("region").map(str::to_string),
        };
        // `[default]` and `[profile default]` are the same profile; the first one listed wins.
        if !profiles.iter().any(|p| p.name == info.name) {
            profiles.push(info);
        }
    }

    for section in &credential_sections {
        if profiles.iter().any(|p| p.name == section.header) {
            continue;
        }
        profiles.push(ProfileInfo {
            name: section.header.clone(),
            kind: if has_access_key(section) {
                ProfileKind::AccessKeys
            } else {
                ProfileKind::Other
            },
            sso_session: None,
            sso_start_url: None,
            sso_region: None,
            sso_account_id: None,
            sso_role_name: None,
            region: section.get("region").map(str::to_string),
        });
    }
    Ok(profiles)
}

/// The exact lines the app shows the user and then appends to `~/.aws/config`.
pub fn render_sso_profile(p: &NewSsoProfile) -> String {
    format!(
        "[profile {}]\n\
         sso_session = {}\n\
         sso_account_id = {}\n\
         sso_role_name = {}\n\
         region = {}\n\
         \n\
         [sso-session {}]\n\
         sso_start_url = {}\n\
         sso_region = {}\n\
         sso_registration_scopes = sso:account:access\n",
        p.profile,
        p.session,
        p.account_id,
        p.role_name,
        p.region,
        p.session,
        p.start_url,
        p.sso_region
    )
}

/// Appends `block` to the config file, separated by a blank line. Existing bytes are never changed. Refuses a block
/// whose `[profile x]` or `[sso-session x]` already exists, so the AWS CLI never sees two sections with one name.
pub fn append_profile(paths: &AwsPaths, block: &str) -> Result<(), CoreError> {
    let existing = read_optional(&paths.config)?;
    let existing_sections = parse_ini(&existing);
    for new in parse_ini(block) {
        let duplicate = existing_sections.iter().any(|s| {
            match (
                config_profile_name(&new.header),
                config_profile_name(&s.header),
            ) {
                (Some(a), Some(b)) => a == b,
                _ => s.header == new.header,
            }
        });
        if duplicate {
            return Err(CoreError::Config(format!(
                "{} already has a [{}] section; choose another name",
                paths.config.display(),
                new.header
            )));
        }
    }

    let mut text = String::new();
    if !existing.is_empty() {
        if !existing.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(block);

    if let Some(dir) = paths.config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.config)?;
    std::io::Write::write_all(&mut file, text.as_bytes())?;
    Ok(())
}

fn read_optional(path: &Path) -> Result<String, CoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(CoreError::Io(format!("{}: {e}", path.display()))),
    }
}

/// `[default]` or `[profile x]` in the config file; `None` for `[sso-session x]` and other sections.
fn config_profile_name(header: &str) -> Option<&str> {
    if header == "default" {
        return Some("default");
    }
    header
        .strip_prefix("profile ")
        .map(str::trim)
        .filter(|n| !n.is_empty())
}

fn profile_kind(section: &Section, keys_in_credentials: bool) -> ProfileKind {
    if ["sso_session", "sso_start_url", "sso_account_id"]
        .iter()
        .any(|k| section.get(k).is_some())
    {
        ProfileKind::Sso
    } else if section.get("role_arn").is_some() {
        ProfileKind::AssumeRole
    } else if section.get("credential_process").is_some() {
        ProfileKind::Process
    } else if keys_in_credentials || has_access_key(section) {
        ProfileKind::AccessKeys
    } else {
        ProfileKind::Other
    }
}

fn has_access_key(section: &Section) -> bool {
    section.get("aws_access_key_id").is_some()
}

struct Section {
    /// The text between the brackets, trimmed, with inner whitespace collapsed (`profile  x` → `profile x`).
    header: String,
    entries: Vec<(String, String)>,
}

impl Section {
    fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }
}

/// A small reader for the AWS shared config format: `[section]` headers, `key = value` lines, full-line `#` and `;`
/// comments. Indented lines (nested settings such as `s3 =` blocks) are skipped. Keys are lowercased.
fn parse_ini(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        if raw.starts_with([' ', '\t']) {
            continue;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(Section {
                header: header.split_whitespace().collect::<Vec<_>>().join(" "),
                entries: Vec::new(),
            });
        } else if let (Some(section), Some((key, value))) =
            (sections.last_mut(), line.split_once('='))
        {
            section
                .entries
                .push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixtures() -> AwsPaths {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/aws");
        AwsPaths {
            config: dir.join("config_mixed"),
            credentials: dir.join("credentials_mixed"),
            sso_cache: dir.join("sso/cache"),
        }
    }

    fn profile(name: &str) -> ProfileInfo {
        list_profiles(&fixtures())
            .unwrap()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("profile {name} not listed"))
    }

    fn sample_new_profile() -> NewSsoProfile {
        NewSsoProfile {
            profile: "arco-dev".into(),
            session: "arco".into(),
            start_url: "https://example.awsapps.com/start".into(),
            sso_region: "us-east-1".into(),
            account_id: "123456789012".into(),
            role_name: "Developer".into(),
            region: "us-east-1".into(),
        }
    }

    fn temp_paths(config: Option<&str>) -> (tempfile::TempDir, AwsPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AwsPaths {
            config: dir.path().join("aws/config"),
            credentials: dir.path().join("aws/credentials"),
            sso_cache: dir.path().join("aws/sso/cache"),
        };
        if let Some(text) = config {
            std::fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
            std::fs::write(&paths.config, text).unwrap();
        }
        (dir, paths)
    }

    #[test]
    fn lists_sso_session_profile() {
        let p = profile("corp-dev");
        assert_eq!(p.kind, ProfileKind::Sso);
        assert_eq!(p.sso_session.as_deref(), Some("corp"));
        assert_eq!(
            p.sso_start_url.as_deref(),
            Some("https://corp.awsapps.com/start")
        );
        assert_eq!(p.sso_region.as_deref(), Some("us-east-1"));
        assert_eq!(p.sso_account_id.as_deref(), Some("111111111111"));
        assert_eq!(p.sso_role_name.as_deref(), Some("Developer"));
        assert_eq!(p.region.as_deref(), Some("eu-west-1"));
    }

    #[test]
    fn lists_legacy_sso_profile() {
        let p = profile("legacy-sso");
        assert_eq!(p.kind, ProfileKind::Sso);
        assert_eq!(p.sso_session, None);
        assert_eq!(
            p.sso_start_url.as_deref(),
            Some("https://legacy.awsapps.com/start")
        );
        assert_eq!(p.sso_region.as_deref(), Some("us-west-2"));
        assert_eq!(p.sso_role_name.as_deref(), Some("ReadOnly"));
    }

    #[test]
    fn lists_access_key_profile_from_credentials_only() {
        assert_eq!(profile("keys-only").kind, ProfileKind::AccessKeys);
    }

    #[test]
    fn default_profile_named_default() {
        let p = profile("default");
        assert_eq!(p.kind, ProfileKind::AccessKeys);
        assert_eq!(p.region.as_deref(), Some("us-east-1"));
    }

    #[test]
    fn lists_assume_role_and_process_profiles() {
        assert_eq!(profile("assume").kind, ProfileKind::AssumeRole);
        assert_eq!(profile("proc").kind, ProfileKind::Process);
    }

    #[test]
    fn nested_settings_are_ignored() {
        let p = profile("with-nested");
        assert_eq!(p.region.as_deref(), Some("us-east-2"));
        assert_eq!(p.kind, ProfileKind::AccessKeys);
    }

    #[test]
    fn lists_each_profile_once_and_no_sso_sessions() {
        let names: Vec<String> = list_profiles(&fixtures())
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(
            names,
            [
                "default",
                "corp-dev",
                "legacy-sso",
                "assume",
                "proc",
                "with-nested",
                "keys-only"
            ]
        );
    }

    #[test]
    fn missing_files_mean_no_profiles() {
        let (_dir, paths) = temp_paths(None);
        assert_eq!(list_profiles(&paths).unwrap(), vec![]);
    }

    #[test]
    fn render_sso_profile_matches_cli_format() {
        assert_eq!(
            render_sso_profile(&sample_new_profile()),
            "[profile arco-dev]\n\
             sso_session = arco\n\
             sso_account_id = 123456789012\n\
             sso_role_name = Developer\n\
             region = us-east-1\n\
             \n\
             [sso-session arco]\n\
             sso_start_url = https://example.awsapps.com/start\n\
             sso_region = us-east-1\n\
             sso_registration_scopes = sso:account:access\n"
        );
    }

    #[test]
    fn append_never_rewrites_existing_lines() {
        let before = "# my settings\r\n[default]\r\nregion = eu-west-1";
        let (_dir, paths) = temp_paths(Some(before));
        append_profile(&paths, &render_sso_profile(&sample_new_profile())).unwrap();
        let after = std::fs::read_to_string(&paths.config).unwrap();
        assert!(
            after.starts_with(before),
            "existing bytes changed:\n{after}"
        );
        assert!(after.ends_with("sso_registration_scopes = sso:account:access\n"));
        let names: Vec<String> = list_profiles(&paths)
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["default", "arco-dev"]);
    }

    #[test]
    fn append_creates_missing_config() {
        let (_dir, paths) = temp_paths(None);
        append_profile(&paths, &render_sso_profile(&sample_new_profile())).unwrap();
        assert_eq!(
            std::fs::read_to_string(&paths.config).unwrap(),
            render_sso_profile(&sample_new_profile())
        );
    }

    #[test]
    fn append_refuses_existing_profile_name() {
        let before = "[profile arco-dev]\nregion = us-east-1\n";
        let (_dir, paths) = temp_paths(Some(before));
        let err = append_profile(&paths, &render_sso_profile(&sample_new_profile())).unwrap_err();
        assert!(matches!(err, CoreError::Config(_)), "{err:?}");
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), before);
    }

    #[test]
    fn append_refuses_existing_sso_session_name() {
        let before = "[sso-session arco]\nsso_start_url = https://other.awsapps.com/start\n";
        let (_dir, paths) = temp_paths(Some(before));
        let err = append_profile(&paths, &render_sso_profile(&sample_new_profile())).unwrap_err();
        assert!(matches!(err, CoreError::Config(_)), "{err:?}");
    }

    #[test]
    fn paths_honour_env_overrides() {
        // Only checks the override logic; it does not touch the real environment.
        let paths = AwsPaths::resolve(
            Some("C:/x/config".into()),
            Some("C:/x/creds".into()),
            Some(PathBuf::from("H")),
        );
        assert_eq!(paths.config, PathBuf::from("C:/x/config"));
        assert_eq!(paths.credentials, PathBuf::from("C:/x/creds"));
        assert_eq!(
            paths.sso_cache,
            PathBuf::from("H").join(".aws").join("sso").join("cache")
        );
        let defaults = AwsPaths::resolve(None, None, Some(PathBuf::from("H")));
        assert_eq!(
            defaults.config,
            PathBuf::from("H").join(".aws").join("config")
        );
        assert_eq!(
            defaults.credentials,
            PathBuf::from("H").join(".aws").join("credentials")
        );
    }
}
