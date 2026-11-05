//! Plain-language explanations of errors, each with one action that fixes it (spec 6.7).

use serde::Serialize;

use crate::catalog::TableInfo;
use crate::error::{AwsErrorKind, CoreError};
use crate::quote::ident;

/// What the app knows when an error happens.
#[derive(Debug, Clone, Default)]
pub struct ExplainContext {
    /// The Postgres role the session signed in as.
    pub role: String,
    pub search_path: Option<String>,
    /// The caller's STS ARN, from `caller_identity`.
    pub caller_arn: Option<String>,
    pub is_admin: bool,
    /// Tables of every schema, for "it is in another schema" and the owner in grant snippets.
    pub known_tables: Vec<TableInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FixAction {
    SwitchSchema(String),
    ShowGrant {
        table: String,
        owner: String,
        sql: String,
    },
    SignIn,
    Retry,
    Rollback,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explanation {
    pub title: String,
    pub body: String,
    pub fix: FixAction,
}

pub fn explain(err: &CoreError, ctx: &ExplainContext) -> Explanation {
    match err {
        CoreError::Aws { kind: AwsErrorKind::SsoExpired, .. } => Explanation {
            title: "Your AWS sign-in has expired".into(),
            body: "The SSO session behind this profile is no longer valid. Sign in again; open tabs keep their work."
                .into(),
            fix: FixAction::SignIn,
        },
        CoreError::Aws { kind: AwsErrorKind::AccessDenied, message } => plain(
            "AWS refused the request",
            format!("Your IAM role is missing a permission for this.{} AWS said: {message}", caller(ctx)),
        ),
        CoreError::Aws { kind: AwsErrorKind::NotAvailableInRegion, message } => plain(
            "Aurora DSQL is not reachable in this region",
            format!("The region may not offer DSQL, or it is an opt-in region not enabled for the account. {message}"),
        ),
        CoreError::Db { sqlstate, message, hint, .. } => explain_db(sqlstate.as_deref(), message, hint.as_deref(), ctx)
            .unwrap_or_else(|| raw(err)),
        _ => raw(err),
    }
}

fn explain_db(
    sqlstate: Option<&str>,
    message: &str,
    hint: Option<&str>,
    ctx: &ExplainContext,
) -> Option<Explanation> {
    if message.starts_with("unable to accept connection") {
        return Some(auth_failed(hint, ctx));
    }
    match sqlstate? {
        "42P01" => {
            let name = quoted_name(message)?;
            let bare = name.rsplit('.').next().unwrap_or(&name);
            let looking_in = ctx.search_path.as_deref().unwrap_or("the default search_path");
            Some(match ctx.known_tables.iter().find(|t| t.name == bare) {
                Some(t) => Explanation {
                    title: format!("\"{bare}\" is in schema {}", t.schema),
                    body: format!("Your session is looking in {looking_in}; the table is in {}.", t.schema),
                    fix: FixAction::SwitchSchema(t.schema.clone()),
                },
                None => plain(
                    format!("There is no table named \"{name}\""),
                    format!("No schema you can see has a table or view called \"{name}\". Check the spelling."),
                ),
            })
        }
        "42501" => {
            if let Some(name) = message.strip_prefix("permission denied for table ") {
                let found = ctx.known_tables.iter().find(|t| t.name == name);
                let (schema, owner) = found.map_or((None, "its owner"), |t| (Some(t.schema.as_str()), t.owner.as_str()));
                let target = match schema {
                    Some(s) => format!("{}.{}", ident(s), ident(name)),
                    None => ident(name),
                };
                return Some(Explanation {
                    title: format!("Your role cannot read \"{name}\""),
                    body: format!(
                        "Table \"{name}\" belongs to {owner}. Your role {} has no privilege on it, and only the owner can \
                         grant it. Ask the owner to run the statement below, or use the team access wizard.",
                        ctx.role
                    ),
                    fix: FixAction::ShowGrant {
                        table: name.to_string(),
                        owner: owner.to_string(),
                        sql: format!("GRANT SELECT ON {target} TO {};", ident(&ctx.role)),
                    },
                });
            }
            if let Some(schema) = message.strip_prefix("permission denied for schema ") {
                return Some(Explanation {
                    title: format!("Your role cannot use schema {schema}"),
                    body: format!(
                        "Role {} has no USAGE on schema {schema}, so it cannot see the tables in it. admin (or the \
                         schema's owner) can grant it.",
                        ctx.role
                    ),
                    fix: FixAction::ShowGrant {
                        table: String::new(),
                        owner: String::new(),
                        sql: format!("GRANT USAGE ON SCHEMA {} TO {};", ident(schema), ident(&ctx.role)),
                    },
                });
            }
            None
        }
        "40001" => Some(Explanation {
            title: "Someone else changed the same rows".into(),
            body: "DSQL uses optimistic concurrency: another transaction committed a change to the same rows first, \
                   so this one was rolled back. Running it again usually works."
                .into(),
            fix: FixAction::Retry,
        }),
        "25P02" => Some(Explanation {
            title: "The transaction was aborted".into(),
            body: "An earlier statement in this transaction failed, so DSQL ignores the rest. Roll back, then try \
                   again."
                .into(),
            fix: FixAction::Rollback,
        }),
        "0A000" => Some(plain(
            "DSQL does not support this",
            format!("{message}. Aurora DSQL leaves out some PostgreSQL features; see the warning shown before running."),
        )),
        _ => None,
    }
}

fn auth_failed(hint: Option<&str>, ctx: &ExplainContext) -> Explanation {
    if hint.is_some_and(|h| h.contains("region")) {
        return plain(
            "The sign-in token is for the wrong region",
            "The token was signed for a different region than the cluster's. Check the connection's region.",
        );
    }
    let body = if ctx.is_admin {
        format!(
            "Connecting as admin needs dsql:DbConnectAdmin on this cluster's ARN, and your IAM role does not have \
             it.{}",
            caller(ctx)
        )
    } else {
        format!(
            "Connecting as {role} needs dsql:DbConnect on this cluster's ARN, and an AWS IAM GRANT {role} TO \
             '<your IAM role ARN>' mapping created by admin. One of them is missing, or the role does not exist.{}",
            caller(ctx),
            role = ctx.role
        )
    };
    plain("DSQL refused the connection", body)
}

fn caller(ctx: &ExplainContext) -> String {
    ctx.caller_arn
        .as_deref()
        .map(|arn| format!(" You are signed in as {arn}."))
        .unwrap_or_default()
}

/// The first `"..."` in a message: `relation "users" does not exist` → `users`.
fn quoted_name(message: &str) -> Option<String> {
    let start = message.find('"')? + 1;
    let len = message[start..].find('"')?;
    Some(message[start..start + len].to_string())
}

fn plain(title: impl Into<String>, body: impl Into<String>) -> Explanation {
    Explanation {
        title: title.into(),
        body: body.into(),
        fix: FixAction::None,
    }
}

/// The message as DSQL sent it, with the SQLSTATE, for errors the app has no explanation for.
fn raw(err: &CoreError) -> Explanation {
    let body = match err {
        CoreError::Db {
            sqlstate,
            message,
            detail,
            hint,
        } => {
            let mut body = format!(
                "{} {message}",
                sqlstate.as_deref().unwrap_or("(no SQLSTATE)")
            );
            for extra in [detail, hint].into_iter().flatten() {
                body.push('\n');
                body.push_str(extra);
            }
            body
        }
        other => other.to_string(),
    };
    plain("Error", body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::TableKind;

    fn db(sqlstate: &str, message: &str) -> CoreError {
        CoreError::Db {
            sqlstate: Some(sqlstate.into()),
            message: message.into(),
            detail: None,
            hint: None,
        }
    }

    fn table(schema: &str, name: &str, owner: &str) -> TableInfo {
        TableInfo {
            schema: schema.into(),
            name: name.into(),
            kind: TableKind::Table,
            owner: owner.into(),
            can_select: false,
            can_insert: false,
            can_update: false,
            can_delete: false,
        }
    }

    fn ctx(role: &str) -> ExplainContext {
        ExplainContext {
            role: role.into(),
            search_path: Some("public".into()),
            caller_arn: Some(
                "arn:aws:sts::123456789012:assumed-role/AWSReservedSSO_Dev_abc/me@example.com"
                    .into(),
            ),
            is_admin: role == "admin",
            known_tables: vec![
                table("app_staging", "users", "app_staging"),
                table("app_staging", "Order Items", "app_staging"),
            ],
        }
    }

    #[test]
    fn explain_relation_in_other_schema() {
        let e = explain(
            &db("42P01", "relation \"users\" does not exist"),
            &ctx("admin"),
        );
        assert_eq!(e.fix, FixAction::SwitchSchema("app_staging".into()));
        assert!(
            e.body.contains("public") && e.body.contains("app_staging"),
            "{}",
            e.body
        );
    }

    #[test]
    fn explain_relation_missing_everywhere() {
        let e = explain(
            &db("42P01", "relation \"nope\" does not exist"),
            &ctx("admin"),
        );
        assert_eq!(e.fix, FixAction::None);
        assert!(e.body.contains("nope"));
    }

    #[test]
    fn explain_permission_denied_table() {
        let e = explain(
            &db("42501", "permission denied for table users"),
            &ctx("readonly"),
        );
        assert_eq!(
            e.fix,
            FixAction::ShowGrant {
                table: "users".into(),
                owner: "app_staging".into(),
                sql: "GRANT SELECT ON \"app_staging\".\"users\" TO \"readonly\";".into(),
            }
        );
        assert!(
            e.body.contains("app_staging") && e.body.contains("readonly"),
            "{}",
            e.body
        );
    }

    #[test]
    fn explain_permission_denied_quotes_hostile_names() {
        let e = explain(
            &db("42501", "permission denied for table Order Items"),
            &ctx("team \"rw\""),
        );
        let FixAction::ShowGrant { sql, .. } = e.fix else {
            panic!("{:?}", e.fix)
        };
        assert_eq!(
            sql,
            "GRANT SELECT ON \"app_staging\".\"Order Items\" TO \"team \"\"rw\"\"\";"
        );
    }

    #[test]
    fn explain_permission_denied_schema() {
        let e = explain(
            &db("42501", "permission denied for schema app_staging"),
            &ctx("readonly"),
        );
        let FixAction::ShowGrant { sql, .. } = &e.fix else {
            panic!("{:?}", e.fix)
        };
        assert_eq!(
            sql,
            "GRANT USAGE ON SCHEMA \"app_staging\" TO \"readonly\";"
        );
        assert!(e.body.contains("USAGE"));
    }

    #[test]
    fn explain_auth_failed_admin() {
        let e = explain(
            &db("28000", "unable to accept connection, access denied"),
            &ctx("admin"),
        );
        assert!(e.body.contains("dsql:DbConnectAdmin"), "{}", e.body);
        assert!(e.body.contains("AWSReservedSSO_Dev_abc"), "{}", e.body);
        assert_eq!(e.fix, FixAction::None);
    }

    #[test]
    fn explain_auth_failed_custom() {
        let e = explain(
            &db("28000", "unable to accept connection, access denied"),
            &ctx("readonly"),
        );
        assert!(
            e.body.contains("dsql:DbConnect") && !e.body.contains("DbConnectAdmin"),
            "{}",
            e.body
        );
        assert!(e.body.contains("AWS IAM GRANT"), "{}", e.body);
        assert!(e.body.contains("readonly"), "{}", e.body);
    }

    #[test]
    fn explain_wrong_region_token() {
        let err = CoreError::Db {
            sqlstate: Some("08006".into()),
            message: "unable to accept connection, access denied".into(),
            detail: None,
            hint: Some("Credential should be scoped to a valid region.".into()),
        };
        let e = explain(&err, &ctx("admin"));
        assert!(e.body.contains("region"), "{}", e.body);
    }

    #[test]
    fn explain_expired_sso() {
        let err = CoreError::Aws {
            kind: AwsErrorKind::SsoExpired,
            message: "Token has expired".into(),
        };
        assert_eq!(explain(&err, &ctx("admin")).fix, FixAction::SignIn);
    }

    #[test]
    fn explain_aws_access_denied() {
        let err = CoreError::Aws {
            kind: AwsErrorKind::AccessDenied,
            message: "not authorized to perform: dsql:ListClusters".into(),
        };
        let e = explain(&err, &ctx("admin"));
        assert!(e.body.contains("dsql:ListClusters"), "{}", e.body);
    }

    #[test]
    fn explain_occ() {
        let e = explain(
            &db(
                "40001",
                "change conflicts with another transaction, please retry: (OC000)",
            ),
            &ctx("admin"),
        );
        assert_eq!(e.fix, FixAction::Retry);
    }

    #[test]
    fn explain_aborted_tx() {
        let e = explain(
            &db(
                "25P02",
                "current transaction is aborted, commands ignored until end of transaction block",
            ),
            &ctx("admin"),
        );
        assert_eq!(e.fix, FixAction::Rollback);
    }

    #[test]
    fn explain_unsupported_feature() {
        let e = explain(
            &db("0A000", "unsupported statement: Truncate"),
            &ctx("admin"),
        );
        assert!(e.body.contains("unsupported statement: Truncate"));
        assert_eq!(e.fix, FixAction::None);
    }

    #[test]
    fn explain_unknown_keeps_raw() {
        let e = explain(&db("XX000", "something odd"), &ctx("admin"));
        assert!(
            e.body.contains("XX000") && e.body.contains("something odd"),
            "{}",
            e.body
        );
        assert_eq!(e.fix, FixAction::None);
    }
}
