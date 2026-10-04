use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementRange {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementKind {
    SelectLike,
    TxBegin,
    TxCommit,
    TxRollback,
    Other,
}

pub fn split_statements(sql: &str) -> Vec<StatementRange> {
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;
    let mut index = 0;
    let bytes = sql.as_bytes();

    while index < bytes.len() {
        let byte = bytes[index];

        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
            let next = find_line_end(bytes, index + 2);
            if start.is_some() {
                index = next;
            } else {
                index = next + usize::from(next < bytes.len());
            }
            continue;
        }

        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let next = find_block_comment_end(bytes, index + 2);
            if start.is_some() {
                index = next;
            } else {
                index = next.min(bytes.len());
            }
            continue;
        }

        let statement_start = *start.get_or_insert(index);

        if byte == b'\'' {
            let escape_backslash = index > 0
                && matches!(bytes[index - 1], b'e' | b'E')
                && (index == 1
                    || !(bytes[index - 2].is_ascii_alphanumeric() || bytes[index - 2] == b'_'));
            index = find_single_quote_end(bytes, index + 1, escape_backslash);
            continue;
        }

        if byte == b'"' {
            index = find_double_quote_end(bytes, index + 1);
            continue;
        }

        if byte == b'$'
            && let Some(tag) = dollar_tag_at(sql, index)
        {
            let needle = tag.clone();
            index += needle.len();
            if let Some(offset) = sql[index..].find(&needle) {
                index += offset + needle.len();
                continue;
            }
            index = bytes.len();
            continue;
        }

        if byte == b';' {
            push_statement(&mut ranges, sql, statement_start, index + 1);
            start = None;
        }

        index += 1;
    }

    if let Some(statement_start) = start {
        push_statement(&mut ranges, sql, statement_start, sql.len());
    }

    ranges
}

pub fn classify_statement(sql: &str) -> StatementKind {
    let words = leading_keywords(sql, 4);
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    if words.first() == Some(&"begin") || words.starts_with(&["start", "transaction"]) {
        StatementKind::TxBegin
    } else if matches!(words.first(), Some(&"commit" | &"end")) {
        StatementKind::TxCommit
    } else if words.starts_with(&["rollback", "to"])
        || words.starts_with(&["rollback", "work", "to"])
        || words.starts_with(&["rollback", "transaction", "to"])
    {
        // A savepoint rollback leaves the surrounding transaction active.
        StatementKind::Other
    } else if matches!(words.first(), Some(&"rollback" | &"abort")) {
        StatementKind::TxRollback
    } else if matches!(
        words.first(),
        Some(&"select" | &"with" | &"values" | &"table")
    ) {
        StatementKind::SelectLike
    } else {
        StatementKind::Other
    }
}

/// The UI tracks one explicit transaction on a checked-out connection. Refuse
/// compound transaction boundaries before running any part of a selected batch.
pub fn validate_transaction_sql(sql: &str) -> Result<(), CoreError> {
    let statements = split_statements(sql);
    for statement in &statements {
        let kind = classify_statement(&statement.text);
        if matches!(
            kind,
            StatementKind::TxBegin | StatementKind::TxCommit | StatementKind::TxRollback
        ) {
            if statements.len() > 1 {
                return Err(CoreError::message(
                    "Run BEGIN, COMMIT, and ROLLBACK separately from other statements so the transaction stays tracked.",
                ));
            }
            let words = leading_keywords(&statement.text, 6);
            if words
                .windows(2)
                .any(|pair| pair[0] == "and" && pair[1] == "chain")
            {
                return Err(CoreError::message(
                    "AND CHAIN is not supported. Finish this transaction, then run BEGIN separately.",
                ));
            }
        }
    }
    Ok(())
}

fn leading_keywords(mut sql: &str, limit: usize) -> Vec<String> {
    let mut words = Vec::new();
    for _ in 0..limit {
        sql = strip_leading_comments(sql);
        let end = sql
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if end == 0 {
            break;
        }
        words.push(sql[..end].to_ascii_lowercase());
        sql = &sql[end..];
    }
    words
}

pub fn auto_limit_sql(sql: &str, offset: usize, page_size: usize) -> Option<String> {
    if classify_statement(sql) != StatementKind::SelectLike || has_limit_or_fetch(sql) {
        return None;
    }

    let lower = sql.to_ascii_lowercase();
    if lower.contains(" for update") || lower.contains(" for share") || lower.contains(" into ") {
        return None;
    }

    let base = sql.trim().trim_end_matches(';').trim_end();
    Some(format!(
        "{base} LIMIT {} OFFSET {offset}",
        page_size.saturating_add(1)
    ))
}

fn has_limit_or_fetch(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    lower.contains(" limit ")
        || lower.contains("\nlimit ")
        || lower.contains("\tlimit ")
        || lower.contains(" fetch ")
        || lower.contains("\nfetch ")
        || lower.trim_end().ends_with(" limit")
}

