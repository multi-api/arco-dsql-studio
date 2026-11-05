//! Quoting identifiers and literals for SQL the app builds itself.

/// Always double-quoted, with `"` doubled: `we"ird` → `"we""ird"`, `Order` → `"Order"`.
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
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
}
