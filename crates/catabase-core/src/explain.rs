//! Validation for the explicit EXPLAIN workflow, which intentionally bypasses
//! automatic BEGIN. Neither the editable preamble nor the selection may smuggle
//! an additional command into that execution path.

use serde_json::Value;
use sqlparser::{
    ast::{AnalyzeFormat, AnalyzeFormatKind, DescribeAlias, Statement},
    dialect::PostgreSqlDialect,
    keywords::Keyword,
    parser::Parser,
    tokenizer::Token,
};

use crate::{CoreError, QueryResult};

pub const DEFAULT_EXPLAIN_PREAMBLE: &str =
    "EXPLAIN (ANALYZE, COSTS, VERBOSE, BUFFERS, FORMAT JSON)";

/// Keep the user's source text intact. Parsing the preamble with a sentinel
/// proves it contains options only, and parsing the selection independently
/// proves it contains exactly one explainable statement. A newline between the
/// two also prevents a trailing preamble line comment from swallowing the SQL.
pub fn build_explain_sql(query: &str, preamble: &str) -> Result<String, CoreError> {
    let query = query.trim();
    let preamble = preamble.trim();
    if query.is_empty() {
        return Err(CoreError::message("Select a query to explain."));
    }
    if preamble.is_empty() {
        return Err(CoreError::message("Enter an EXPLAIN preamble."));
    }

    const SENTINEL: &str = "SELECT 8675309 AS __catabase_explain_preamble_probe";
    let probe = parse_one(&format!("{preamble}\n{SENTINEL}")).map_err(|_| invalid_preamble())?;
    match &probe {
        Statement::Explain {
            describe_alias: DescribeAlias::Explain,
            query_plan: false,
            estimate: false,
            statement,
            format,
            options,
            ..
        } if statement.to_string() == SENTINEL => {
            if let Some(AnalyzeFormatKind::Keyword(format) | AnalyzeFormatKind::Assignment(format)) =
                format
                && !matches!(format, AnalyzeFormat::JSON | AnalyzeFormat::TEXT)
            {
                return Err(unsupported_format());
            }
            if let Some(options) = options {
                for option in options {
                    if option.name.value.eq_ignore_ascii_case("format") {
                        let format = option.arg.as_ref().map(ToString::to_string);
                        let format = format
                            .as_deref()
                            .map(|value| value.trim_matches('\'').to_ascii_lowercase());
                        if !matches!(format.as_deref(), Some("json" | "text")) {
                            return Err(unsupported_format());
                        }
                    }
                }
            }
        }
        _ => return Err(invalid_preamble()),
    }

    let statement = parse_one(query).map_err(|_| {
        CoreError::message("Explain requires one complete SQL statement. Select just one query and check its syntax.")
    })?;
    if matches!(
        statement,
        Statement::Explain { .. } | Statement::ExplainTable { .. }
    ) {
        return Err(CoreError::message(
            "Select the query without EXPLAIN; configure EXPLAIN in the preamble field.",
        ));
    }
    if !explainable(&statement) {
        return Err(CoreError::message(
            "This statement cannot be explained. Select a SELECT, INSERT, UPDATE, DELETE, MERGE, VALUES, EXECUTE, cursor query, or CREATE TABLE/MATERIALIZED VIEW AS query.",
        ));
    }

    Ok(format!("{preamble}\n{query}"))
}

fn invalid_preamble() -> CoreError {
    CoreError::message(
        "The preamble must contain EXPLAIN and its options only. Put the query in the SQL editor and remove extra statements from the preamble.",
    )
}

fn unsupported_format() -> CoreError {
    CoreError::message("Use FORMAT JSON or FORMAT TEXT for Explain and Dalibo sharing.")
}

fn parse_one(sql: &str) -> Result<Statement, ()> {
    let dialect = PostgreSqlDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(sql).map_err(|_| ())?;
    // sqlparser supports TABLE query bodies but not its top-level dispatcher.
    let statement = if parser.peek_keyword(Keyword::TABLE) {
        parser.parse_query().map(Statement::Query)
    } else {
        parser.parse_statement()
    }
    .map_err(|_| ())?;
    while parser.consume_token(&Token::SemiColon) {}
    if parser.peek_token().token != Token::EOF {
        return Err(());
    }
    Ok(statement)
}

