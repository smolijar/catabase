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

/// Editor-only boundaries for independent queries and transaction commands
/// separated by blank lines.
/// SQL execution and transaction validation still use semicolons exclusively.
/// A WITH clause stays attached to its first top-level query, and set-operation
/// operands, nested queries, strings and comments never introduce a boundary.
pub fn split_editor_statements(sql: &str) -> Vec<StatementRange> {
    let bytes = sql.as_bytes();
    let mut ranges = Vec::new();

    for statement in split_statements(sql) {
        let kind = classify_statement(&statement.text);
        if !matches!(
            kind,
            StatementKind::SelectLike
                | StatementKind::TxBegin
                | StatementKind::TxCommit
                | StatementKind::TxRollback
        ) {
            ranges.push(statement);
            continue;
        }
        let mut start = statement.start;
        let mut index = start;
        let mut depth = 0usize;
        let mut line_breaks = 0;
        let mut query_body_seen = false;
        let mut transaction_body_seen = kind != StatementKind::SelectLike;
        let mut set_operand_pending = false;
        let mut case_depth = 0usize;

        while index < statement.end {
            let byte = bytes[index];
            if byte.is_ascii_whitespace() {
                line_breaks += usize::from(byte == b'\n');
                index += 1;
                continue;
            }
            if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
                index = find_line_end(bytes, index + 2);
                continue;
            }
            if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                index = find_block_comment_end(bytes, index + 2);
                continue;
            }

            if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 128 {
                let word_start = index;
                while index < statement.end
                    && (bytes[index].is_ascii_alphanumeric()
                        || matches!(bytes[index], b'_' | b'$')
                        || bytes[index] >= 128)
                {
                    index += 1;
                }
                if depth == 0 {
                    let word = sql[word_start..index].to_ascii_lowercase();
                    let starts_query =
                        matches!(word.as_str(), "select" | "with" | "values" | "table");
                    let starts_transaction = matches!(
                        word.as_str(),
                        "begin" | "commit" | "end" | "rollback" | "abort"
                    ) || (word == "start"
                        && classify_statement(&sql[word_start..]) == StatementKind::TxBegin);
                    if (starts_query || starts_transaction)
                        && (query_body_seen || transaction_body_seen)
                        && !set_operand_pending
                        && case_depth == 0
                        && line_breaks >= 2
                    {
                        let end = start + sql[start..word_start].trim_end().len();
                        push_statement(&mut ranges, sql, start, end);
                        start = word_start;
                        query_body_seen = false;
                        transaction_body_seen = starts_transaction;
                    }
                    // END in a multiline CASE expression is not COMMIT's alias.
                    if word == "case" {
                        case_depth += 1;
                    } else if word == "end" {
                        case_depth = case_depth.saturating_sub(1);
                    }
                    if matches!(word.as_str(), "select" | "values" | "table") {
                        query_body_seen = true;
                        set_operand_pending = false;
                    } else if matches!(word.as_str(), "union" | "intersect" | "except") {
                        set_operand_pending = true;
                    }
                }
            } else if byte == b'\'' {
                let escape_backslash = index > 0
                    && matches!(bytes[index - 1], b'e' | b'E')
                    && (index == 1
                        || !(bytes[index - 2].is_ascii_alphanumeric() || bytes[index - 2] == b'_'));
                index = find_single_quote_end(bytes, index + 1, escape_backslash);
            } else if byte == b'"' {
                index = find_double_quote_end(bytes, index + 1);
            } else if byte == b'$'
                && let Some(tag) = dollar_tag_at(sql, index)
            {
                index += tag.len();
                index = sql[index..]
                    .find(&tag)
                    .map_or(bytes.len(), |offset| index + offset + tag.len());
            } else {
                if byte == b'(' {
                    if depth == 0 {
                        set_operand_pending = false;
                    }
                    depth += 1;
                } else if byte == b')' {
                    depth = depth.saturating_sub(1);
                }
                index += 1;
            }
            line_breaks = 0;
        }
        push_statement(&mut ranges, sql, start, statement.end);
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
    fn editor_splits_independent_select_after_ledger_cte() {
        let sql = r#"with foo as (
    select x.notes, x."chargeCode", x.description, x."chargeAmountCents" / 100 as charge_amount, "transType" as trans_type, "transDate" as date, tenant_id
from
	pms.property_residents_ledger,
	jsonb_to_recordset(transactions) as x(
		notes text,
		"chargeCode" text,
		description text,
		"transType" text,
		"transDate" date,
		"chargeAmountCents" numeric
	)
)

