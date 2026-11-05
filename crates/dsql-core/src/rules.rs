//! What DSQL rejects, in one place: each rule's id, the message shown before running, the DSQL alternative and a
//! docs link. Detection lives in `classify.rs`, keyed by id. Sources: the `multiapi/laravel-aurora-dsql` README, the
//! AWS docs, and the Task 1 spike (`docs/spike-findings.md`).

pub struct Rule {
    pub id: &'static str,
    pub message: &'static str,
    pub alternative: &'static str,
    pub docs_url: Option<&'static str>,
}

const UNSUPPORTED: &str = "https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-postgresql-compatibility-unsupported-features.html";
const INDEX_ASYNC: &str =
    "https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-create-index-async.html";
const LARAVEL_DRIVER: &str =
    "https://github.com/multi-api/laravel-aurora-dsql#unsupported-features";

pub const RULES: &[Rule] = &[
    Rule {
        id: "truncate",
        message: "DSQL does not support TRUNCATE.",
        alternative: "Use DELETE FROM <table>. DSQL changes at most 3,000 rows per transaction, so delete in batches.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "add_constraint",
        message: "DSQL cannot add a PRIMARY KEY, FOREIGN KEY, UNIQUE or CHECK constraint to an existing table.",
        alternative: "Declare constraints in CREATE TABLE. For uniqueness, use CREATE UNIQUE INDEX ASYNC.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "add_column_default_or_not_null",
        message: "DSQL cannot add a column with a DEFAULT, NOT NULL or a foreign key.",
        alternative: "Add the column as nullable without a default, then backfill it with UPDATE in batches.",
        docs_url: Some(LARAVEL_DRIVER),
    },
    Rule {
        id: "alter_column_type",
        message: "DSQL cannot change a column's type.",
        alternative: "Create a new table with the new type, copy the rows, then swap the names.",
        docs_url: Some(LARAVEL_DRIVER),
    },
    Rule {
        id: "set_not_null",
        message: "DSQL cannot make an existing column NOT NULL.",
        alternative: "Create a new table with the column NOT NULL, copy the rows, then swap the names.",
        docs_url: Some(LARAVEL_DRIVER),
    },
    Rule {
        id: "savepoint",
        message: "DSQL does not support SAVEPOINT, ROLLBACK TO or RELEASE.",
        alternative: "Use a single transaction and roll back the whole transaction instead.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "temp_table",
        message: "DSQL does not support temporary tables.",
        alternative: "Use a regular table and drop it when you are done, or a CTE (WITH ...) inside one query.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "trigger",
        message: "DSQL does not support triggers.",
        alternative: "Do the extra work in the application, in the same transaction as the change.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "plpgsql",
        message: "DSQL does not support PL/pgSQL functions or DO blocks.",
        alternative: "Use a LANGUAGE sql function, or run the steps as separate statements.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "extension",
        message: "DSQL does not support CREATE EXTENSION.",
        alternative: "Check the DSQL docs for the built-in functions you need.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "advisory_lock",
        message: "DSQL does not support advisory locks.",
        alternative: "DSQL uses optimistic concurrency: retry the transaction when it fails with 40001.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "for_share",
        message: "DSQL does not support SELECT ... FOR SHARE.",
        alternative: "Use FOR UPDATE, or rely on DSQL's optimistic concurrency and retry on 40001.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "fts_config",
        message: "DSQL supports only the 'simple' text search configuration.",
        alternative: "Pass 'simple' as the configuration, for example to_tsvector('simple', body).",
        docs_url: Some(LARAVEL_DRIVER),
    },
    Rule {
        id: "create_database",
        message: "DSQL has a single database, postgres; CREATE DATABASE is not supported.",
        alternative: "Use a schema instead: CREATE SCHEMA <name>.",
        docs_url: Some(UNSUPPORTED),
    },
    Rule {
        id: "index_without_async",
        message: "DSQL rejects CREATE INDEX without ASYNC, even on an empty table.",
        alternative: "Use CREATE INDEX ASYNC. It returns a job id; the index is usable when the job finishes.",
        docs_url: Some(INDEX_ASYNC),
    },
    Rule {
        id: "multiple_ddl_in_transaction",
        message: "A DSQL transaction can hold one DDL statement, and no data changes next to it.",
        alternative: "Run each CREATE, ALTER or DROP in its own transaction, and data changes in separate ones.",
        docs_url: Some(UNSUPPORTED),
    },
];

pub fn rule(id: &str) -> &'static Rule {
    RULES
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("unknown rule {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_are_unique_and_texts_present() {
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len());
        for r in RULES {
            assert!(
                !r.message.is_empty() && !r.alternative.is_empty(),
                "{}",
                r.id
            );
        }
    }
}
