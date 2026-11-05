//! Connections, the catalog, running SQL, table pages, grid edits and error explanations.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::State;

use dsql_core::CoreError;
use dsql_core::catalog::{self, ColumnInfo, IndexInfo, SchemaInfo, TableInfo};
use dsql_core::classify::{self, Classified, StatementKind};
use dsql_core::conn::{Pool, db_error};
use dsql_core::edits::{self, ApplyReport, ChangeSet, MAX_ROWS_PER_TX};
use dsql_core::exec::{self, MAX_ROWS, ResultSet, ScriptResult, StatementOutcome};
use dsql_core::explain::{self, ExplainContext, Explanation};
use dsql_core::identity;
use dsql_core::quote::{ident, literal};
use dsql_core::store::{HistoryEntry, SavedConnection};

use crate::state::{AppState, OpenConnection, POOL_SIZE, target};

/// OCC retries per batch when saving grid edits.
const APPLY_RETRIES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionHandle {
    pub id: String,
    pub current_user: String,
    /// The schema the connection opened in; `None` means the user has to pick one.
    pub schema: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Contains,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    pub offset: u64,
    pub limit: u64,
    /// Column and ascending.
    pub order_by: Vec<(String, bool)>,
    pub filters: Vec<Filter>,
    /// A WHERE condition typed by the user, ANDed with the filters. Refused unless it only reads.
    pub where_sql: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TablePage {
    pub result: ResultSet,
    /// The query that produced the page, shown so the user can copy it into the editor.
    pub sql: String,
}

/// Opens a pool for a saved connection and checks it with one session. Without a saved schema, picks the role's
/// default one when it is clear (`catalog::pick_default_schema`).
#[tauri::command]
pub async fn open_connection(
    state: State<'_, AppState>,
    saved: SavedConnection,
) -> Result<ConnectionHandle, CoreError> {
    let mut pool = Pool::new(target(&saved, saved.schema.clone()), POOL_SIZE);
    let (current_user, schema) = {
        let s = pool.get().await?;
        let current_user: String = s
            .client
            .query_one("select current_user::text", &[])
            .await
            .map_err(db_error)?
            .get(0);
        let schema = match &saved.schema {
            Some(schema) => Some(schema.clone()),
            None => {
                let schemas = catalog::schemas(&s).await?;
                let role_path = catalog::role_search_path(&s).await?;
                catalog::pick_default_schema(&schemas, role_path.as_deref(), &current_user)
            }
        };
        (current_user, schema)
    };
    if schema != saved.schema {
        pool = Pool::new(target(&saved, schema.clone()), POOL_SIZE);
    }
    let id = state.new_id("conn");
    state.connections.lock().expect("connections lock").insert(
        id.clone(),
        Arc::new(OpenConnection {
            saved,
            pool: Mutex::new(pool),
            running: Mutex::new(None),
        }),
    );
    Ok(ConnectionHandle {
        id,
        current_user,
        schema,
    })
}

#[tauri::command]
pub fn close_connection(state: State<'_, AppState>, id: String) {
    state
        .connections
        .lock()
        .expect("connections lock")
        .remove(&id);
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<SchemaInfo>, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    catalog::schemas(&s).await
}

/// Switches the connection's `search_path`: new sessions open in `schema`, idle ones are closed.
#[tauri::command]
pub fn set_schema(state: State<'_, AppState>, id: String, schema: String) -> Result<(), CoreError> {
    let conn = state.connection(&id)?;
    let mut pool = conn.pool.lock().expect("pool lock");
    pool.clear();
    *pool = Pool::new(target(&conn.saved, Some(schema)), POOL_SIZE);
    Ok(())
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, AppState>,
    id: String,
    schema: String,
) -> Result<Vec<TableInfo>, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    catalog::tables(&s, &schema).await
}

#[tauri::command]
pub async fn table_columns(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
) -> Result<Vec<ColumnInfo>, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    catalog::columns(&s, &schema, &table).await
}

#[tauri::command]
pub async fn table_indexes(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
) -> Result<Vec<IndexInfo>, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    catalog::indexes(&s, &schema, &table).await
}

#[tauri::command]
pub async fn table_page(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
    page: PageRequest,
) -> Result<TablePage, CoreError> {
    let sql = table_page_sql(&schema, &table, &page)?;
    let conn = state.connection(&id)?;
    let result = run_tracked(&conn, &sql, MAX_ROWS).await;
    match result.outcomes.into_iter().next() {
        Some(StatementOutcome::Rows(result)) => Ok(TablePage { result, sql }),
        Some(StatementOutcome::Error(err)) => Err(err),
        other => Err(CoreError::Config(format!(
            "unexpected result for a table page: {other:?}"
        ))),
    }
}

