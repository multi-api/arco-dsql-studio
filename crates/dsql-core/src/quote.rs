//! Quoting identifiers and literals for SQL the app builds itself.

use crate::error::CoreError;

/// Always double-quoted, with `"` doubled: `we"ird` → `"we""ird"`, `Order` → `"Order"`.
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `None` is `NULL`; text is single-quoted with `'` doubled, so `Some("NULL")` stays the string `'NULL'`. Sessions run
/// with `standard_conforming_strings` on (the default), so backslashes need no escaping. Postgres text cannot hold a
/// NUL byte, so one is an error instead of a silently truncated value.
pub fn literal(v: Option<&str>) -> Result<String, CoreError> {
    match v {
        None => Ok("NULL".to_string()),
        Some(s) if s.contains('\0') => Err(CoreError::Config(
            "the value contains a NUL byte, which Postgres text cannot store".to_string(),
        )),
        Some(s) => Ok(format!("'{}'", s.replace('\'', "''"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_quotes_and_doubles() {
        assert_eq!(ident("users"), "\"users\"");
        assert_eq!(ident("Order"), "\"Order\"");
        assert_eq!(ident("My Schema"), "\"My Schema\"");
        assert_eq!(ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn literal_null_empty_and_text() {
        assert_eq!(literal(None).unwrap(), "NULL");
        assert_eq!(literal(Some("")).unwrap(), "''");
        assert_eq!(literal(Some("NULL")).unwrap(), "'NULL'");
        assert_eq!(literal(Some(r"back\slash")).unwrap(), r"'back\slash'");
    }

    #[test]
    fn literal_doubles_quotes() {
        assert_eq!(literal(Some("O'Brien")).unwrap(), "'O''Brien'");
        assert_eq!(
            literal(Some("'); drop table t; --")).unwrap(),
            "'''); drop table t; --'"
        );
    }

    #[test]
    fn literal_rejects_nul() {
        assert!(matches!(literal(Some("a\0b")), Err(CoreError::Config(_))));
    }
}
