pub mod conn_url;
pub mod error;
pub mod opencode;
pub mod pg;
pub mod sql;
pub mod storage;

pub use conn_url::{ConnectionFields, build_connection_url, parse_connection_url};
pub use error::{CoreError, PgErrorInfo};
pub use opencode::{AiCompletionRequest, AiHealth, OpenCodeClient};
pub use pg::{
    ColumnMeta, ConnectionStats, PgCancelToken, PgClient, QueryResult, SchemaColumn, SchemaItem,
    SchemaTree, cancel_query_token, connect_client, connection_stats, execute_on_client,
    execute_query, get_ddl, introspect_schema, test_connection,
};
pub use sql::{StatementKind, StatementRange, classify_statement, split_statements};
pub use storage::{
    ConnectionInput, ConnectionRecord, HistoryEntry, HistoryPage, HistoryStats, NewHistoryEntry,
    Settings, Storage,
};