fn explainable(statement: &Statement) -> bool {
    match statement {
        Statement::Query(_)
        | Statement::Insert(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Merge(_)
        | Statement::Execute { .. } => true,
        Statement::CreateTable(table) => table.query.is_some(),
        Statement::CreateView(view) => view.materialized,
        Statement::Declare { stmts } => stmts.len() == 1 && stmts[0].for_query.is_some(),
        _ => false,
    }
}

/// Convert PostgreSQL's one JSON cell or many text lines into the exact source
/// accepted by the plan viewer. Raw JSON is retained rather than reserialized so
/// high-precision numeric values in a plan are not rounded during conversion.
pub fn extract_explain_plan(result: &QueryResult) -> Result<String, CoreError> {
    let invalid = || {
        CoreError::message(
            "PostgreSQL did not return a complete EXPLAIN plan in JSON or text format.",
        )
    };
    if result.columns.len() != 1
        || result.columns[0].name != "QUERY PLAN"
        || result.rows.is_empty()
        || result.has_more
        || result.auto_limited
    {
        return Err(invalid());
    }
    let values = result
        .rows
        .iter()
        .map(|row| {
            let object = row
                .as_object()
                .filter(|object| object.len() == 1)
                .ok_or_else(invalid)?;
            object.get("QUERY PLAN").ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    match result.columns[0].type_name.as_str() {
        "json" | "jsonb" => {
            if values.len() != 1 {
                return Err(invalid());
            }
            let source = match values[0] {
                Value::String(source) => source.clone(),
                value @ Value::Array(_) => serde_json::to_string_pretty(value)?,
                _ => return Err(invalid()),
            };
            let parsed: Value = serde_json::from_str(&source).map_err(|_| invalid())?;
            let plans = parsed
                .as_array()
                .filter(|plans| !plans.is_empty())
                .ok_or_else(invalid)?;
            if !plans.iter().all(|entry| {
                entry
                    .get("Plan")
                    .and_then(|plan| plan.get("Node Type"))
                    .and_then(Value::as_str)
                    .is_some_and(|node| !node.is_empty())
            }) {
                return Err(invalid());
            }
            Ok(source)
        }
        "text" => {
            let lines = values
                .iter()
                .map(|value| value.as_str().ok_or_else(invalid))
                .collect::<Result<Vec<_>, _>>()?;
            let source = lines.join("\n");
            if source.trim().is_empty() {
                return Err(invalid());
            }
            Ok(source)
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ColumnMeta;
    use serde_json::json;

    fn result(type_name: &str, cells: Vec<Value>) -> QueryResult {
        let mut result = QueryResult::command(cells.len(), 1);
        result.columns = vec![ColumnMeta {
            name: "QUERY PLAN".into(),
            source_name: None,
            type_oid: 0,
            type_name: type_name.into(),
            table_oid: None,
            table_schema: None,
            table_name: None,
            column_id: None,
            nullable: None,
            generated: false,
            primary_key: false,
        }];
        result.rows = cells
            .into_iter()
            .map(|value| json!({"QUERY PLAN": value}))
            .collect();
        result
    }

    #[test]
    fn preamble_and_query_are_preserved_for_supported_formats_and_statements() {
        for preamble in [
            DEFAULT_EXPLAIN_PREAMBLE,
            "EXPLAIN",
            "EXPLAIN ANALYZE VERBOSE",
            "EXPLAIN (ANALYZE false, COSTS false, FORMAT TEXT)",
            "EXPLAIN (ANALYZE on, FORMAT 'json', TIMING off, SETTINGS)",
            "/* before */ EXPLAIN (/* a */ FORMAT JSON) -- reason",
        ] {
            for query in [
                "SELECT * FROM landlords WHERE name = 'quoted; string'; -- end",
                "WITH x AS (SELECT 1) SELECT * FROM x",
                "WITH changed AS (UPDATE properties SET name = 'new' RETURNING *) SELECT * FROM changed",
                "SELECT $$; ROLLBACK;$$, E'\\'quoted; value'",
                "INSERT INTO t (id) VALUES (1) RETURNING id",
                "DELETE FROM t WHERE id = 1",
                "UPDATE t SET id = 2",
                "VALUES (1), (2)",
                "TABLE public.landlords",
                "EXECUTE prepared_query(1)",
                "CREATE TEMP TABLE copied AS SELECT * FROM landlords",
                "CREATE MATERIALIZED VIEW copied AS SELECT 1",
                "DECLARE rows CURSOR FOR SELECT 1",
            ] {
                assert_eq!(
                    build_explain_sql(query, preamble).unwrap(),
                    format!("{preamble}\n{query}")
                );
            }
        }
    }

    #[test]
    fn preamble_cannot_replace_the_query_or_append_unexplained_statements() {
        for preamble in [
            "",
            "SELECT",
            "DESCRIBE",
            "EXPLAIN QUERY PLAN",
            "EXPLAIN ESTIMATE",
            "EXPLAIN SELECT 1;",
            "EXPLAIN (ANALYZE) SELECT 1 -- ignored",
            "EXPLAIN (FORMAT JSON); DELETE FROM t;",
            "EXPLAIN (ANALYZE) SELECT 8675309 AS __catabase_explain_preamble_probe;",
            "EXPLAIN (ANALYZE) /* unterminated",
            "EXPLAIN (ANALYZE) $tag$",
            "BEGIN; EXPLAIN",
            "EXPLAIN (FORMAT JSON) SELECT 1 UNION ALL",
        ] {
            assert!(
                build_explain_sql("SELECT 1", preamble).is_err(),
                "{preamble}"
            );
        }
    }

    #[test]
    fn unsupported_formats_and_non_queries_are_rejected_before_execution() {
        for preamble in [
            "EXPLAIN (FORMAT YAML)",
            "EXPLAIN (FORMAT XML)",
            "EXPLAIN (FORMAT 'xml')",
            "EXPLAIN FORMAT GRAPHVIZ",
        ] {
            assert!(
                build_explain_sql("SELECT 1", preamble).is_err(),
                "{preamble}"
            );
        }
        for query in [
            "",
            "-- empty",
            "SELECT 1; SELECT 2",
            "SELECT 1; DELETE FROM t",
            "SELECT 1\n\nUPDATE t SET id = 3",
            "SELECT 1 END",
            "SELECT 1; END",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT a",
            "DO $$BEGIN END$$",
            "CREATE TABLE t (id int)",
            "CREATE VIEW v AS SELECT 1",
            "SHOW ALL",
            "EXPLAIN SELECT 1",
            "EXPLAIN (FORMAT JSON) SELECT 1",
        ] {
            assert!(
                build_explain_sql(query, DEFAULT_EXPLAIN_PREAMBLE).is_err(),
                "{query}"
            );
        }
    }

    #[test]
    fn json_plan_keeps_exact_source_and_accepts_legacy_decoded_arrays() {
        let raw = r#"[ { "Plan": { "Node Type": "Result", "Total Cost": 0.12345678901234567890123456789 } } ]"#;
        let mut raw_result = result("json", vec![json!(raw)]);
        raw_result.json_values_as_text = true;
        assert_eq!(extract_explain_plan(&raw_result).unwrap(), raw);
        let decoded = json!([{"Plan": {"Node Type": "Result"}}]);
        let extracted = extract_explain_plan(&result("json", vec![decoded.clone()])).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&extracted).unwrap(), decoded);
    }

    #[test]
    fn text_plan_joins_all_lines_without_losing_indentation() {
        let rows = vec![
            json!("Result  (cost=0.00..0.01 rows=1 width=4)"),
            json!("  Output: 1"),
            json!("Planning Time: 0.01 ms"),
        ];
        assert_eq!(
            extract_explain_plan(&result("text", rows)).unwrap(),
            "Result  (cost=0.00..0.01 rows=1 width=4)\n  Output: 1\nPlanning Time: 0.01 ms"
        );
    }

    #[test]
    fn incomplete_or_unexpected_results_are_not_treated_as_plans() {
        for value in [
            json!(null),
            json!("not json"),
            json!([]),
            json!({"Plan": {"Node Type": "Result"}}),
            json!([{"no_plan": true}]),
        ] {
            assert!(extract_explain_plan(&result("json", vec![value])).is_err());
        }
        assert!(extract_explain_plan(&result("text", vec![])).is_err());
        assert!(extract_explain_plan(&result("text", vec![json!("")])).is_err());
        assert!(extract_explain_plan(&result("text", vec![json!(42)])).is_err());
        assert!(extract_explain_plan(&result("xml", vec![json!("<plan/>")])).is_err());
        let mut wrong = result("text", vec![json!("Result")]);
        wrong.columns[0].name = "id".into();
        assert!(extract_explain_plan(&wrong).is_err());
        let mut partial = result("text", vec![json!("Result")]);
        partial.has_more = true;
        assert!(extract_explain_plan(&partial).is_err());
    }
}
