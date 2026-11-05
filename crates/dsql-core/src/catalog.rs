//! What the schema tree shows: schemas, tables with the current role's privileges, columns, indexes, primary keys,
//! and which schema to open by default.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::conn::{Session, db_error};
use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaInfo {
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TableKind {
    Table,
    View,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
    pub owner: String,
    pub can_select: bool,
    pub can_insert: bool,
    pub can_update: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub is_pk: bool,
    pub ordinal: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// Schemas a user would browse. Excludes system schemas, and `public` when it holds nothing.
pub async fn schemas(s: &Session) -> Result<Vec<SchemaInfo>, CoreError> {
    let rows = s
        .client
        .query(
            "select n.nspname::text, pg_get_userbyid(n.nspowner)::text, \
                    exists (select 1 from pg_class c where c.relnamespace = n.oid \
                            and c.relkind in ('r','p','v','m','S')) \
             from pg_namespace n \
             where n.nspname not in ('pg_catalog', 'information_schema', 'sys', 'pg_toast') \
               and n.nspname not like 'pg\\_temp\\_%' and n.nspname not like 'pg\\_toast\\_temp\\_%' \
             order by 1",
            &[],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<_, String>(0) != "public" || r.get::<_, bool>(2))
        .map(|r| SchemaInfo {
            name: r.get(0),
            owner: r.get(1),
        })
        .collect())
}

/// Tables, views and sequences in `schema`, with what the current role may do on each.
pub async fn tables(s: &Session, schema: &str) -> Result<Vec<TableInfo>, CoreError> {
    let rows = s
        .client
        .query(
            "select n.nspname::text, c.relname::text, c.relkind::text, pg_get_userbyid(c.relowner)::text, \
                    has_table_privilege(c.oid, 'SELECT'), has_table_privilege(c.oid, 'INSERT'), \
                    has_table_privilege(c.oid, 'UPDATE'), has_table_privilege(c.oid, 'DELETE') \
             from pg_class c join pg_namespace n on n.oid = c.relnamespace \
             where n.nspname = $1 and c.relkind in ('r','p','v','m','S') \
             order by 2",
            &[&schema],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|r| TableInfo {
            schema: r.get(0),
            name: r.get(1),
            kind: table_kind(&r.get::<_, String>(2)),
            owner: r.get(3),
            can_select: r.get(4),
            can_insert: r.get(5),
            can_update: r.get(6),
            can_delete: r.get(7),
        })
        .collect())
}

fn table_kind(relkind: &str) -> TableKind {
    match relkind {
        "v" | "m" => TableKind::View,
        "S" => TableKind::Sequence,
        _ => TableKind::Table,
    }
}

/// The key columns of a primary-key index, in key order. DSQL's primary-key indexes cover every column, so only the
/// first `indnkeyatts` entries of `indkey` are the key (Task 1 spike); `attnum = any(indkey)` would list them all.
const PRIMARY_KEY_SQL: &str = "select a.attname::text \
     from pg_index ix \
     join pg_class t on t.oid = ix.indrelid join pg_namespace n on n.oid = t.relnamespace \
     cross join lateral unnest(ix.indkey) with ordinality k(attnum, ord) \
     join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = k.attnum \
     where n.nspname = $1 and t.relname = $2 and ix.indisprimary and k.ord <= ix.indnkeyatts \
     order by k.ord";

pub async fn primary_key(s: &Session, schema: &str, table: &str) -> Result<Vec<String>, CoreError> {
    let rows = s
        .client
        .query(PRIMARY_KEY_SQL, &[&schema, &table])
        .await
        .map_err(db_error)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

pub async fn columns(s: &Session, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, CoreError> {
    let pk = primary_key(s, schema, table).await?;
    let rows = s
        .client
        .query(
            "select a.attname::text, format_type(a.atttypid, a.atttypmod), not a.attnotnull, \
                    pg_get_expr(d.adbin, d.adrelid), a.attnum::int4 \
             from pg_attribute a \
             join pg_class c on c.oid = a.attrelid join pg_namespace n on n.oid = c.relnamespace \
             left join pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum \
             where n.nspname = $1 and c.relname = $2 and a.attnum > 0 and not a.attisdropped \
             order by a.attnum",
            &[&schema, &table],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|r| {
            let name: String = r.get(0);
            ColumnInfo {
                is_pk: pk.contains(&name),
                name,
                data_type: r.get(1),
                nullable: r.get(2),
                default: r.get(3),
                ordinal: r.get(4),
            }
        })
        .collect())
}

/// Indexes with their key columns only (DSQL indexes may also carry included columns).
pub async fn indexes(s: &Session, schema: &str, table: &str) -> Result<Vec<IndexInfo>, CoreError> {
    let rows = s
        .client
        .query(
            "select i.relname::text, ix.indisunique, \
                    array(select a.attname::text from unnest(ix.indkey) with ordinality k(attnum, ord) \
                          join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = k.attnum \
                          where k.ord <= ix.indnkeyatts order by k.ord) \
             from pg_index ix \
             join pg_class i on i.oid = ix.indexrelid \
             join pg_class t on t.oid = ix.indrelid join pg_namespace n on n.oid = t.relnamespace \
             where n.nspname = $1 and t.relname = $2 \
             order by 1",
            &[&schema, &table],
        )
        .await
        .map_err(db_error)?;
    Ok(rows
        .iter()
        .map(|r| IndexInfo {
            name: r.get(0),
            unique: r.get(1),
            columns: r.get(2),
        })
        .collect())
}

/// The current role's own `search_path` setting (`ALTER ROLE ... SET search_path`), if it has one.
pub async fn role_search_path(s: &Session) -> Result<Option<String>, CoreError> {
    let row = s
        .client
        .query_one(
            "select rolconfig::text[] from pg_roles where rolname = current_user",
            &[],
        )
        .await
        .map_err(db_error)?;
    let config: Option<Vec<String>> = row.get(0);
    Ok(config.and_then(|c| first_schema_in_role_config(&c)))
}

/// The first real schema in a role's `search_path=...` setting, skipping `"$user"` and `public`.
pub fn first_schema_in_role_config(config: &[String]) -> Option<String> {
    let value = config.iter().find_map(|c| c.strip_prefix("search_path="))?;
    split_search_path(value)
        .into_iter()
        .find(|s| s != "$user" && s != "public")
}

/// Splits `"$user", public, "My ""App"""` into unquoted names, honouring quotes and doubled quotes.
fn split_search_path(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => names.push(std::mem::take(&mut current)),
            c if c.is_whitespace() && !quoted => {}
            c => current.push(c),
        }
    }
    names.push(current);
    names.into_iter().filter(|n| !n.is_empty()).collect()
}

