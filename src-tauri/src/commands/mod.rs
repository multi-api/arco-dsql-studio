//! The Tauri commands the frontend calls. `src/api.ts` has a typed wrapper for each, and `src/types.ts` mirrors the
//! types they return.

pub mod aws;
pub mod db;
pub mod roles;
pub mod store;

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::Value;

    use dsql_core::catalog::{ColumnInfo, IndexInfo, SchemaInfo, TableInfo, TableKind};
    use dsql_core::classify::classify;
    use dsql_core::clusters::ClusterInfo;
    use dsql_core::edits::ApplyReport;
    use dsql_core::exec::{ResultSet, ScriptResult, StatementOutcome};
    use dsql_core::explain::{Explanation, FixAction};
    use dsql_core::identity::CallerIdentity;
    use dsql_core::profiles::{AwsPaths, list_profiles};
    use dsql_core::roles::{Access, create_plan};
    use dsql_core::sso::SsoAccountRole;
    use dsql_core::store::{HistoryEntry, SavedConnection};
    use dsql_core::{AwsErrorKind, CoreError};

    use super::aws::{ClusterList, IamRole, RegionFailure, SsoLoginStarted};
    use super::db::{ConnectionHandle, TablePage};

    const SECRET_KEYS: &[&str] = &[
        "token",
        "access_token",
        "accesstoken",
        "refresh_token",
        "refreshtoken",
        "client_secret",
        "clientsecret",
        "device_code",
        "devicecode",
        "secret_access_key",
        "session_token",
        "password",
    ];

    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                for (k, v) in map {
                    out.push(k.to_ascii_lowercase());
                    keys(v, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }

    fn assert_no_secrets(name: &str, value: impl Serialize) {
        let mut found = Vec::new();
        keys(&serde_json::to_value(value).unwrap(), &mut found);
        for key in found {
            assert!(
                !SECRET_KEYS.contains(&key.as_str()),
                "{name} returns a field named {key}"
            );
        }
    }

    /// Every command's response type, filled in, serialised: no field may carry a token, secret or password.
    #[test]
    fn commands_never_return_tokens() {
        let s = || "x".to_string();
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/dsql-core/tests/fixtures/aws");
        let profiles = list_profiles(&AwsPaths {
            config: fixtures.join("config_mixed"),
            credentials: fixtures.join("credentials_mixed"),
            sso_cache: fixtures.clone(),
        })
        .unwrap();
        assert!(!profiles.is_empty());
        assert_no_secrets("list_profiles", profiles);
        assert_no_secrets(
            "sso_start_login",
            SsoLoginStarted {
                login_id: s(),
                verification_uri_complete: s(),
                user_code: s(),
                expires_in: 1,
            },
        );
        assert_no_secrets(
            "sso_list_accounts",
            vec![SsoAccountRole {
                account_id: s(),
                account_name: s(),
                role_name: s(),
            }],
        );
        assert_no_secrets(
            "caller_identity",
            CallerIdentity {
                account: s(),
                arn: s(),
                user_id: s(),
            },
        );
        assert_no_secrets(
            "list_clusters",
            ClusterList {
                clusters: vec![ClusterInfo {
                    identifier: s(),
                    region: s(),
                    status: s(),
                    endpoint: s(),
                    arn: s(),
                    tags: [(s(), s())].into(),
                }],
                failed: vec![RegionFailure {
                    region: s(),
                    error: CoreError::Aws {
                        kind: AwsErrorKind::NotAvailableInRegion,
                        message: s(),
                    },
                }],
            },
        );
        assert_no_secrets(
            "iam_role_for_profile",
            IamRole {
                arn: s(),
                guessed: true,
            },
        );
        assert_no_secrets(
            "open_connection",
            ConnectionHandle {
                id: s(),
                current_user: s(),
                schema: Some(s()),
            },
        );
        assert_no_secrets(
            "list_schemas",
            vec![SchemaInfo {
                name: s(),
                owner: s(),
            }],
        );
        assert_no_secrets(
            "list_tables",
            vec![TableInfo {
                schema: s(),
                name: s(),
                kind: TableKind::Table,
                owner: s(),
                can_select: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
            }],
        );
        assert_no_secrets(
            "table_columns",
            vec![ColumnInfo {
                name: s(),
                data_type: s(),
                nullable: true,
                default: Some(s()),
                is_pk: true,
                ordinal: 1,
            }],
        );
        assert_no_secrets(
            "table_indexes",
            vec![IndexInfo {
                name: s(),
                columns: vec![s()],
                unique: true,
            }],
        );
        let rows = ResultSet {
            columns: vec![s()],
            rows: vec![vec![Some(s()), None]],
            truncated: false,
        };
        assert_no_secrets(
            "table_page",
            TablePage {
                result: rows.clone(),
                sql: s(),
            },
        );
        assert_no_secrets("classify_sql", classify("select 1; truncate t"));
        assert_no_secrets(
            "run_sql",
            ScriptResult {
                outcomes: vec![
                    StatementOutcome::Rows(rows),
                    StatementOutcome::Command {
                        tag: s(),
                        rows_affected: 1,
                    },
                    StatementOutcome::Error(CoreError::Db {
                        sqlstate: Some(s()),
                        message: s(),
                        detail: Some(s()),
                        hint: Some(s()),
                    }),
                ],
                duration_ms: 1,
            },
        );
        assert_no_secrets(
            "apply_changes",
            ApplyReport {
                error: Some(CoreError::Cancelled),
                ..ApplyReport::default()
            },
        );
        assert_no_secrets(
            "explain_error",
            Explanation {
                title: s(),
                body: s(),
                fix: FixAction::ShowGrant {
                    table: s(),
                    owner: s(),
                    sql: s(),
                },
            },
        );
        let saved = SavedConnection {
            id: Some(1),
            name: s(),
            profile: s(),
            region: s(),
            cluster_id: s(),
            host: s(),
            role: s(),
            schema: Some(s()),
            production: true,
            read_only: true,
        };
        assert_no_secrets("list_connections", vec![saved]);
        assert_no_secrets(
            "search_history",
            vec![HistoryEntry {
                id: Some(1),
                connection_id: Some(1),
                sql: s(),
                started_at: chrono::Utc::now(),
                duration_ms: 1,
                rows: Some(1),
                error: Some(s()),
            }],
        );
        assert_no_secrets(
            "role_create_plan",
            create_plan(
                "r",
                "arn:aws:iam::111122223333:role/r",
                "s",
                &[s()],
                Access::ReadWrite,
            )
            .unwrap(),
        );
        assert_no_secrets("role_mappings", vec![(s(), s())]);
    }
}
