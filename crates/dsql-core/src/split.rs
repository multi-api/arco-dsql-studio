//! Splitting a SQL script into statements on `;`, outside quotes, comments and dollar-quoted bodies.
//!
//! Scripts run one statement at a time: Postgres runs a multi-statement simple query in one implicit transaction, and
//! DSQL allows a single DDL statement per transaction, so `CREATE TABLE a ...; CREATE TABLE b ...;` must be two calls.

/// The statements of `sql`, trimmed, without their trailing `;`. Empty statements and comment-only pieces are
/// dropped; comments inside a statement are kept.
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut has_code = false;
    let mut i = 0;

    let mut finish = |from: usize, to: usize, has_code: bool| {
        if has_code {
            statements.push(
                chars[from..to]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string(),
            );
        }
    };

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '-' if next == Some('-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if next == Some('*') => {
                i = skip_block_comment(&chars, i);
                continue;
            }
            '\'' => {
                let escapes = i > 0
                    && matches!(chars[i - 1], 'e' | 'E')
                    && (i < 2 || !is_ident_char(chars[i - 2]));
                i = skip_quoted(&chars, i, '\'', escapes);
                has_code = true;
                continue;
            }
            '"' => {
                i = skip_quoted(&chars, i, '"', false);
                has_code = true;
                continue;
            }
            '$' if i == 0 || !is_ident_char(chars[i - 1]) => {
                if let Some(tag_end) = dollar_tag_end(&chars, i) {
                    let tag: Vec<char> = chars[i..=tag_end].to_vec();
                    i = find_sequence(&chars, tag_end + 1, &tag)
                        .map_or(chars.len(), |p| p + tag.len());
                    has_code = true;
                    continue;
                }
                has_code = true;
            }
            ';' => {
                finish(start, i, has_code);
                start = i + 1;
                has_code = false;
            }
            c if !c.is_whitespace() => has_code = true,
            _ => {}
        }
        i += 1;
    }
    finish(start, chars.len(), has_code);
    statements
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Index just past a `/* ... */` comment starting at `i`; Postgres block comments nest.
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

/// Index just past a quoted string or identifier starting at `i`. A doubled quote is part of the text; with
/// `escapes` (an `E'...'` string) a backslash escapes the next character. Unterminated runs to the end.
fn skip_quoted(chars: &[char], mut i: usize, quote: char, escapes: bool) -> usize {
    i += 1;
    while i < chars.len() {
        if escapes && chars[i] == '\\' {
            i += 2;
        } else if chars[i] == quote {
            if chars.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                return i + 1;
            }
        } else {
            i += 1;
        }
    }
    chars.len()
}

/// For `$tag$` or `$$` starting at `i`, the index of the closing `$` of the opening delimiter. `$1` is not a tag.
fn dollar_tag_end(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    while j < chars.len() && is_ident_char(chars[j]) {
        j += 1;
    }
    (chars.get(j) == Some(&'$')).then_some(j)
}

fn find_sequence(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|&p| chars[p..p + needle.len()] == *needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_semicolons() {
        assert_eq!(
            split_statements("select 1; select 2;\n select 3"),
            ["select 1", "select 2", "select 3"]
        );
    }

    #[test]
    fn comment_and_string_with_semicolon_not_split() {
        assert_eq!(split_statements("select ';' -- ;"), ["select ';' -- ;"]);
        assert_eq!(
            split_statements("select 'it''s; fine'; select 2"),
            ["select 'it''s; fine'", "select 2"]
        );
        assert_eq!(
            split_statements("select \"a;b\" from t; select 2"),
            ["select \"a;b\" from t", "select 2"]
        );
        assert_eq!(
            split_statements("select /* ; /* nested ; */ ; */ 1; select 2"),
            ["select /* ; /* nested ; */ ; */ 1", "select 2"]
        );
        assert_eq!(
            split_statements("select E'a\\';b'; select 2"),
            ["select E'a\\';b'", "select 2"]
        );
    }

    #[test]
    fn dollar_quoted_bodies_not_split() {
        let sql =
            "create function f() returns int as $body$ select 1; $body$ language sql; select 2";
        assert_eq!(
            split_statements(sql),
            [
                "create function f() returns int as $body$ select 1; $body$ language sql",
                "select 2"
            ]
        );
        assert_eq!(
            split_statements("select $$a;b$$; select $1"),
            ["select $$a;b$$", "select $1"]
        );
    }

    #[test]
    fn drops_empty_and_comment_only_pieces() {
        assert_eq!(
            split_statements(" ; ;\n-- just a comment\n;select 1;;"),
            ["select 1"]
        );
        assert_eq!(split_statements("/* header */"), Vec::<String>::new());
        assert_eq!(split_statements(""), Vec::<String>::new());
    }

    #[test]
    fn unterminated_quote_keeps_the_rest_as_one_statement() {
        assert_eq!(
            split_statements("select 'oops; select 2"),
            ["select 'oops; select 2"]
        );
    }
}
