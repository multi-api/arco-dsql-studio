//! Turning the grid's pending edits into SQL, splitting it into transactions DSQL accepts, and applying them with
//! retries on optimistic-concurrency conflicts.
//!
//! Rows are found by primary key only, so a table without one cannot be edited. Values travel as text literals cast
//! to the column's type (`'42'::bigint`), the same text the grid shows, so every type round-trips without a per-type
//! encoder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::conn::{Pool, PooledSession, db_error};
use crate::error::CoreError;
use crate::occ::{backoff, is_occ};
use crate::quote::{ident, literal};

/// DSQL changes at most 3,000 rows in one transaction (Task 1 spike).
pub const MAX_ROWS_PER_TX: usize = 3000;

/// DSQL also caps a write transaction at 10 MiB; the SQL text is a stand-in for the data size, with headroom.
pub const MAX_BYTES_PER_TX: usize = 8 * 1024 * 1024;

/// Column name to value; `None` is SQL `NULL`.
pub type Values = BTreeMap<String, Option<String>>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowUpdate {
    pub pk_values: Values,
    pub set: Values,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub schema: String,
    pub table: String,
    pub pk: Vec<String>,
    /// From `format_type`, for example `bigint` or `character varying(20)`. A column missing here is sent untyped.
    pub column_types: BTreeMap<String, String>,
    pub updates: Vec<RowUpdate>,
    pub inserts: Vec<Values>,
    /// The primary key values of each row to delete.
    pub deletes: Vec<Values>,
}

/// One statement per changed row: deletes first, then updates, then inserts, so deleting a row and adding one with
/// the same key in one save works.
pub fn build_statements(cs: &ChangeSet) -> Result<Vec<String>, CoreError> {
    if cs.pk.is_empty() {
        return Err(CoreError::Config(format!(
            "{}.{} has no primary key, so its rows cannot be edited safely",
            cs.schema, cs.table
        )));
    }
    let table = format!("{}.{}", ident(&cs.schema), ident(&cs.table));
    let mut out = Vec::with_capacity(cs.deletes.len() + cs.updates.len() + cs.inserts.len());

    for pk_values in &cs.deletes {
        out.push(format!(
            "DELETE FROM {table} WHERE {}",
            where_pk(cs, pk_values)?
        ));
    }
    for u in &cs.updates {
        if u.set.is_empty() {
            continue;
        }
        let set = u
            .set
            .iter()
            .map(|(col, v)| Ok(format!("{} = {}", ident(col), value(cs, col, v)?)))
            .collect::<Result<Vec<_>, CoreError>>()?;
        out.push(format!(
            "UPDATE {table} SET {} WHERE {}",
            set.join(", "),
            where_pk(cs, &u.pk_values)?
        ));
    }
    for row in &cs.inserts {
        if row.is_empty() {
            out.push(format!("INSERT INTO {table} DEFAULT VALUES"));
            continue;
        }
        let cols: Vec<String> = row.keys().map(|c| ident(c)).collect();
        let vals = row
            .iter()
            .map(|(col, v)| value(cs, col, v))
            .collect::<Result<Vec<_>, CoreError>>()?;
        out.push(format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            cols.join(", "),
            vals.join(", ")
        ));
    }
    Ok(out)
}

fn value(cs: &ChangeSet, col: &str, v: &Option<String>) -> Result<String, CoreError> {
    let lit = literal(v.as_deref())?;
    Ok(match (v, cs.column_types.get(col)) {
        (Some(_), Some(ty)) => format!("{lit}::{ty}"),
        _ => lit,
    })
}

