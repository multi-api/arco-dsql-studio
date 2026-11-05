//! Running SQL typed by the user: one statement at a time with the simple query protocol, every value as text.

use std::time::Instant;

use futures_util::{StreamExt, pin_mut};
use serde::Serialize;
use tokio_postgres::SimpleQueryMessage;

use crate::conn::{Session, db_error};
use crate::error::CoreError;
use crate::split::split_statements;

/// Results are capped so a `select *` on a big table cannot exhaust memory; the UI shows "results truncated".
pub const MAX_ROWS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultSet {
    pub columns: Vec<String>,
    /// `None` is SQL NULL; `Some("")` an empty string; `Some("NULL")` the text NULL.
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum StatementOutcome {
    Rows(ResultSet),
    Command { tag: String, rows_affected: u64 },
    Error(CoreError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptResult {
    /// One per statement that ran; the last one is an `Error` when the script stopped early.
    pub outcomes: Vec<StatementOutcome>,
    pub duration_ms: u64,
}

/// A cell as the simple query protocol returns it: SQL NULL stays `None`, text is kept as is.
pub fn cell(value: Option<&str>) -> Option<String> {
    value.map(str::to_string)
}

/// Collects rows up to `max`, remembering whether more arrived.
#[derive(Debug, Default)]
pub struct RowCollector {
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
    max: usize,
}

impl RowCollector {
    pub fn new(max: usize) -> RowCollector {
        RowCollector {
            rows: Vec::new(),
            truncated: false,
            max,
        }
    }

    /// Returns false once the cap is reached, so the caller can stop reading.
    pub fn push(&mut self, row: Vec<Option<String>>) -> bool {
        if self.rows.len() >= self.max {
            self.truncated = true;
            return false;
        }
        self.rows.push(row);
        true
    }
}

/// A short tag for a statement that returned no rows, from its first keywords: `INSERT`, `CREATE TABLE`, `GRANT`.
pub fn command_tag(statement: &str) -> String {
    let words: Vec<String> = strip_leading_comments(statement)
        .split_whitespace()
        .take(6)
        .map(|w| w.trim_end_matches(';').to_ascii_uppercase())
        .collect();
    let Some(first) = words.first() else {
        return String::new();
    };
    let taken = match first.as_str() {
        "AWS" => 3,
        "CREATE" | "ALTER" | "DROP" => {
            const MODIFIERS: &[&str] = &[
                "OR",
                "REPLACE",
                "UNIQUE",
                "TEMP",
                "TEMPORARY",
                "UNLOGGED",
                "MATERIALIZED",
                "GLOBAL",
                "LOCAL",
            ];
            1 + words[1..]
                .iter()
                .take_while(|w| MODIFIERS.contains(&w.as_str()))
                .count()
                + 1
        }
        _ => 1,
    };
    words.into_iter().take(taken).collect::<Vec<_>>().join(" ")
}

fn strip_leading_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("--") {
            s = rest.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(rest) = s.strip_prefix("/*") {
            s = rest.split_once("*/").map_or("", |(_, after)| after);
        } else {
            return s;
        }
    }
}

/// Runs `sql` one statement at a time, stopping at the first error. Rows are capped at `max_rows` per statement; the
/// rest of a capped result is not read.
pub async fn run_script(s: &Session, sql: &str, max_rows: usize) -> ScriptResult {
    let started = Instant::now();
    let mut outcomes = Vec::new();
    for statement in split_statements(sql) {
        let outcome = run_statement(s, &statement, max_rows).await;
        let failed = matches!(outcome, StatementOutcome::Error(_));
        outcomes.push(outcome);
        if failed {
            break;
        }
    }
    ScriptResult {
        outcomes,
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

async fn run_statement(s: &Session, statement: &str, max_rows: usize) -> StatementOutcome {
    let stream = match s.client.simple_query_raw(statement).await {
        Ok(stream) => stream,
        Err(e) => return StatementOutcome::Error(db_error(e)),
    };
    pin_mut!(stream);
    let mut columns: Option<Vec<String>> = None;
    let mut rows = RowCollector::new(max_rows);
    while let Some(message) = stream.next().await {
        match message {
            Ok(SimpleQueryMessage::RowDescription(cols)) => {
                columns = Some(cols.iter().map(|c| c.name().to_string()).collect());
            }
            Ok(SimpleQueryMessage::Row(row)) => {
                if !rows.push((0..row.len()).map(|i| cell(row.get(i))).collect()) {
                    break;
                }
            }
            Ok(SimpleQueryMessage::CommandComplete(n)) => {
                return match columns {
                    Some(columns) => StatementOutcome::Rows(ResultSet {
                        columns,
                        rows: rows.rows,
                        truncated: false,
                    }),
                    None => StatementOutcome::Command {
                        tag: command_tag(statement),
                        rows_affected: n,
                    },
                };
            }
            Ok(_) => {}
            Err(e) => return StatementOutcome::Error(db_error(e)),
        }
    }
    StatementOutcome::Rows(ResultSet {
        columns: columns.unwrap_or_default(),
        truncated: rows.truncated,
        rows: rows.rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_query_preserves_null() {
        assert_eq!(cell(None), None);
        assert_eq!(cell(Some("")), Some(String::new()));
        assert_eq!(cell(Some("NULL")), Some("NULL".to_string()));
    }

    #[test]
    fn run_script_caps_rows() {
        let mut rows = RowCollector::new(10_000);
        let mut accepted = 0;
        for i in 0..10_001 {
            if rows.push(vec![Some(i.to_string())]) {
                accepted += 1;
            } else {
                break;
            }
        }
        assert_eq!(accepted, 10_000);
        assert_eq!(rows.rows.len(), 10_000);
        assert!(rows.truncated);
    }

    #[test]
    fn exactly_max_rows_is_not_truncated() {
        let mut rows = RowCollector::new(3);
        for i in 0..3 {
            assert!(rows.push(vec![Some(i.to_string())]));
        }
        assert!(!rows.truncated);
    }

    #[test]
    fn command_tags() {
        assert_eq!(command_tag("insert into t values (1)"), "INSERT");
        assert_eq!(command_tag("  -- note\n  Update t set a = 1"), "UPDATE");
        assert_eq!(
            command_tag("create table t (id int primary key)"),
            "CREATE TABLE"
        );
        assert_eq!(
            command_tag("CREATE UNIQUE INDEX ASYNC i on t (a)"),
            "CREATE UNIQUE INDEX"
        );
        assert_eq!(command_tag("drop role readonly"), "DROP ROLE");
        assert_eq!(command_tag("aws iam grant r to 'arn'"), "AWS IAM GRANT");
        assert_eq!(command_tag("/* x */ begin"), "BEGIN");
        assert_eq!(command_tag(""), "");
    }
}
