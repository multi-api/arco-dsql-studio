//! Classifying SQL before it runs (spec 6.6): what kind of statement it is, and what DSQL will reject.
//!
//! A small tokenizer, not a full parser: DSQL has syntax general SQL parsers do not know (`CREATE INDEX ASYNC`,
//! `AWS IAM GRANT`), and the warnings only need leading keywords and a few patterns. Comments and string contents
//! never trigger a rule.

use serde::Serialize;

use crate::rules::rule;
use crate::split::split_statements;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StatementKind {
    Read,
    Write,
    Schema,
    Transaction,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule_id: &'static str,
    pub message: String,
    pub alternative: String,
    pub docs_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassifiedStatement {
    pub sql: String,
    pub kind: StatementKind,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Classified {
    pub statements: Vec<ClassifiedStatement>,
}

pub fn classify(sql: &str) -> Classified {
    let mut statements: Vec<ClassifiedStatement> = split_statements(sql)
        .into_iter()
        .map(|sql| {
            let toks = tokenize(&sql);
            let kind = statement_kind(&toks);
            let findings = statement_rules(&toks).into_iter().map(finding).collect();
            ClassifiedStatement {
                sql,
                kind,
                findings,
            }
        })
        .collect();
    flag_mixed_transactions(&mut statements);
    Classified { statements }
}

/// Production connections confirm anything that writes data or changes schema.
pub fn needs_confirmation(c: &Classified, production: bool) -> bool {
    production && changes_something(c)
}

/// Read-only connections refuse anything that writes data or changes schema.
pub fn blocked_by_read_only(c: &Classified) -> bool {
    changes_something(c)
}

fn changes_something(c: &Classified) -> bool {
    c.statements
        .iter()
        .any(|s| matches!(s.kind, StatementKind::Write | StatementKind::Schema))
}

fn finding(id: &'static str) -> Finding {
    let r = rule(id);
    Finding {
        rule_id: r.id,
        message: r.message.to_string(),
        alternative: r.alternative.to_string(),
        docs_url: r.docs_url.map(str::to_string),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// A keyword or bare name, uppercased.
    Word(String),
    /// A `"quoted"` identifier; never matched as a keyword.
    Ident,
    /// A string literal's contents (also dollar-quoted bodies).
    Str(String),
    Sym(char),
}

fn tokenize(sql: &str) -> Vec<Tok> {
    let chars: Vec<char> = sql.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c.is_whitespace() {
            i += 1;
        } else if c == '-' && next == Some('-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i = skip_block_comment(&chars, i);
        } else if c == '\'' || (matches!(c, 'e' | 'E') && next == Some('\'')) {
            let (text, end) = read_string(&chars, i);
            toks.push(Tok::Str(text));
            i = end;
        } else if c == '"' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '"' && chars.get(i + 1) == Some(&'"') {
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            toks.push(Tok::Ident);
        } else if let Some(end) = (c == '$').then(|| dollar_tag_end(&chars, i)).flatten() {
            let tag = &chars[i..=end];
            let body_start = end + 1;
            let close = (body_start..chars.len().saturating_sub(tag.len() - 1))
                .find(|&p| chars[p..p + tag.len()] == *tag);
            let body_end = close.unwrap_or(chars.len());
            toks.push(Tok::Str(chars[body_start..body_end].iter().collect()));
            i = close.map_or(chars.len(), |p| p + tag.len());
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            toks.push(Tok::Word(
                chars[start..i].iter().collect::<String>().to_uppercase(),
            ));
        } else {
            toks.push(Tok::Sym(c));
            i += 1;
        }
    }
    toks
}

fn skip_block_comment(chars: &[char], mut i: usize) -> usize {
    let mut depth = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            depth += 1;
            i += 2;
        } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    chars.len()
}

/// A `'...'` or `E'...'` literal starting at `i`: its contents and the index just past it.
fn read_string(chars: &[char], mut i: usize) -> (String, usize) {
    let escapes = chars[i] != '\'';
    i += if escapes { 2 } else { 1 };
    let mut text = String::new();
    while i < chars.len() {
        if escapes && chars[i] == '\\' {
            if let Some(&n) = chars.get(i + 1) {
                text.push(n);
            }
            i += 2;
        } else if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                text.push('\'');
                i += 2;
            } else {
                return (text, i + 1);
            }
        } else {
            text.push(chars[i]);
            i += 1;
        }
    }
    (text, chars.len())
}

