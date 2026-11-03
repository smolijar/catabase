//! Conservative SQL classification for automatic transaction boundaries.
//!
//! The PostgreSQL AST is visited recursively, so a read-looking outer SELECT
//! cannot hide a write in a CTE, subquery, or function argument. This is a UX
//! safeguard, not a database authorization boundary: custom operators, casts,
//! function overloads and search_path resolution still belong to PostgreSQL.

use std::ops::ControlFlow;

use sqlparser::{
    ast::{Expr, Ident, ObjectName, Query, Select, Statement, TableFactor, Visit, Visitor},
    dialect::PostgreSqlDialect,
    keywords::Keyword,
    parser::Parser,
    tokenizer::Token,
};

/// True unless every statement is recognized as read-only. Unsupported syntax
/// and unknown functions use a transaction instead of guessing that they read.
pub fn requires_transaction(sql: &str) -> bool {
    let dialect = PostgreSqlDialect {};
    let Ok(mut parser) = Parser::new(&dialect).try_with_sql(sql) else {
        return true;
    };
    loop {
        while parser.consume_token(&Token::SemiColon) {}
        if parser.peek_token().token == Token::EOF {
            return false;
        }
        // sqlparser handles TABLE inside query bodies but omits it from its
        // top-level statement dispatcher. Route that PostgreSQL shorthand to
        // the same query parser, without rewriting the user's SQL.
        let statement = if parser.peek_keyword(Keyword::TABLE) {
            parser.parse_query().map(Statement::Query)
        } else {
            parser.parse_statement()
        };
        let Ok(statement) = statement else {
            return true;
        };
        if statement_requires_transaction(&statement) {
            return true;
        }
        // Require complete consumption, including delimiters between every
        // statement. Never classify only a successfully parsed prefix.
        if parser.peek_token().token != Token::EOF && !parser.consume_token(&Token::SemiColon) {
            return true;
        }
    }
}

fn statement_requires_transaction(statement: &Statement) -> bool {
    if let Statement::Explain {
        analyze,
        options,
        statement,
        ..
    } = statement
    {
        // PostgreSQL accepts both EXPLAIN ANALYZE and EXPLAIN (ANALYZE true).
        // An absent/unknown option value is treated as enabled; only explicit
        // false values let EXPLAIN of a write avoid an automatic transaction.
        let runs_statement = *analyze
            || options.as_ref().is_some_and(|options| {
                options.iter().any(|option| {
                    option.name.value.eq_ignore_ascii_case("analyze")
                        && !option.arg.as_ref().is_some_and(|arg| {
                            matches!(
                                arg.to_string().to_ascii_lowercase().as_str(),
                                "false" | "off" | "0" | "'false'" | "'off'" | "'0'"
                            )
                        })
                })
            });
        return runs_statement && statement_requires_transaction(statement);
    }
    statement.visit(&mut ReadOnlyVisitor).is_break()
}

struct ReadOnlyVisitor;

impl Visitor for ReadOnlyVisitor {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        match statement {
            Statement::Query(_) | Statement::ShowVariable { .. } => ControlFlow::Continue(()),
            _ => ControlFlow::Break(()),
        }
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if query.locks.is_empty() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        if select.into.is_none() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(function) = expr
            && !known_read_function(&function.name)
        {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, table: &TableFactor) -> ControlFlow<Self::Break> {
        match table {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. }
                if !known_read_function(name) =>
            {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        }
    }
}

fn identifier_name(ident: &Ident) -> String {
    if ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_ascii_lowercase()
    }
}