fn where_pk(cs: &ChangeSet, pk_values: &Values) -> Result<String, CoreError> {
    let parts = cs
        .pk
        .iter()
        .map(|col| match pk_values.get(col) {
            Some(v @ Some(_)) => Ok(format!("{} = {}", ident(col), value(cs, col, v)?)),
            _ => Err(CoreError::Config(format!(
                "a changed row has no value for its primary key column {col}"
            ))),
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    Ok(parts.join(" AND "))
}

/// Groups statements into transactions of at most `max` statements (each touches one row) and about
/// `MAX_BYTES_PER_TX` of SQL.
pub fn batches(stmts: Vec<String>, max: usize) -> Vec<Vec<String>> {
    let max = max.max(1);
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut bytes = 0;
    for stmt in stmts {
        let full = out.last().is_none_or(|b| {
            b.len() >= max || (bytes + stmt.len() > MAX_BYTES_PER_TX && !b.is_empty())
        });
        if full {
            out.push(Vec::new());
            bytes = 0;
        }
        bytes += stmt.len();
        out.last_mut().expect("just pushed").push(stmt);
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ApplyReport {
    /// Batches committed. Each batch is one transaction, so a failure leaves earlier batches saved and later ones not.
    pub batches_applied: usize,
    pub batches_total: usize,
    pub rows_affected: u64,
    /// UPDATE or DELETE statements that matched no row: someone else deleted the row or changed its key.
    pub rows_missing: usize,
    pub retries: u32,
    /// Why the batch after the applied ones failed; `None` when everything was saved.
    pub error: Option<CoreError>,
}

/// Runs each batch in its own transaction. A batch that fails with an OCC conflict is retried as a whole, up to
/// `max_retries` times with a jittered backoff; any other error stops at that batch.
pub async fn apply(pool: &Pool, batches: Vec<Vec<String>>, max_retries: u32) -> ApplyReport {
    let mut report = ApplyReport {
        batches_total: batches.len(),
        ..ApplyReport::default()
    };
    for batch in &batches {
        let mut attempt = 0;
        loop {
            let result = match pool.get().await {
                Ok(mut session) => run_batch(&mut session, batch).await,
                Err(err) => Err(err),
            };
            match result {
                Ok((rows, missing)) => {
                    report.batches_applied += 1;
                    report.rows_affected += rows;
                    report.rows_missing += missing;
                    break;
                }
                Err(err) if is_occ(&err) && attempt < max_retries => {
                    tokio::time::sleep(backoff(attempt)).await;
                    attempt += 1;
                    report.retries += 1;
                }
                Err(err) => {
                    report.error = Some(err);
                    return report;
                }
            }
        }
    }
    report
}

async fn run_batch(
    session: &mut PooledSession,
    batch: &[String],
) -> Result<(u64, usize), CoreError> {
    let result = async {
        session
            .client
            .batch_execute("BEGIN")
            .await
            .map_err(db_error)?;
        let (mut rows, mut missing) = (0, 0);
        for stmt in batch {
            let n = session
                .client
                .execute(stmt.as_str(), &[])
                .await
                .map_err(db_error)?;
            rows += n;
            if n == 0 && !stmt.starts_with("INSERT") {
                missing += 1;
            }
        }
        session
            .client
            .batch_execute("COMMIT")
            .await
            .map_err(db_error)?;
        Ok((rows, missing))
    }
    .await;
    if result.is_err() && session.client.batch_execute("ROLLBACK").await.is_err() {
        session.discard();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, Option<&str>)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
            .collect()
    }

    fn change_set() -> ChangeSet {
        ChangeSet {
            schema: "s".into(),
            table: "t".into(),
            pk: vec!["id".into()],
            column_types: [("id", "bigint"), ("name", "text")]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..ChangeSet::default()
        }
    }

    fn update(id: &str, set: &[(&str, Option<&str>)]) -> RowUpdate {
        RowUpdate {
            pk_values: vals(&[("id", Some(id))]),
            set: vals(set),
        }
    }

    #[test]
    fn edits_update_targets_pk() {
        let mut cs = change_set();
        cs.updates.push(update("1", &[("name", Some("x"))]));
        assert_eq!(
            build_statements(&cs).unwrap(),
            [r#"UPDATE "s"."t" SET "name" = 'x'::text WHERE "id" = '1'::bigint"#]
        );
    }

    #[test]
    fn edits_quote_hostile_identifiers() {
        let mut cs = change_set();
        cs.table = "Order".into();
        cs.pk = vec!["we\"ird".into()];
        cs.updates.push(RowUpdate {
            pk_values: vals(&[("we\"ird", Some("7"))]),
            set: vals(&[("My Col", Some("v"))]),
        });
        assert_eq!(
            build_statements(&cs).unwrap(),
            [r#"UPDATE "s"."Order" SET "My Col" = 'v' WHERE "we""ird" = '7'"#]
        );
    }

    #[test]
    fn edits_null_vs_empty() {
        let mut cs = change_set();
        cs.updates.push(update(
            "1",
            &[("a", None), ("b", Some("")), ("c", Some("NULL"))],
        ));
        assert_eq!(
            build_statements(&cs).unwrap(),
            [r#"UPDATE "s"."t" SET "a" = NULL, "b" = '', "c" = 'NULL' WHERE "id" = '1'::bigint"#]
        );
    }

    #[test]
    fn edits_literal_doubles_quotes() {
        let mut cs = change_set();
        cs.updates.push(update(
            "1",
            &[("name", Some("O'Brien'); drop table t; --"))],
        ));
        assert_eq!(
            build_statements(&cs).unwrap(),
            [
                r#"UPDATE "s"."t" SET "name" = 'O''Brien''); drop table t; --'::text WHERE "id" = '1'::bigint"#
            ]
        );
    }

    #[test]
    fn edits_require_pk() {
        let mut cs = change_set();
        cs.pk.clear();
        cs.updates.push(update("1", &[("name", Some("x"))]));
        assert!(matches!(build_statements(&cs), Err(CoreError::Config(_))));

        let mut cs = change_set();
        cs.deletes.push(vals(&[("id", None)]));
        assert!(matches!(build_statements(&cs), Err(CoreError::Config(_))));
        let mut cs = change_set();
        cs.deletes.push(vals(&[("other", Some("1"))]));
        assert!(matches!(build_statements(&cs), Err(CoreError::Config(_))));
    }

    #[test]
    fn edits_composite_pk_insert_delete_and_order() {
        let mut cs = change_set();
        cs.pk = vec!["id".into(), "name".into()];
        cs.inserts.push(vals(&[("id", Some("2")), ("name", None)]));
        cs.inserts.push(Values::new());
        cs.updates.push(RowUpdate {
            pk_values: vals(&[("id", Some("1")), ("name", Some("a"))]),
            set: Values::new(),
        });
        cs.deletes
            .push(vals(&[("id", Some("1")), ("name", Some("a"))]));
        assert_eq!(
            build_statements(&cs).unwrap(),
            [
                r#"DELETE FROM "s"."t" WHERE "id" = '1'::bigint AND "name" = 'a'::text"#,
                r#"INSERT INTO "s"."t" ("id", "name") VALUES ('2'::bigint, NULL)"#,
                r#"INSERT INTO "s"."t" DEFAULT VALUES"#,
            ]
        );
    }

    #[test]
    fn edits_reject_nul_bytes() {
        let mut cs = change_set();
        cs.updates.push(update("1", &[("name", Some("a\0b"))]));
        assert!(matches!(build_statements(&cs), Err(CoreError::Config(_))));
    }

    #[test]
    fn edits_batch_under_row_limit() {
        let stmts: Vec<String> = (0..5000).map(|i| format!("UPDATE t SET x = {i}")).collect();
        let b = batches(stmts, MAX_ROWS_PER_TX);
        assert_eq!(b.iter().map(Vec::len).collect::<Vec<_>>(), [3000, 2000]);
        assert_eq!(b[1][0], "UPDATE t SET x = 3000");
        assert!(batches(Vec::new(), MAX_ROWS_PER_TX).is_empty());
    }

    #[test]
    fn edits_batch_under_size_limit() {
        let big = "x".repeat(3 * 1024 * 1024);
        let b = batches(
            vec![big.clone(), big.clone(), big.clone(), "small".into()],
            3000,
        );
        assert_eq!(b.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2]);
        // A single statement over the limit still gets a batch of its own.
        let huge = "x".repeat(MAX_BYTES_PER_TX + 1);
        assert_eq!(batches(vec![huge, "a".into()], 3000).len(), 2);
    }
}