/// For `$tag$` or `$$` at `i`, the index of the delimiter's closing `$`. `$1` is a parameter, not a tag.
fn dollar_tag_end(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return None;
    }
    let mut j = i + 1;
    if chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    (chars.get(j) == Some(&'$')).then_some(j)
}

fn word(toks: &[Tok], i: usize) -> Option<&str> {
    match toks.get(i) {
        Some(Tok::Word(w)) => Some(w.as_str()),
        _ => None,
    }
}

fn is_word(toks: &[Tok], i: usize, w: &str) -> bool {
    word(toks, i) == Some(w)
}

fn has_word(toks: &[Tok], words: &[&str]) -> bool {
    toks.iter()
        .any(|t| matches!(t, Tok::Word(w) if words.contains(&w.as_str())))
}

fn has_sequence(toks: &[Tok], words: &[&str]) -> bool {
    toks.windows(words.len()).any(|w| {
        w.iter()
            .zip(words)
            .all(|(t, expected)| matches!(t, Tok::Word(x) if x == expected))
    })
}

const WRITE_WORDS: &[&str] = &["INSERT", "UPDATE", "DELETE", "MERGE"];

fn statement_kind(toks: &[Tok]) -> StatementKind {
    let first = toks.iter().find_map(|t| match t {
        Tok::Word(w) => Some(w.as_str()),
        _ => None,
    });
    match first.unwrap_or_default() {
        "SELECT" | "TABLE" | "VALUES" | "SHOW" => StatementKind::Read,
        "WITH" if has_word(toks, WRITE_WORDS) => StatementKind::Write,
        "EXPLAIN" if has_word(toks, &["ANALYZE"]) && has_word(toks, WRITE_WORDS) => {
            StatementKind::Write
        }
        "WITH" | "EXPLAIN" => StatementKind::Read,
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "COPY" | "DO" | "CALL" => StatementKind::Write,
        "CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE" | "TRUNCATE" | "COMMENT" | "REASSIGN"
        | "SECURITY" | "REFRESH" | "AWS" => StatementKind::Schema,
        "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "ABORT" => {
            StatementKind::Transaction
        }
        _ => StatementKind::Other,
    }
}

/// `ROLLBACK TO [SAVEPOINT] x`, `ROLLBACK WORK TO x`.
fn is_rollback_to(toks: &[Tok]) -> bool {
    is_word(toks, 0, "ROLLBACK") && (1..=2).any(|i| is_word(toks, i, "TO"))
}

const TS_FUNCTIONS: &[&str] = &[
    "TO_TSVECTOR",
    "TO_TSQUERY",
    "PLAINTO_TSQUERY",
    "PHRASETO_TSQUERY",
    "WEBSEARCH_TO_TSQUERY",
    "TS_HEADLINE",
];

fn statement_rules(toks: &[Tok]) -> Vec<&'static str> {
    let mut ids = Vec::new();
    let w = |i: usize| word(toks, i).unwrap_or_default();

    match w(0) {
        "TRUNCATE" => ids.push("truncate"),
        "SAVEPOINT" | "RELEASE" => ids.push("savepoint"),
        "ROLLBACK" if is_rollback_to(toks) => ids.push("savepoint"),
        // DO blocks default to PL/pgSQL.
        "DO" if !has_word(toks, &["LANGUAGE"]) => ids.push("plpgsql"),
        "CREATE" => {
            let i = if w(1) == "OR" && w(2) == "REPLACE" {
                3
            } else {
                1
            };
            match w(i) {
                "GLOBAL" | "LOCAL"
                    if matches!(w(i + 1), "TEMP" | "TEMPORARY") && w(i + 2) == "TABLE" =>
                {
                    ids.push("temp_table")
                }
                "TEMP" | "TEMPORARY" if w(i + 1) == "TABLE" => ids.push("temp_table"),
                "TRIGGER" => ids.push("trigger"),
                "CONSTRAINT" if w(i + 1) == "TRIGGER" => ids.push("trigger"),
                "EXTENSION" => ids.push("extension"),
                "DATABASE" => ids.push("create_database"),
                "INDEX" if w(i + 1) != "ASYNC" => ids.push("index_without_async"),
                "UNIQUE" if w(i + 1) == "INDEX" && w(i + 2) != "ASYNC" => {
                    ids.push("index_without_async")
                }
                "TEXT" if w(i + 1) == "SEARCH" => ids.push("fts_config"),
                _ => {}
            }
        }
        "ALTER" if w(1) == "TABLE" => ids.extend(alter_table_rules(toks)),
        _ => {}
    }

    if mentions_plpgsql(toks) {
        ids.push("plpgsql");
    }
    if has_sequence(toks, &["FOR", "SHARE"]) || has_sequence(toks, &["FOR", "KEY", "SHARE"]) {
        ids.push("for_share");
    }
    for (i, t) in toks.iter().enumerate() {
        let Tok::Word(name) = t else { continue };
        if toks.get(i + 1) != Some(&Tok::Sym('(')) {
            continue;
        }
        if name.starts_with("PG_ADVISORY") || name.starts_with("PG_TRY_ADVISORY") {
            ids.push("advisory_lock");
        }
        if TS_FUNCTIONS.contains(&name.as_str())
            && let (Some(Tok::Str(config)), Some(Tok::Sym(','))) =
                (toks.get(i + 2), toks.get(i + 3))
            && !config
                .trim_start_matches("pg_catalog.")
                .eq_ignore_ascii_case("simple")
        {
            ids.push("fts_config");
        }
    }
    let mut unique = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// `LANGUAGE plpgsql` or `LANGUAGE 'plpgsql'`.