fn strip_leading_comments(mut sql: &str) -> &str {
    loop {
        let trimmed = sql.trim_start();
        if let Some(rest) = trimmed.strip_prefix("--") {
            if let Some(offset) = rest.find('\n') {
                sql = &rest[offset + 1..];
                continue;
            }
            return "";
        }
        if trimmed.starts_with("/*") {
            sql = &trimmed[find_block_comment_end(trimmed.as_bytes(), 2)..];
            continue;
        }
        return trimmed;
    }
}

fn push_statement(ranges: &mut Vec<StatementRange>, sql: &str, start: usize, end: usize) {
    let text = sql[start..end].trim().to_string();
    if !text.is_empty() {
        ranges.push(StatementRange { start, end, text });
    }
}

fn find_line_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn find_block_comment_end(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1;
    while index + 1 < bytes.len() {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' {
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
            continue;
        }
        index += 1;
    }
    bytes.len()
}

fn find_single_quote_end(bytes: &[u8], mut index: usize, escape_backslash: bool) -> usize {
    while index < bytes.len() {
        if escape_backslash && bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'\'' {
            if bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn find_double_quote_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        if bytes[index] == b'"' {
            if bytes.get(index + 1) == Some(&b'"') {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn dollar_tag_at(sql: &str, index: usize) -> Option<String> {
    let bytes = sql.as_bytes();
    if bytes.get(index) != Some(&b'$') {
        return None;
    }

    let mut end = index + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }

    if bytes.get(end) == Some(&b'$') {
        Some(sql[index..=end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_semicolons_outside_strings_and_comments() {
        let ranges = split_statements(
            "select ';' as semi; -- ; ignored\nselect $$;$$ as dollar; /* ; */ select 3",
        );

        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].text, "select ';' as semi;");
        assert_eq!(ranges[1].text, "select $$;$$ as dollar;");
        assert_eq!(ranges[2].text, "select 3");
    }

    #[test]
    fn classifies_transactions_and_selects() {
        assert_eq!(classify_statement("begin"), StatementKind::TxBegin);
        assert_eq!(classify_statement("commit"), StatementKind::TxCommit);
        assert_eq!(classify_statement("rollback"), StatementKind::TxRollback);
        assert_eq!(
            classify_statement("with x as (select 1) select * from x"),
            StatementKind::SelectLike
        );
    }

    #[test]
    fn appends_auto_limit_when_safe() {
        assert_eq!(
            auto_limit_sql("select * from cats;", 200, 100).unwrap(),
            "select * from cats LIMIT 101 OFFSET 200"
        );
        assert!(auto_limit_sql("select * from cats limit 10", 0, 100).is_none());
    }

    #[test]
    fn savepoint_rollback_does_not_end_transaction() {
        for sql in [
            "ROLLBACK TO SAVEPOINT checkpoint",
            "rollback work to checkpoint",
            "rollback transaction /* nested /* comment */ ok */ to checkpoint",
        ] {
            assert_eq!(classify_statement(sql), StatementKind::Other);
            assert!(validate_transaction_sql(sql).is_ok());
        }
        assert_eq!(
            classify_statement("ROLLBACK WORK"),
            StatementKind::TxRollback
        );
        assert_eq!(
            classify_statement("START\n/* explanation */ TRANSACTION"),
            StatementKind::TxBegin
        );
    }

    #[test]
    fn rejects_untracked_transaction_boundaries_before_running_batches() {
        for sql in [
            "BEGIN; SELECT 1 / 0;",
            "SELECT 1; BEGIN;",
            "COMMIT; BEGIN;",
            "COMMIT AND CHAIN",
            "ROLLBACK WORK AND /* comment */ CHAIN",
        ] {
            assert!(validate_transaction_sql(sql).is_err(), "{sql}");
        }
        assert!(validate_transaction_sql("COMMIT AND NO CHAIN").is_ok());
        assert!(validate_transaction_sql("select 1; select 2;").is_ok());
        assert!(validate_transaction_sql("SAVEPOINT a; ROLLBACK TO a;").is_ok());
    }

    #[test]
    fn transaction_validation_ignores_boundaries_in_quoted_content() {
        for sql in [
            r#"SELECT 'x; COMMIT' AS "alias; BEGIN";"#,
            "SELECT E'quoted\\'; COMMIT'",
            "SELECT $$begin; commit$$",
            "/* outer /* nested */ BEGIN */ SELECT 1",
        ] {
            assert_eq!(split_statements(sql).len(), 1, "{sql}");
            assert!(validate_transaction_sql(sql).is_ok(), "{sql}");
        }
        assert_eq!(classify_statement("beginning"), StatementKind::Other);
    }
}