fn known_read_function(name: &ObjectName) -> bool {
    let parts = name
        .0
        .iter()
        .map(|part| part.as_ident().map(identifier_name))
        .collect::<Option<Vec<_>>>();
    let Some(parts) = parts else { return false };
    let name = match parts.as_slice() {
        [name] => name,
        [schema, name] if schema == "pg_catalog" => name,
        _ => return false,
    };
    // A deliberately bounded set of common PostgreSQL read functions. Do not
    // allow whole prefixes (e.g. pg_ or jsonb_): some functions change state.
    // Unknown/user-defined functions may write and therefore need a transaction.
    // Volatility alone is not a write test: random(), clock_timestamp() and
    // pg_sleep() are volatile but do not change application data.
    const READ_FUNCTIONS: &str = "
        count sum avg min max bool_and bool_or every array_agg string_agg
        json_agg jsonb_agg json_object_agg jsonb_object_agg
        row_number rank dense_rank percent_rank cume_dist ntile lag lead
        first_value last_value nth_value percentile_cont percentile_disc mode
        coalesce nullif greatest least abs ceil ceiling floor round trunc mod
        power sqrt exp ln log sign random
        lower upper length char_length character_length octet_length concat concat_ws
        left right replace split_part strpos substr substring trim ltrim rtrim btrim
        regexp_replace regexp_match regexp_matches regexp_split_to_array regexp_split_to_table
        quote_ident quote_literal quote_nullable format md5 encode decode
        now transaction_timestamp statement_timestamp clock_timestamp age
        current_timestamp current_date current_time localtimestamp localtime
        current_user current_role session_user user
        date_trunc date_part date_bin make_date make_timestamp to_char to_date to_timestamp timezone
        cardinality array_length array_ndims array_lower array_upper array_position array_positions
        array_append array_prepend array_cat array_remove array_replace array_to_string
        string_to_array unnest generate_series generate_subscripts
        to_json to_jsonb row_to_json json_build_array jsonb_build_array json_build_object jsonb_build_object
        json_array_length jsonb_array_length json_each jsonb_each json_each_text jsonb_each_text
        json_array_elements jsonb_array_elements json_array_elements_text jsonb_array_elements_text
        json_object_keys jsonb_object_keys json_extract_path jsonb_extract_path
        json_extract_path_text jsonb_extract_path_text json_typeof jsonb_typeof
        json_to_record jsonb_to_record json_to_recordset jsonb_to_recordset
        current_database current_schema current_schemas current_setting version pg_typeof
        pg_backend_pid pg_size_pretty pg_relation_size pg_total_relation_size pg_table_size
        pg_sleep pg_sleep_for pg_sleep_until
    ";
    READ_FUNCTIONS
        .split_ascii_whitespace()
        .any(|item| item == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::auto_limit_sql;

    #[test]
    fn ordinary_reads_and_read_only_ctes_do_not_start_transactions() {
        for sql in [
            "SELECT 1",
            "select * from public.landlords where name like '%UPDATE%'",
            "WITH foo AS (SELECT 1 AS id) SELECT * FROM foo",
            "WITH foo AS MATERIALIZED (SELECT 1) SELECT * FROM foo",
            "WITH RECURSIVE n(x) AS (VALUES (1) UNION ALL SELECT x + 1 FROM n WHERE x < 5) SELECT * FROM n",
            "WITH foo AS (WITH bar AS (SELECT 1) SELECT * FROM bar) SELECT * FROM foo",
            "SELECT (SELECT count(*) FROM properties) FROM landlords",
            "SELECT id FROM landlords UNION ALL SELECT id FROM properties",
            "VALUES (1), (2)",
            "TABLE public.landlords",
            "SHOW statement_timeout",
            "SHOW ALL",
            "SELECT CURRENT_TIMESTAMP, CURRENT_DATE, CURRENT_USER",
            "SELECT 1; WITH foo AS (SELECT 2) SELECT * FROM foo;",
            "select '-- update' as name, $$DELETE FROM landlords$$, E'\\'insert'",
            "/* DELETE /* nested UPDATE */ still a comment */ SELECT \"update\" FROM \"insert\"",
        ] {
            assert!(!requires_transaction(sql), "{sql}");
        }
    }

    #[test]
    fn write_ctes_and_locks_are_detected_at_every_depth() {
        for sql in [
            "WITH edited AS (UPDATE landlords SET name = 'new' RETURNING *) SELECT * FROM edited",
            "WITH added AS (INSERT INTO landlords (name) VALUES ('new') RETURNING id) SELECT id FROM added",
            "WITH removed AS (DELETE FROM landlords RETURNING id) SELECT * FROM removed",
            "WITH foo AS (SELECT 1), edited AS (UPDATE landlords SET name = 'new' RETURNING *) SELECT * FROM foo",
            "WITH foo AS (WITH edited AS (DELETE FROM landlords RETURNING *) SELECT * FROM edited) SELECT * FROM foo",
            "SELECT * FROM (WITH edited AS (UPDATE landlords SET name = 'new' RETURNING *) SELECT * FROM edited) x",
            "WITH foo AS (SELECT 1) UPDATE landlords SET name = 'new'",
            "SELECT * INTO TEMP copied FROM landlords",
            "WITH foo AS (SELECT * FROM landlords FOR UPDATE) SELECT * FROM foo",
            "SELECT * FROM landlords FOR NO KEY UPDATE",
            "SELECT * FROM landlords FOR KEY SHARE",
            "SELECT * FROM (SELECT * FROM landlords FOR SHARE) AS locked",
            "SELECT 1; UPDATE landlords SET name = 'new'",
            "CREATE TABLE test(id int)",
            "TRUNCATE landlords",
            "DO $$ BEGIN UPDATE landlords SET name = 'new'; END $$",
            "CALL update_landlord()",
            "SOME FUTURE POSTGRESQL SYNTAX",
            "SELECT ('unterminated",
            "SELECT 1 END; DELETE FROM landlords",
            "TABLE landlords; DELETE FROM landlords",
        ] {
            assert!(requires_transaction(sql), "{sql}");
        }
    }

    #[test]
    fn unknown_and_state_changing_functions_need_transactions() {
        for sql in [
            "SELECT nextval('landlords_id_seq')",
            "SELECT pg_catalog.setval('landlords_id_seq', 1)",
            "SELECT set_config('search_path', 'private', false)",
            "SELECT pg_advisory_lock(1)",
            "SELECT pg_notify('channel', 'message')",
            "SELECT update_landlord()",
            "SELECT public.lower('CUSTOM')",
            "SELECT \"LOWER\"('CUSTOM')",
            "SELECT * FROM update_landlord()",
            "SELECT * FROM LATERAL update_landlord()",
            "SELECT count(*) FROM landlords WHERE id = nextval('ids')",
            "SELECT coalesce((SELECT nextval('ids')), 1)",
            "SELECT * FROM generate_series(1, update_landlord())",
            "WITH foo AS (SELECT update_landlord()) SELECT * FROM foo",
        ] {
            assert!(requires_transaction(sql), "{sql}");
        }
        for sql in [
            "SELECT count(*), min(id), max(id) FROM landlords",
            "SELECT pg_catalog.lower('HELLO'), UPPER('hello'), now(), coalesce(NULL, 1)",
            "SELECT * FROM generate_series(1, 10)",
            "SELECT * FROM LATERAL jsonb_to_recordset('[{\"id\":1}]') AS x(id int)",
            "SELECT row_number() OVER (ORDER BY id), sum(id) OVER () FROM landlords",
            "SELECT pg_sleep(1), current_database(), random()",
        ] {
            assert!(!requires_transaction(sql), "{sql}");
        }
    }

    #[test]
    fn explain_only_needs_transaction_when_it_executes_a_write() {
        for sql in [
            "EXPLAIN UPDATE landlords SET name = 'new'",
            "EXPLAIN (ANALYZE false, FORMAT JSON) UPDATE landlords SET name = 'new'",
            "EXPLAIN ANALYZE SELECT * FROM landlords",
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT * FROM landlords",
        ] {
            assert!(!requires_transaction(sql), "{sql}");
        }
        for sql in [
            "EXPLAIN ANALYZE UPDATE landlords SET name = 'new'",
            "EXPLAIN (ANALYZE true, FORMAT JSON) DELETE FROM landlords",
            "EXPLAIN (ANALYZE) WITH edited AS (DELETE FROM landlords RETURNING *) SELECT * FROM edited",
            "EXPLAIN ANALYZE SELECT nextval('ids')",
        ] {
            assert!(requires_transaction(sql), "{sql}");
        }
    }

    #[test]
    fn unsafe_selects_are_not_automatically_limited_or_replayed() {
        for sql in [
            "WITH edited AS (DELETE FROM landlords RETURNING *) SELECT * FROM edited",
            "SELECT nextval('ids')",
            "SELECT * FROM landlords FOR\nUPDATE",
            "SELECT *\nINTO TEMP copied FROM landlords",
        ] {
            assert!(auto_limit_sql(sql, 0, 100).is_none(), "{sql}");
        }
        assert!(auto_limit_sql("WITH foo AS (SELECT 1) SELECT * FROM foo", 0, 100).is_some());
    }
}