fn mentions_plpgsql(toks: &[Tok]) -> bool {
    toks.windows(2).any(|p| match p {
        [Tok::Word(l), Tok::Word(w)] => l == "LANGUAGE" && w == "PLPGSQL",
        [Tok::Word(l), Tok::Str(s)] => l == "LANGUAGE" && s.eq_ignore_ascii_case("plpgsql"),
        _ => false,
    })
}

/// Rules for each comma-separated action of `ALTER TABLE ...`.
fn alter_table_rules(toks: &[Tok]) -> Vec<&'static str> {
    let mut actions: Vec<&[Tok]> = Vec::new();
    let mut depth = 0;
    let mut start = 3.min(toks.len());
    for (i, t) in toks.iter().enumerate().skip(start) {
        match t {
            Tok::Sym('(') => depth += 1,
            Tok::Sym(')') => depth -= 1,
            Tok::Sym(',') if depth == 0 => {
                actions.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    actions.push(&toks[start..]);

    let mut ids = Vec::new();
    for action in actions {
        let Some(at) = action
            .iter()
            .position(|t| matches!(t, Tok::Word(w) if w == "ADD" || w == "ALTER"))
        else {
            continue;
        };
        let a = &action[at..];
        if is_word(a, 0, "ADD") {
            let i = if is_word(a, 1, "CONSTRAINT") { 3 } else { 1 };
            if matches!(
                word(a, i),
                Some("PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK" | "EXCLUDE")
            ) {
                ids.push("add_constraint");
            } else if has_word(a, &["DEFAULT", "REFERENCES"]) || has_sequence(a, &["NOT", "NULL"]) {
                ids.push("add_column_default_or_not_null");
            }
        } else if has_word(a, &["TYPE"]) {
            ids.push("alter_column_type");
        } else if has_sequence(a, &["SET", "NOT", "NULL"]) {
            ids.push("set_not_null");
        }
    }
    ids
}

/// Inside an explicit transaction, DSQL allows one DDL statement and no data changes next to it.
fn flag_mixed_transactions(statements: &mut [ClassifiedStatement]) {
    let mut in_tx = false;
    let (mut ddl, mut writes, mut flagged) = (0, 0, false);
    for s in statements.iter_mut() {
        let toks = tokenize(&s.sql);
        let first = word(&toks, 0).unwrap_or_default();
        match first {
            "BEGIN" | "START" => {
                in_tx = true;
                (ddl, writes, flagged) = (0, 0, false);
                continue;
            }
            "COMMIT" | "END" | "ABORT" => {
                in_tx = false;
                continue;
            }
            "ROLLBACK" if !is_rollback_to(&toks) => {
                in_tx = false;
                continue;
            }
            _ => {}
        }
        if !in_tx {
            continue;
        }
        if matches!(first, "CREATE" | "ALTER" | "DROP") {
            ddl += 1;
        } else if s.kind == StatementKind::Write {
            writes += 1;
        }
        if !flagged && (ddl > 1 || (ddl >= 1 && writes >= 1)) {
            s.findings.push(finding("multiple_ddl_in_transaction"));
            flagged = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<StatementKind> {
        classify(sql).statements.iter().map(|s| s.kind).collect()
    }

    fn rule_ids(sql: &str) -> Vec<&'static str> {
        classify(sql)
            .statements
            .iter()
            .flat_map(|s| s.findings.iter().map(|f| f.rule_id))
            .collect()
    }

    fn fires(id: &str, sql: &str) {
        assert!(
            rule_ids(sql).contains(&id),
            "{id} should fire for {sql:?}, got {:?}",
            rule_ids(sql)
        );
    }

    fn silent(sql: &str) {
        assert!(
            rule_ids(sql).is_empty(),
            "no rule should fire for {sql:?}, got {:?}",
            rule_ids(sql)
        );
    }

    #[test]
    fn select_is_read() {
        assert_eq!(kinds("select * from users"), [StatementKind::Read]);
        assert_eq!(
            kinds("with t as (select 1) select * from t"),
            [StatementKind::Read]
        );
        assert_eq!(kinds("(select 1) union (select 2)"), [StatementKind::Read]);
        assert_eq!(
            kinds("explain select 1; show search_path; values (1)"),
            [StatementKind::Read; 3]
        );
    }

    #[test]
    fn insert_is_write() {
        assert_eq!(kinds("insert into t values (1)"), [StatementKind::Write]);
        assert_eq!(
            kinds(
                "update t set a = 1; delete from t; merge into t using s on true when matched then delete"
            ),
            [StatementKind::Write; 3]
        );
        assert_eq!(
            kinds("with d as (delete from t returning *) select * from d"),
            [StatementKind::Write]
        );
        assert_eq!(
            kinds("explain analyze delete from t"),
            [StatementKind::Write]
        );
        assert_eq!(kinds("copy t from stdin"), [StatementKind::Write]);
    }

    #[test]
    fn grant_is_schema() {
        assert_eq!(kinds("grant select on t to r"), [StatementKind::Schema]);
        assert_eq!(
            kinds(
                "revoke select on t from r; create table t (id int primary key); drop role r; aws iam grant r to 'arn'"
            ),
            [StatementKind::Schema; 4]
        );
        assert_eq!(kinds("truncate t"), [StatementKind::Schema]);
    }

    #[test]
    fn transaction_and_other_kinds() {
        assert_eq!(
            kinds("begin; commit; rollback; start transaction"),
            [StatementKind::Transaction; 4]
        );
        assert_eq!(kinds("set search_path = x"), [StatementKind::Other]);
    }

    #[test]
    fn comment_and_string_with_semicolon_not_split() {
        let c = classify("select ';' -- ;");
        assert_eq!(c.statements.len(), 1);
        assert_eq!(c.statements[0].kind, StatementKind::Read);
    }

    #[test]
    fn strings_and_comments_never_trigger_rules() {
        silent("select 'truncate table t' as note");
        silent("select 1 -- for share\n");
        silent("select /* savepoint s */ 1");
        silent("insert into notes values ('create extension x; drop database y')");
        silent("select \"truncate\" from t");
    }

    #[test]
    fn rule_truncate() {
        fires("truncate", "truncate table users");
        fires("truncate", "TRUNCATE users, orders");
    }

    #[test]
    fn rule_add_constraint() {
        fires("add_constraint", "alter table t add primary key (id)");
        fires(
            "add_constraint",
            "alter table t add constraint t_fk foreign key (u) references u (id)",
        );
        fires("add_constraint", "ALTER TABLE t ADD UNIQUE (email)");
        fires(
            "add_constraint",
            "alter table t add constraint positive check (n > 0)",
        );
        silent("alter table t add column note text");
    }

    #[test]
    fn rule_add_column_default_or_not_null() {
        fires(
            "add_column_default_or_not_null",
            "alter table t add column active boolean default true",
        );
        fires(
            "add_column_default_or_not_null",
            "alter table t add active boolean not null",
        );
        fires(
            "add_column_default_or_not_null",
            "alter table t add column user_id bigint references users (id)",
        );
        silent("alter table t add column note text");
        silent("alter table t add column note text null");
    }

    #[test]
    fn rule_alter_column_type() {
        fires(
            "alter_column_type",
            "alter table t alter column n type bigint",
        );
        fires(
            "alter_column_type",
            "alter table t alter n set data type bigint",
        );
    }

    #[test]
    fn rule_set_not_null() {
        fires("set_not_null", "alter table t alter column n set not null");
        silent("alter table t alter column n drop not null");
    }

    #[test]
    fn rule_savepoint() {
        fires("savepoint", "savepoint s1");
        fires("savepoint", "rollback to savepoint s1");
        fires("savepoint", "rollback to s1");
        fires("savepoint", "release savepoint s1");
        silent("rollback");
    }

    #[test]
    fn rule_temp_table() {
        fires("temp_table", "create temp table t (id int)");
        fires("temp_table", "create temporary table t as select 1");
        fires("temp_table", "create global temporary table t (id int)");
        silent("create table temp (id int primary key)");
    }

    #[test]
    fn rule_trigger() {
        fires(
            "trigger",
            "create trigger t before insert on x for each row execute function f()",
        );
        fires(
            "trigger",
            "create or replace trigger t after update on x execute function f()",
        );
    }

    #[test]
    fn rule_plpgsql() {
        fires(
            "plpgsql",
            "create function f() returns int language plpgsql as $$ begin return 1; end $$",
        );
        fires(
            "plpgsql",
            "create function f() returns int as 'select 1' language 'plpgsql'",
        );
        fires("plpgsql", "do $$ begin perform 1; end $$");
        silent("create function f() returns int language sql as $$ select 1 $$");
    }

    #[test]
    fn rule_extension() {
        fires("extension", "create extension if not exists pgcrypto");
    }

    #[test]
    fn rule_advisory_lock() {
        fires("advisory_lock", "select pg_advisory_lock(42)");
        fires("advisory_lock", "select pg_try_advisory_xact_lock(1, 2)");
    }

    #[test]
    fn rule_for_share() {
        fires("for_share", "select * from t where id = 1 for share");
        fires("for_share", "select * from t for key share");
        silent("select * from t for update");
    }

    #[test]
    fn rule_fts_config() {
        fires(
            "fts_config",
            "select to_tsvector('english', body) from posts",
        );
        fires(
            "fts_config",
            "select * from p where to_tsvector(body) @@ plainto_tsquery('spanish', 'hola')",
        );
        silent("select to_tsvector('simple', body) from posts");
        silent("select to_tsvector(body) from posts");
    }

    #[test]
    fn rule_create_database() {
        fires("create_database", "create database reports");
    }

    #[test]
    fn rule_index_without_async() {
        fires("index_without_async", "create index i on t (a)");
        fires(
            "index_without_async",
            "create unique index if not exists i on t (a)",
        );
        silent("create index async i on t (a)");
        silent("CREATE UNIQUE INDEX ASYNC i ON t (a)");
    }

    #[test]
    fn rule_multiple_ddl_in_transaction() {
        fires(
            "multiple_ddl_in_transaction",
            "begin; create table a (id int primary key); create table b (id int primary key); commit",
        );
        fires(
            "multiple_ddl_in_transaction",
            "begin; create table a (id int primary key); insert into a values (1); commit",
        );
        silent("create table a (id int primary key); create table b (id int primary key)");
        silent(
            "begin; create table a (id int primary key); commit; begin; create table b (id int primary key); commit",
        );
        silent("begin; insert into a values (1); insert into a values (2); commit");
    }

    #[test]
    fn findings_carry_rule_texts() {
        let c = classify("truncate t");
        let f = &c.statements[0].findings[0];
        assert_eq!(f.rule_id, "truncate");
        assert!(f.alternative.contains("DELETE FROM"));
        assert!(f.docs_url.is_some());
    }

    #[test]
    fn production_write_needs_confirmation() {
        assert!(needs_confirmation(&classify("update t set a = 1"), true));
        assert!(needs_confirmation(
            &classify("select 1; drop table t"),
            true
        ));
        assert!(!needs_confirmation(&classify("update t set a = 1"), false));
        assert!(!needs_confirmation(
            &classify("select 1; begin; commit"),
            true
        ));
    }

    #[test]
    fn read_only_blocks_delete() {
        assert!(blocked_by_read_only(&classify("delete from t")));
        assert!(blocked_by_read_only(&classify(
            "select 1; grant select on t to r"
        )));
        assert!(!blocked_by_read_only(&classify(
            "select 1; set search_path = x; begin; rollback"
        )));
    }
}