select * from foo


select 1"#;
        let ranges = split_editor_statements(sql);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].text, sql[..sql.find("\n\n\nselect 1").unwrap()]);
        assert_eq!(ranges[1].text, "select 1");
        assert_eq!(ranges[1].start, sql.rfind("select 1").unwrap());
        assert_eq!(ranges[1].end, sql.len());
        assert!(ranges[0].end < ranges[1].start);
        // Editor conveniences must not change batch execution/transaction parsing.
        assert_eq!(split_statements(sql).len(), 1);
    }

    #[test]
    fn editor_splits_begin_between_ledger_cte_and_query() {
        let sql = r#"with foo as (
    select x.notes, x."chargeCode", x.description, x."chargeAmountCents" / 100 as charge_amount, "transType" as trans_type, "transDate" as date, tenant_id
from
	pms.property_residents_ledger,
	jsonb_to_recordset(transactions) as x(
		notes text,
		"chargeCode" text,
		description text,
		"transType" text,
		"transDate" date,
		"chargeAmountCents" numeric
	)
)

select * from foo
order by "chargeCode"


begin


SELECT name FROM LANDLORDS"#;
        let ranges = split_editor_statements(sql);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].text, sql[..sql.find("\n\n\nbegin").unwrap()]);
        assert_eq!(ranges[1].text, "begin");
        assert_eq!(ranges[1].start, sql.find("begin").unwrap());
        assert_eq!(ranges[1].end, ranges[1].start + "begin".len());
        assert_eq!(ranges[2].text, "SELECT name FROM LANDLORDS");
        assert_eq!(classify_statement(&ranges[1].text), StatementKind::TxBegin);
        assert!(validate_transaction_sql(&ranges[1].text).is_ok());
        // An explicit selection remains one unmodified batch, not editor blocks.
        assert_eq!(split_statements(sql).len(), 1);
    }

    #[test]
    fn editor_splits_transaction_commands_before_and_after_queries() {
        for command in [
            "BEGIN",
            "begin transaction isolation level serializable",
            "START /* explanation */ TRANSACTION READ ONLY",
            "COMMIT",
            "END",
            "ROLLBACK",
            "ABORT",
            "ROLLBACK TO SAVEPOINT checkpoint",
        ] {
            let sql = format!("SELECT 1\r\n \r\n{command}\r\n\r\nSELECT 2");
            let ranges = split_editor_statements(&sql);
            assert_eq!(ranges.len(), 3, "{command}");
            assert_eq!(ranges[0].text, "SELECT 1");
            assert_eq!(ranges[1].text, command);
            assert_eq!(ranges[2].text, "SELECT 2");
        }
        let ranges =
            split_editor_statements("BEGIN\n\nSELECT 1\n\nCOMMIT\n\nBEGIN\n\nSELECT 2\n\nROLLBACK");
        assert_eq!(
            ranges
                .iter()
                .map(|range| range.text.as_str())
                .collect::<Vec<_>>(),
            [
                "BEGIN", "SELECT 1", "COMMIT", "BEGIN", "SELECT 2", "ROLLBACK"
            ]
        );
    }

    #[test]
    fn editor_keeps_ctes_query_clauses_and_case_end_together() {
        let query = "WITH foo AS (SELECT 1 AS n)\n\nSELECT CASE WHEN n = 1 THEN CASE WHEN true THEN 2\n\nEND\n\nEND AS value\n\nFROM foo\n\nORDER BY value";
        let next = "WITH bar AS (SELECT 3)\n\nSELECT * FROM bar";
        let sql = format!("{query}\n\nBEGIN\n\n{next}\n\nVALUES (4)\n\nTABLE example");
        let ranges = split_editor_statements(&sql);
        assert_eq!(
            ranges
                .iter()
                .map(|range| range.text.as_str())
                .collect::<Vec<_>>(),
            [query, "BEGIN", next, "VALUES (4)", "TABLE example"]
        );
    }

    #[test]
    fn editor_keeps_nested_queries_and_cte_body_together() {
        let query = "WITH foo AS (\nSELECT 1\n\nUNION ALL\n\nSELECT 2\n), bar AS (SELECT 3)\n\nSELECT * FROM (\n\nSELECT * FROM foo\n) AS nested";
        let sql = format!("{query}\n\nSELECT 4");
        let ranges = split_editor_statements(&sql);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].text, query);
        assert_eq!(ranges[1].text, "SELECT 4");
    }

    #[test]
    fn editor_keeps_set_operation_operands_together() {
        for operator in ["UNION", "UNION ALL", "INTERSECT", "EXCEPT"] {
            for operand in ["SELECT 2", "(SELECT 2)"] {
                let query = format!("SELECT 1\n\n{operator}\n\n{operand}");
                let ranges = split_editor_statements(&format!("{query}\n\nSELECT 3"));
                assert_eq!(ranges.len(), 2, "{query}");
                assert_eq!(ranges[0].text, query);
                assert_eq!(ranges[1].text, "SELECT 3");
            }
        }
    }

    #[test]
    fn editor_ignores_blank_lines_in_quoted_content_and_comments() {
        for query in [
            "SELECT 'a\n\nselect 99' AS literal",
            "SELECT 'a\n\nBEGIN\n\nEND' AS literal",
            "SELECT E'escaped\\'\n\nselect 99' AS literal",
            "SELECT 1 AS \"a\n\nselect 99\"",
            "SELECT $tag$a\n\nselect 99$tag$ AS literal",
            "SELECT $tag$a\n\nBEGIN\n\nROLLBACK$tag$ AS literal",
            "SELECT 1 /* outer\n\nselect /* nested */ 99 */ AS value",
            "SELECT 1 /* outer\n\nBEGIN /* nested */ COMMIT */ AS value",
            "SELECT 1 -- select 99\nAS value",
            "SELECT 1 AS čselect",
        ] {
            let ranges = split_editor_statements(&format!("{query}\n\nSELECT 2"));
            assert_eq!(ranges.len(), 2, "{query}");
            assert_eq!(ranges[0].text, query);
            assert_eq!(ranges[1].text, "SELECT 2");
        }
    }

    #[test]
    fn editor_respects_semicolons_and_requires_a_blank_line() {
        assert_eq!(split_editor_statements("select 1\nselect 2").len(), 1);
        assert_eq!(split_editor_statements("select 1\nbegin").len(), 1);
        let sql = "select 1;select 2\r\n \r\nselect 3; -- end";
        let ranges = split_editor_statements(sql);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].text, "select 1;");
        assert_eq!(ranges[1].text, "select 2");
        assert_eq!(ranges[2].text, "select 3;");
        assert_eq!(ranges[1].start, sql.find("select 2").unwrap());
    }

    #[test]
    fn editor_does_not_split_selects_belonging_to_other_commands() {
        for sql in [
            "CREATE TABLE example AS\n\nSELECT 1",
            "EXPLAIN SELECT 1\n\nUNION ALL SELECT 2",
            "INSERT INTO example\n\nSELECT 1",
        ] {
            assert_eq!(split_editor_statements(sql), split_statements(sql), "{sql}");
        }
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