#[tauri::command]
pub fn classify_sql(sql: String) -> Classified {
    classify::classify(&sql)
}

/// Runs a script statement by statement and records it in the history. A read-only connection refuses anything that
/// is not a read before sending it.
#[tauri::command]
pub async fn run_sql(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> Result<ScriptResult, CoreError> {
    let conn = state.connection(&id)?;
    if conn.saved.read_only && classify::blocked_by_read_only(&classify::classify(&sql)) {
        return Err(CoreError::Config(
            "this connection is read-only; only queries that read can run".to_string(),
        ));
    }
    let started_at = chrono::Utc::now();
    let result = run_tracked(&conn, &sql, MAX_ROWS).await;
    let (rows, error) = summarize(&result);
    state.with_store(|store| {
        store.add_history(&HistoryEntry {
            id: None,
            connection_id: conn.saved.id,
            sql: sql.clone(),
            started_at,
            duration_ms: result.duration_ms,
            rows,
            error,
        })
    })?;
    Ok(result)
}

/// Stops the statement running on the connection, if any.
#[tauri::command]
pub async fn cancel(state: State<'_, AppState>, id: String) -> Result<(), CoreError> {
    let canceller = state
        .connection(&id)?
        .running
        .lock()
        .expect("running lock")
        .clone();
    if let Some(canceller) = canceller {
        canceller.cancel().await;
    }
    Ok(())
}

/// The transactions `apply_changes` would run, for the preview.
#[tauri::command]
pub fn preview_changes(changeset: ChangeSet) -> Result<Vec<Vec<String>>, CoreError> {
    Ok(edits::batches(
        edits::build_statements(&changeset)?,
        MAX_ROWS_PER_TX,
    ))
}

#[tauri::command]
pub async fn apply_changes(
    state: State<'_, AppState>,
    id: String,
    changeset: ChangeSet,
) -> Result<ApplyReport, CoreError> {
    let conn = state.connection(&id)?;
    if conn.saved.read_only {
        return Err(CoreError::Config(
            "this connection is read-only; edits cannot be saved".to_string(),
        ));
    }
    let batches = edits::batches(edits::build_statements(&changeset)?, MAX_ROWS_PER_TX);
    Ok(edits::apply(&conn.pool(), batches, APPLY_RETRIES).await)
}

/// Explains an error the connection returned, with the context it needs: role, schema, caller and the tables of every
/// schema (so "it is in another schema" and grant snippets work). Missing context only makes the text less specific.
#[tauri::command]
pub async fn explain_error(
    state: State<'_, AppState>,
    id: String,
    err: CoreError,
) -> Result<Explanation, CoreError> {
    let conn = state.connection(&id)?;
    let mut known_tables = Vec::new();
    if let Ok(s) = conn.pool().get().await
        && let Ok(schemas) = catalog::schemas(&s).await
    {
        for schema in schemas {
            known_tables.extend(catalog::tables(&s, &schema.name).await.unwrap_or_default());
        }
    }
    let caller_arn = match identity::sdk_config(&conn.saved.profile, &conn.saved.region).await {
        Ok(cfg) => identity::caller_identity(&cfg).await.ok().map(|me| me.arn),
        Err(_) => None,
    };
    let ctx = ExplainContext {
        role: conn.saved.role.clone(),
        search_path: conn.schema(),
        caller_arn,
        is_admin: conn.saved.role == "admin",
        known_tables,
    };
    Ok(explain::explain(&err, &ctx))
}

/// Runs `sql` on a pooled session while `cancel` can reach it.
pub async fn run_tracked(conn: &OpenConnection, sql: &str, max_rows: usize) -> ScriptResult {
    let s = match conn.pool().get().await {
        Ok(s) => s,
        Err(err) => {
            return ScriptResult {
                outcomes: vec![StatementOutcome::Error(err)],
                duration_ms: 0,
            };
        }
    };
    *conn.running.lock().expect("running lock") = Some(s.canceller());
    let started = Instant::now();
    let mut result = exec::run_script(&s, sql, max_rows).await;
    *conn.running.lock().expect("running lock") = None;
    result.duration_ms = result.duration_ms.max(started.elapsed().as_millis() as u64);
    result
}

/// Rows returned or changed, and the error message, for the history.
fn summarize(result: &ScriptResult) -> (Option<u64>, Option<String>) {
    let mut rows = None;
    let mut error = None;
    for outcome in &result.outcomes {
        match outcome {
            StatementOutcome::Rows(set) => rows = Some(rows.unwrap_or(0) + set.rows.len() as u64),
            StatementOutcome::Command { rows_affected, .. } => {
                rows = Some(rows.unwrap_or(0) + rows_affected)
            }
            StatementOutcome::Error(err) => error = Some(err.to_string()),
        }
    }
    (rows, error)
}

/// `SELECT * FROM "schema"."table" WHERE ... ORDER BY ... LIMIT ... OFFSET ...`, with every name and value quoted.
pub fn table_page_sql(schema: &str, table: &str, page: &PageRequest) -> Result<String, CoreError> {
    let mut conditions = page
        .filters
        .iter()
        .map(filter_sql)
        .collect::<Result<Vec<_>, CoreError>>()?;
    if let Some(where_sql) = page.where_sql.as_deref().filter(|w| !w.trim().is_empty()) {
        check_where_reads_only(where_sql)?;
        conditions.push(format!("({where_sql})"));
    }
    let mut sql = format!("SELECT * FROM {}.{}", ident(schema), ident(table));
    if !conditions.is_empty() {
        sql += &format!(" WHERE {}", conditions.join(" AND "));
    }
    if !page.order_by.is_empty() {
        let order: Vec<String> = page
            .order_by
            .iter()
            .map(|(col, asc)| format!("{} {}", ident(col), if *asc { "ASC" } else { "DESC" }))
            .collect();
        sql += &format!(" ORDER BY {}", order.join(", "));
    }
    let limit = page.limit.clamp(1, MAX_ROWS as u64);
    sql += &format!(" LIMIT {limit} OFFSET {}", page.offset);
    Ok(sql)
}

fn filter_sql(f: &Filter) -> Result<String, CoreError> {
    let col = ident(&f.column);
    let value = || {
        f.value
            .as_deref()
            .ok_or_else(|| CoreError::Config(format!("the filter on {} needs a value", f.column)))
    };
    Ok(match f.op {
        FilterOp::IsNull => format!("{col} IS NULL"),
        FilterOp::IsNotNull => format!("{col} IS NOT NULL"),
        FilterOp::Contains => {
            let escaped = value()?
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            format!(
                "{col}::text ILIKE {}",
                literal(Some(&format!("%{escaped}%")))?
            )
        }
        op => {
            let sym = match op {
                FilterOp::Eq => "=",
                FilterOp::Ne => "<>",
                FilterOp::Lt => "<",
                _ => ">",
            };
            format!("{col} {sym} {}", literal(Some(value()?))?)
        }
    })
}

/// The condition must not end the statement and start another, or change anything.
fn check_where_reads_only(where_sql: &str) -> Result<(), CoreError> {
    let classified = classify::classify(&format!("SELECT 1 WHERE {where_sql}"));
    match classified.statements.as_slice() {
        [only] if only.kind == StatementKind::Read => Ok(()),
        _ => Err(CoreError::Config(
            "the WHERE condition must only read; use the SQL editor to change data".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(filters: Vec<Filter>, where_sql: Option<&str>) -> PageRequest {
        PageRequest {
            offset: 100,
            limit: 50,
            order_by: vec![("id".into(), true), ("Created At".into(), false)],
            filters,
            where_sql: where_sql.map(str::to_string),
        }
    }

    fn filter(column: &str, op: FilterOp, value: Option<&str>) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn table_page_sql_builds_filters() {
        let sql = table_page_sql(
            "app",
            "Order",
            &page(
                vec![
                    filter("name", FilterOp::Contains, Some("50%_off\\'x")),
                    filter("id", FilterOp::Gt, Some("10")),
                    filter("deleted_at", FilterOp::IsNull, None),
                ],
                Some("total > 5"),
            ),
        )
        .unwrap();
        assert_eq!(
            sql,
            r#"SELECT * FROM "app"."Order" WHERE "name"::text ILIKE '%50\%\_off\\''x%' AND "id" > '10' AND "deleted_at" IS NULL AND (total > 5) ORDER BY "id" ASC, "Created At" DESC LIMIT 50 OFFSET 100"#
        );
    }

    #[test]
    fn table_page_without_conditions_and_capped_limit() {
        let mut p = PageRequest {
            limit: 1_000_000,
            ..PageRequest::default()
        };
        assert_eq!(
            table_page_sql("s", "t", &p).unwrap(),
            format!(r#"SELECT * FROM "s"."t" LIMIT {MAX_ROWS} OFFSET 0"#)
        );
        p.filters.push(filter("a", FilterOp::Eq, None));
        assert!(matches!(
            table_page_sql("s", "t", &p),
            Err(CoreError::Config(_))
        ));
    }

    #[test]
    fn table_page_rejects_where_with_write() {
        for bad in [
            "1=1; delete from t",
            "true; drop table t",
            "1=1; insert into t values (1)",
        ] {
            assert!(
                matches!(
                    table_page_sql("s", "t", &page(vec![], Some(bad))),
                    Err(CoreError::Config(_))
                ),
                "{bad}"
            );
        }
        assert!(table_page_sql("s", "t", &page(vec![], Some("name = 'a;b'"))).is_ok());
    }
}
