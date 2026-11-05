//! The "Give my team access" wizard: the SQL that creates or removes a DSQL role mapped to an IAM role, split by who
//! can run it. `admin` can create the role, map it and grant schema usage; only the tables' owner (the app's role) can
//! grant on its tables. Recipe: spec section 6.5; ordering and the admin takeover: `docs/spike-findings.md`.

use serde::{Deserialize, Serialize};

use crate::conn::{Session, db_error};
use crate::error::CoreError;
use crate::quote::{ident, literal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

impl Access {
    fn privileges(self) -> &'static str {
        match self {
            Access::ReadOnly => "SELECT",
            Access::ReadWrite => "SELECT, INSERT, UPDATE, DELETE",
        }
    }
}

/// `admin_sql` runs as `admin`, in order, one statement per transaction. `owner_sql` must run as the tables' owner;
/// `owner_tinker` is the same as a Laravel tinker snippet, for `vendor/bin/arco tinker`, Arco Cloud's Run command or
/// the console Lambda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RolePlan {
    pub admin_sql: Vec<String>,
    pub owner_sql: Vec<String>,
    pub owner_tinker: String,
}

/// Creates `role`, maps it to `iam_role_arn`, gives it the schema as its `search_path`, and grants `access` on
/// `tables` (owner side). Tables added later need the owner grant again.
pub fn create_plan(
    role: &str,
    iam_role_arn: &str,
    schema: &str,
    tables: &[String],
    access: Access,
) -> Result<RolePlan, CoreError> {
    check_role_name(role)?;
    let (r, s, arn) = (ident(role), ident(schema), arn_literal(iam_role_arn)?);
    let owner_sql: Vec<String> = tables
        .iter()
        .map(|t| format!("GRANT {} ON {s}.{} TO {r}", access.privileges(), ident(t)))
        .collect();
    Ok(RolePlan {
        admin_sql: vec![
            format!("CREATE ROLE {r} WITH LOGIN"),
            format!("AWS IAM GRANT {r} TO {arn}"),
            format!("GRANT USAGE ON SCHEMA {s} TO {r}"),
            format!("ALTER ROLE {r} SET search_path = {s}"),
        ],
        owner_tinker: tinker_statements(&owner_sql),
        owner_sql,
    })
}

/// Undoes `create_plan`. `DROP ROLE` refuses while the role holds privileges or a mapping, so the owner's REVOKEs run
/// first, then `admin` unmaps, revokes schema usage and drops.
pub fn remove_plan(
    role: &str,
    iam_role_arn: &str,
    schema: &str,
    tables: &[String],
) -> Result<RolePlan, CoreError> {
    check_role_name(role)?;
    let (r, s, arn) = (ident(role), ident(schema), arn_literal(iam_role_arn)?);
    let owner_sql: Vec<String> = tables
        .iter()
        .map(|t| format!("REVOKE ALL ON {s}.{} FROM {r}", ident(t)))
        .collect();
    Ok(RolePlan {
        admin_sql: vec![
            format!("AWS IAM REVOKE {r} FROM {arn}"),
            format!("REVOKE USAGE ON SCHEMA {s} FROM {r}"),
            format!("DROP ROLE {r}"),
        ],
        owner_tinker: tinker_statements(&owner_sql),
        owner_sql,
    })
}

/// Runs owner-side SQL as `admin` by joining the app's role for a moment. DSQL has no `SET ROLE`, but membership alone
/// inherits the owner's privileges, including `GRANT` (Task 1 spike). Offer it only when `admin_can_take_over` is true,
/// and only with the user's explicit consent.
pub fn grant_owner_privileges_via_admin_plan(app_role: &str, sql: &[String]) -> Vec<String> {
    let r = ident(app_role);
    let mut out = Vec::with_capacity(sql.len() + 2);
    out.push(format!("GRANT {r} TO admin"));
    out.extend(sql.iter().cloned());
    out.push(format!("REVOKE {r} FROM admin"));
    out
}