/// Which schema to open, or `None` to let the user choose. In order: the role's own default `search_path` schema if
/// it exists; the only non-`public` schema; a schema named like the current user. Several candidate schemas are never
/// guessed between.
pub fn pick_default_schema(
    schemas: &[SchemaInfo],
    role_search_path: Option<&str>,
    current_user: &str,
) -> Option<String> {
    let exists = |name: &str| schemas.iter().any(|s| s.name == name);
    if let Some(name) = role_search_path.filter(|n| exists(n)) {
        return Some(name.to_string());
    }
    let non_public: Vec<&SchemaInfo> = schemas.iter().filter(|s| s.name != "public").collect();
    if let [only] = non_public.as_slice() {
        return Some(only.name.clone());
    }
    exists(current_user).then(|| current_user.to_string())
}

/// Production-looking schemas (`_production` / `_prod` suffix) or clusters tagged `production`. Pre-fills the
/// connection's production flag; the user can change it.
pub fn looks_like_production(schema: &str, tags: &BTreeMap<String, String>) -> bool {
    schema.ends_with("_production")
        || schema.ends_with("_prod")
        || tags.values().any(|v| v.eq_ignore_ascii_case("production"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemas(names: &[&str]) -> Vec<SchemaInfo> {
        names
            .iter()
            .map(|n| SchemaInfo {
                name: n.to_string(),
                owner: "admin".into(),
            })
            .collect()
    }

    #[test]
    fn pick_default_single_schema() {
        assert_eq!(
            pick_default_schema(&schemas(&["public", "app_staging"]), None, "admin").as_deref(),
            Some("app_staging")
        );
        assert_eq!(
            pick_default_schema(&schemas(&["app_staging"]), None, "admin").as_deref(),
            Some("app_staging")
        );
    }

    #[test]
    fn pick_default_schema_named_like_user() {
        let list = schemas(&["app_production", "app_staging"]);
        assert_eq!(
            pick_default_schema(&list, None, "app_staging").as_deref(),
            Some("app_staging")
        );
    }

    #[test]
    fn pick_default_prefers_role_search_path() {
        let list = schemas(&["app_production", "app_staging"]);
        assert_eq!(
            pick_default_schema(&list, Some("app_production"), "readonly").as_deref(),
            Some("app_production")
        );
        // A setting that points at a schema that is gone is ignored.
        assert_eq!(
            pick_default_schema(&list, Some("old_schema"), "app_staging").as_deref(),
            Some("app_staging")
        );
    }

    #[test]
    fn pick_default_none_when_ambiguous() {
        let list = schemas(&["public", "app_production", "app_staging"]);
        assert_eq!(pick_default_schema(&list, None, "admin"), None);
        assert_eq!(
            pick_default_schema(&schemas(&["public"]), None, "admin"),
            None
        );
        assert_eq!(pick_default_schema(&[], None, "admin"), None);
    }

    #[test]
    fn role_config_search_path() {
        let cfg = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            first_schema_in_role_config(&cfg(&["search_path=app_staging"])).as_deref(),
            Some("app_staging")
        );
        assert_eq!(
            first_schema_in_role_config(&cfg(&[
                "statement_timeout=0",
                "search_path=\"$user\", public, \"My App\""
            ]))
            .as_deref(),
            Some("My App")
        );
        assert_eq!(
            first_schema_in_role_config(&cfg(&["search_path=\"we\"\"ird\""])).as_deref(),
            Some("we\"ird")
        );
        assert_eq!(
            first_schema_in_role_config(&cfg(&["search_path=\"$user\", public"])),
            None
        );
        assert_eq!(
            first_schema_in_role_config(&cfg(&["statement_timeout=0"])),
            None
        );
    }

    #[test]
    fn looks_like_production_suffix() {
        let none = BTreeMap::new();
        assert!(looks_like_production("myapp_production", &none));
        assert!(looks_like_production("myapp_prod", &none));
        assert!(!looks_like_production("myapp_staging", &none));
        assert!(!looks_like_production("production_reports_archive", &none));
        assert!(!looks_like_production("myapp_product", &none));
    }

    #[test]
    fn looks_like_production_tag() {
        let tags = BTreeMap::from([(
            "arco-cloud:environment".to_string(),
            "Production".to_string(),
        )]);
        assert!(looks_like_production("myapp_x", &tags));
        let tags = BTreeMap::from([("Name".to_string(), "staging".to_string())]);
        assert!(!looks_like_production("myapp_x", &tags));
    }

    #[test]
    fn primary_key_uses_key_columns_only() {
        assert!(PRIMARY_KEY_SQL.contains("k.ord <= ix.indnkeyatts"));
        assert!(!PRIMARY_KEY_SQL.contains("any(ix.indkey)"));
    }
}