/// Whether the current user (normally `admin`) holds ADMIN OPTION on `app_role`, which DSQL gives the role's creator.
pub async fn admin_can_take_over(s: &Session, app_role: &str) -> Result<bool, CoreError> {
    let row = s
        .client
        .query_one(
            "select coalesce(bool_or(am.admin_option), false) from pg_auth_members am \
             join pg_roles r on r.oid = am.roleid join pg_roles m on m.oid = am.member \
             where r.rolname = $1 and m.rolname = current_user",
            &[&app_role],
        )
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

/// Existing role-to-IAM mappings as `(role, iam_role_arn)`, sorted by role.
pub async fn existing_mappings(s: &Session) -> Result<Vec<(String, String)>, CoreError> {
    let rows = s
        .client
        .query(
            "select pg_role_name::text, arn::text from sys.iam_pg_role_mappings order by 1, 2",
            &[],
        )
        .await
        .map_err(db_error)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

fn check_role_name(role: &str) -> Result<(), CoreError> {
    let reserved = role.eq_ignore_ascii_case("admin")
        || role.eq_ignore_ascii_case("public")
        || role.to_ascii_lowercase().starts_with("pg_");
    if role.trim().is_empty() || role.len() > 63 || reserved {
        return Err(CoreError::Config(format!(
            "\"{role}\" cannot be used as a role name; pick a new name of up to 63 characters"
        )));
    }
    Ok(())
}

fn arn_literal(arn: &str) -> Result<String, CoreError> {
    if !arn.starts_with("arn:aws") || !arn.contains(":role/") {
        return Err(CoreError::Config(format!(
            "{arn} is not an IAM role ARN (arn:aws:iam::<account>:role/<path>/<name>)"
        )));
    }
    literal(Some(arn))
}

/// `DB::statement('...');` per statement, as a PHP single-quoted string (`\` and `'` escaped).
fn tinker_statements(sql: &[String]) -> String {
    sql.iter()
        .map(|s| {
            format!(
                "DB::statement('{}');",
                s.replace('\\', "\\\\").replace('\'', "\\'")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARN: &str = "arn:aws:iam::111122223333:role/aws-reserved/sso.amazonaws.com/ReadOnly_abc";

    fn tables() -> Vec<String> {
        vec!["users".into(), "Order Items".into()]
    }

    #[test]
    fn role_plan_quotes_schema() {
        let plan = create_plan("team rw", ARN, "my\"app", &tables(), Access::ReadOnly).unwrap();
        assert_eq!(
            plan.admin_sql,
            [
                r#"CREATE ROLE "team rw" WITH LOGIN"#,
                &format!(r#"AWS IAM GRANT "team rw" TO '{ARN}'"#),
                r#"GRANT USAGE ON SCHEMA "my""app" TO "team rw""#,
                r#"ALTER ROLE "team rw" SET search_path = "my""app""#,
            ]
        );
        assert_eq!(
            plan.owner_sql[1],
            r#"GRANT SELECT ON "my""app"."Order Items" TO "team rw""#
        );
    }

    #[test]
    fn create_plan_readonly_grants_select_only() {
        let plan = create_plan("readonly", ARN, "app", &tables(), Access::ReadOnly).unwrap();
        assert_eq!(
            plan.owner_sql,
            [
                r#"GRANT SELECT ON "app"."users" TO "readonly""#,
                r#"GRANT SELECT ON "app"."Order Items" TO "readonly""#,
            ]
        );
    }

    #[test]
    fn create_plan_readwrite() {
        let plan = create_plan("rw", ARN, "app", &tables(), Access::ReadWrite).unwrap();
        assert_eq!(
            plan.owner_sql[0],
            r#"GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."users" TO "rw""#
        );
    }

    #[test]
    fn remove_plan_revokes_before_drop() {
        let plan = remove_plan("readonly", ARN, "app", &tables()).unwrap();
        assert!(plan.owner_sql.iter().all(|s| s.starts_with("REVOKE ")));
        assert_eq!(plan.owner_sql.len(), 2);
        assert_eq!(
            plan.admin_sql,
            [
                &format!(r#"AWS IAM REVOKE "readonly" FROM '{ARN}'"#),
                r#"REVOKE USAGE ON SCHEMA "app" FROM "readonly""#,
                r#"DROP ROLE "readonly""#,
            ]
        );
    }

    #[test]
    fn plans_reject_bad_names_and_arns() {
        for role in ["", "admin", "ADMIN", "pg_x", "public"] {
            assert!(matches!(
                create_plan(role, ARN, "app", &[], Access::ReadOnly),
                Err(CoreError::Config(_))
            ));
        }
        let sts = "arn:aws:sts::111122223333:assumed-role/ReadOnly_abc/me";
        assert!(matches!(
            create_plan("r", sts, "app", &[], Access::ReadOnly),
            Err(CoreError::Config(_))
        ));
        assert!(remove_plan("r", "not an arn", "app", &[]).is_err());
    }

    #[test]
    fn owner_tinker_escapes_quotes() {
        let plan =
            create_plan("r", ARN, "app", &["it's\\t".to_string()], Access::ReadOnly).unwrap();
        assert_eq!(
            plan.owner_tinker,
            r#"DB::statement('GRANT SELECT ON "app"."it\'s\\t" TO "r"');"#
        );
        let two = create_plan("r", ARN, "app", &tables(), Access::ReadOnly).unwrap();
        assert_eq!(two.owner_tinker.matches("DB::statement(").count(), 2);
    }

    #[test]
    fn admin_takeover_plan_has_no_set_role() {
        let owner = vec![r#"GRANT SELECT ON "app"."users" TO "r""#.to_string()];
        let plan = grant_owner_privileges_via_admin_plan("app_role", &owner);
        assert_eq!(
            plan,
            [
                r#"GRANT "app_role" TO admin"#,
                r#"GRANT SELECT ON "app"."users" TO "r""#,
                r#"REVOKE "app_role" FROM admin"#,
            ]
        );
        assert!(plan.iter().all(|s| !s.to_uppercase().contains("SET ROLE")));
    }
}
