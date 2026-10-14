use std::{
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ConnectionFields, CoreError, QueryResult};

const KEYRING_SERVICE: &str = "catabase";
const RESULT_JSON_CAP_BYTES: usize = 256 * 1024;
const KEYRING_TIMEOUT: Duration = Duration::from_secs(2);

/// Serialize once from borrowed data and stop immediately at the snapshot cap.
/// Oversized entries retain query metadata, but have no snapshot or exact size.
struct SnapshotWriter {
    buffer: Vec<u8>,
    exceeded: bool,
}

impl Write for SnapshotWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > RESULT_JSON_CAP_BYTES - self.buffer.len() {
            self.exceeded = true;
            return Err(io::Error::other("history snapshot exceeds size cap"));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HistorySnapshot {
    json: Option<String>,
    size: Option<i64>,
}

impl HistorySnapshot {
    fn oversized() -> Self {
        Self {
            json: None,
            size: None,
        }
    }
}

fn history_snapshot(result: &impl Serialize) -> Result<HistorySnapshot, CoreError> {
    let mut writer = SnapshotWriter {
        buffer: Vec::new(),
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, result) {
        if writer.exceeded {
            return Ok(HistorySnapshot::oversized());
        }
        return Err(error.into());
    }
    Ok(HistorySnapshot {
        size: Some(writer.buffer.len() as i64),
        json: Some(
            String::from_utf8(writer.buffer)
                .map_err(|error| CoreError::message(error.to_string()))?,
        ),
    })
}

// Raw UTF-8 lengths are a lower bound on encoded JSON. This preflight avoids
// even scanning/escaping megabyte strings when they already exceed the cap.
// It never rejects a snapshot that could fit, and stops once the bound is hit.
fn payload_exceeds_cap(value: &serde_json::Value, remaining: &mut usize) -> bool {
    let bytes = match value {
        serde_json::Value::String(text) => text.len() + 2,
        serde_json::Value::Null => 4,
        serde_json::Value::Bool(value) => {
            if *value {
                4
            } else {
                5
            }
        }
        serde_json::Value::Number(_) => 1,
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => 2,
    };
    if bytes > *remaining {
        return true;
    }
    *remaining -= bytes;
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| payload_exceeds_cap(value, remaining)),
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            let bytes = key.len() + 3;
            if bytes > *remaining {
                return true;
            }
            *remaining -= bytes;
            payload_exceeds_cap(value, remaining)
        }),
        _ => false,
    }
}

#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub pool_size: u32,
    pub ai_model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRecord {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewHistoryEntry {
    pub connection_id: Option<String>,
    pub query: String,
    pub duration_ms: u128,
    pub row_count: Option<usize>,
    pub is_error: bool,
    pub error_message: Option<String>,
    pub result_json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub connection_id: Option<String>,
    pub query: String,
    pub duration_ms: i64,
    pub row_count: Option<i64>,
    pub result_size: Option<i64>,
    pub is_error: bool,
    pub error_message: Option<String>,
    pub has_result: bool,
    pub executed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub total: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    pub total: i64,
    pub errors: i64,
    pub last_executed_at: Option<String>,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let conn = Connection::open(path)?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let storage = Self {
            conn: Arc::new(Mutex::new(Connection::open_in_memory()?)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn get_settings(&self) -> Result<Settings, CoreError> {
        let conn = self.lock()?;
        let pool_size = setting_value(&conn, "pool_size")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(5)
            .clamp(1, 32);
        let ai_model = setting_value(&conn, "ai_model")?.filter(|value| !value.is_empty());
        Ok(Settings {
            pool_size,
            ai_model,
        })
    }

    pub fn set_pool_size(&self, pool_size: u32) -> Result<u32, CoreError> {
        let normalized = pool_size.clamp(1, 32);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('pool_size', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![normalized.to_string()],
        )?;
        Ok(normalized)
    }

    pub fn set_ai_model(&self, model: Option<String>) -> Result<Option<String>, CoreError> {
        let normalized = model.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        let conn = self.lock()?;
        match &normalized {
            Some(model) => {
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('ai_model', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![model],
                )?;
            }
            None => {
                conn.execute("DELETE FROM settings WHERE key = 'ai_model'", [])?;
            }
        }
        Ok(normalized)
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, database, username, password_fallback, ssl,
                    is_active, created_at, updated_at
             FROM connections
             ORDER BY is_active DESC, updated_at DESC, name ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_connection)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().map(resolve_connection_password).collect())
    }

    pub fn active_connection(&self) -> Result<Option<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let record = conn
            .query_row(
                "SELECT id, name, host, port, database, username, password_fallback, ssl,
                        is_active, created_at, updated_at
                 FROM connections WHERE is_active = 1 LIMIT 1",
                [],
                row_to_connection,
            )
            .optional()?;
        Ok(record.map(resolve_connection_password))
    }

    pub fn save_connection(&self, input: ConnectionInput) -> Result<ConnectionRecord, CoreError> {
        self.save_connection_inner(input, true)
    }

    pub fn duplicate_connection(&self, id: &str) -> Result<ConnectionRecord, CoreError> {
        let source = self
            .get_connection(id)?
            .ok_or_else(|| CoreError::message("The copied connection no longer exists."))?;
        // Creating a copy must not activate it, reset the pool or lose the
        // original connection's resolved keyring/fallback password.
        self.save_connection_inner(connection_copy_input(&source), false)
    }

    fn save_connection_inner(
        &self,
        input: ConnectionInput,
        activate_new: bool,
    ) -> Result<ConnectionRecord, CoreError> {
        validate_connection(&input)?;

        let now = Utc::now().to_rfc3339();
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let incoming_password = input
            .password
            .clone()
            .filter(|password| !password.is_empty());
        if let Some(password) = incoming_password.as_deref() {
            let _ = set_and_verify_keyring_password(&id, password);
        }

        {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            let existing = tx
                .query_row(
                    "SELECT created_at, is_active, password_fallback FROM connections WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)? == 1,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            let is_new = existing.is_none();
            let created_at = existing
                .as_ref()
                .map(|(created_at, _, _)| created_at.clone())
                .unwrap_or_else(|| now.clone());
            let is_active = existing
                .as_ref()
                .map(|(_, is_active, _)| *is_active)
                .unwrap_or(activate_new);
            let stored_fallback = incoming_password
                .clone()
                .or_else(|| existing.and_then(|(_, _, password)| password));

            if is_new && activate_new {
                tx.execute("UPDATE connections SET is_active = 0", [])?;
            }

            let mut name = input.name.trim().to_string();
            if is_new && !activate_new {
                // Generate collision-free copy names inside the write transaction.
                let base = name.clone();
                let mut number = 2;
                while tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM connections WHERE name = ?1)",
                    params![name],
                    |row| row.get::<_, bool>(0),
                )? {
                    name = format!("{base} {number}");
                    number += 1;
                }
            }

            tx.execute(
                "INSERT INTO connections (
                    id, name, host, port, database, username, ssl, password_fallback,
                    is_active, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    host = excluded.host,
                    port = excluded.port,
                    database = excluded.database,
                    username = excluded.username,
                    ssl = excluded.ssl,
                    password_fallback = excluded.password_fallback,
                    is_active = excluded.is_active,
                    updated_at = excluded.updated_at",
                params![
                    id,
                    name,
                    input.host.trim(),
                    input.port,
                    input.database.trim(),
                    input.username.trim(),
                    bool_to_i64(input.ssl),
                    stored_fallback,
                    bool_to_i64(is_active),
                    created_at,
                    now,
                ],
            )?;
            tx.commit()?;
        }

        self.get_connection(&id)
            .and_then(|record| record.ok_or_else(|| CoreError::message("connection was not saved")))
    }

    pub fn get_connection(&self, id: &str) -> Result<Option<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let record = conn
            .query_row(
                "SELECT id, name, host, port, database, username, password_fallback, ssl,
                        is_active, created_at, updated_at
                 FROM connections WHERE id = ?1",
                params![id],
                row_to_connection,
            )
            .optional()?;
        Ok(record.map(resolve_connection_password))
    }

    pub fn activate_connection(&self, id: &str) -> Result<ConnectionRecord, CoreError> {
        let changed = {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            tx.execute("UPDATE connections SET is_active = 0", [])?;
            let changed = tx.execute(
                "UPDATE connections SET is_active = 1, updated_at = ?2 WHERE id = ?1",
                params![id, Utc::now().to_rfc3339()],
            )?;
            tx.commit()?;
            changed
        };
        if changed == 0 {
            return Err(CoreError::message("connection not found"));
        }
        self.get_connection(id)?
            .ok_or_else(|| CoreError::message("connection not found"))
    }

    pub fn delete_connection(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
        let _ = delete_keyring_password(id);
        Ok(())
    }

    pub fn save_draft(&self, tab_kind: &str, content: &str) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO script_drafts (tab_kind, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(tab_kind) DO UPDATE SET
                content = excluded.content,
                updated_at = excluded.updated_at",
            params![tab_kind, content, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn load_draft(&self, tab_kind: &str) -> Result<Option<String>, CoreError> {
        let conn = self.lock()?;
        Ok(conn
            .query_row(
                "SELECT content FROM script_drafts WHERE tab_kind = ?1",
                params![tab_kind],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn insert_history(&self, entry: NewHistoryEntry) -> Result<i64, CoreError> {
        let snapshot = entry
            .result_json
            .as_ref()
            .map(history_snapshot)
            .transpose()?;
        self.insert_history_snapshot(entry, snapshot)
    }

    /// Successful query history borrows the result instead of cloning it into
    /// serde_json::Value before serializing a potentially discarded snapshot.
    pub fn insert_query_history(
        &self,
        connection_id: Option<String>,
        query: String,
        result: &QueryResult,
    ) -> Result<i64, CoreError> {
        let mut remaining = RESULT_JSON_CAP_BYTES;
        let snapshot = if result
            .rows
            .iter()
            .any(|row| payload_exceeds_cap(row, &mut remaining))
        {
            HistorySnapshot::oversized()
        } else {
            history_snapshot(result)?
        };
        self.insert_history_snapshot(
            NewHistoryEntry {
                connection_id,
                query,
                duration_ms: result.duration_ms,
                row_count: Some(result.row_count),
                is_error: false,
                error_message: None,
                result_json: None,
            },
            Some(snapshot),
        )
    }

    fn insert_history_snapshot(
        &self,
        entry: NewHistoryEntry,
        snapshot: Option<HistorySnapshot>,
    ) -> Result<i64, CoreError> {
        let result_size = snapshot.as_ref().and_then(|snapshot| snapshot.size);
        let stored_result = snapshot.and_then(|snapshot| snapshot.json);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO query_history (
                connection_id, query, duration_ms, row_count, result_size, is_error,
                error_message, result_json, executed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.connection_id,
                entry.query,
                i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
                entry.row_count.map(|value| value as i64),
                result_size,
                bool_to_i64(entry.is_error),
                entry.error_message,
                stored_result,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn history_page(
        &self,
        search: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<HistoryPage, CoreError> {
        let conn = self.lock()?;
        let normalized_limit = limit.clamp(1, 200);
        let search = search.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(format!("%{trimmed}%"))
        });

        let total = match &search {
            Some(search) => conn.query_row(
                "SELECT COUNT(*) FROM query_history WHERE query LIKE ?1 OR error_message LIKE ?1",
                params![search],
                |row| row.get(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM query_history", [], |row| row.get(0))?,
        };

        let entries = match &search {
            Some(search) => {
                let mut stmt = conn.prepare(HISTORY_SELECT_WITH_SEARCH)?;
                stmt.query_map(params![search, normalized_limit, offset], row_to_history)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = conn.prepare(HISTORY_SELECT)?;
                stmt.query_map(params![normalized_limit, offset], row_to_history)?
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        Ok(HistoryPage { entries, total })
    }

    pub fn history_stats(&self) -> Result<HistoryStats, CoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(is_error), 0), MAX(executed_at) FROM query_history",
            [],
            |row| {
                Ok(HistoryStats {
                    total: row.get(0)?,
                    errors: row.get(1)?,
                    last_executed_at: row.get(2)?,
                })
            },
        )
        .map_err(CoreError::from)
    }

    pub fn history_result(&self, id: i64) -> Result<Option<serde_json::Value>, CoreError> {
        let conn = self.lock()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT result_json FROM query_history WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        json.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(CoreError::from)
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM query_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn history_purge(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM query_history", [])?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS connections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL DEFAULT 5432,
                database TEXT NOT NULL,
                username TEXT NOT NULL,
                ssl INTEGER NOT NULL DEFAULT 0,
                password_fallback TEXT,
                is_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS script_drafts (
                tab_kind TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                connection_id TEXT,
                query TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                row_count INTEGER,
                result_size INTEGER,
                is_error INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                result_json TEXT,
                executed_at TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_history_executed
                ON query_history(executed_at DESC);",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('pool_size', '5')",
            [],
        )?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.conn
            .lock()
            .map_err(|_| CoreError::message("storage lock poisoned"))
    }
}

impl From<&ConnectionRecord> for ConnectionFields {
    fn from(record: &ConnectionRecord) -> Self {
        Self {
            name: record.name.clone(),
            host: record.host.clone(),
            port: record.port,
            database: record.database.clone(),
            username: record.username.clone(),
            password: record.password.clone(),
            ssl: record.ssl,
        }
    }
}

impl From<&ConnectionInput> for ConnectionFields {
    fn from(input: &ConnectionInput) -> Self {
        Self {
            name: input.name.clone(),
            host: input.host.clone(),
            port: input.port,
            database: input.database.clone(),
            username: input.username.clone(),
            password: input.password.clone(),
            ssl: input.ssl,
        }
    }
}

const HISTORY_SELECT: &str =
    "SELECT id, connection_id, query, duration_ms, row_count, result_size, is_error,
            error_message, result_json IS NOT NULL, executed_at
     FROM query_history
     ORDER BY executed_at DESC
     LIMIT ?1 OFFSET ?2";

const HISTORY_SELECT_WITH_SEARCH: &str =
    "SELECT id, connection_id, query, duration_ms, row_count, result_size, is_error,
            error_message, result_json IS NOT NULL, executed_at
     FROM query_history
     WHERE query LIKE ?1 OR error_message LIKE ?1
     ORDER BY executed_at DESC
     LIMIT ?2 OFFSET ?3";

fn setting_value(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

fn validate_connection(input: &ConnectionInput) -> Result<(), CoreError> {
    if input.name.trim().is_empty() {
        return Err(CoreError::message("connection name is required"));
    }
    if input.host.trim().is_empty() {
        return Err(CoreError::message("host is required"));
    }
    if input.database.trim().is_empty() {
        return Err(CoreError::message("database is required"));
    }
    if input.username.trim().is_empty() {
        return Err(CoreError::message("username is required"));
    }
    if input.port == 0 {
        return Err(CoreError::message("port is required"));
    }
    Ok(())
}

fn row_to_connection(row: &rusqlite::Row<'_>) -> Result<ConnectionRecord, rusqlite::Error> {
    Ok(ConnectionRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get::<_, u16>(3)?,
        database: row.get(4)?,
        username: row.get(5)?,
        password: row.get(6)?,
        ssl: row.get::<_, i64>(7)? == 1,
        is_active: row.get::<_, i64>(8)? == 1,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_history(row: &rusqlite::Row<'_>) -> Result<HistoryEntry, rusqlite::Error> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        connection_id: row.get(1)?,
        query: row.get(2)?,
        duration_ms: row.get(3)?,
        row_count: row.get(4)?,
        result_size: row.get(5)?,
        is_error: row.get::<_, i64>(6)? == 1,
        error_message: row.get(7)?,
        has_result: row.get::<_, i64>(8)? == 1,
        executed_at: row.get(9)?,
    })
}

fn resolve_connection_password(mut record: ConnectionRecord) -> ConnectionRecord {
    if let Some(password) = get_keyring_password(&record.id) {
        record.password = Some(password);
        return record;
    }
    record
}

fn set_and_verify_keyring_password(id: &str, password: &str) -> bool {
    let id = id.to_string();
    let password = password.to_string();
    run_keyring(move || {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        entry
            .set_password(&password)
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        let stored = entry
            .get_password()
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        Ok(stored == password)
    })
    .ok()
    .unwrap_or(false)
}

fn get_keyring_password(id: &str) -> Option<String> {
    let id = id.to_string();
    run_keyring(move || {
        keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?
            .get_password()
            .map_err(|error| CoreError::Keyring(error.to_string()))
    })
    .ok()
}

fn delete_keyring_password(id: &str) -> Result<(), CoreError> {
    let id = id.to_string();
    run_keyring(move || {
        keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?
            .delete_credential()
            .map_err(|error| CoreError::Keyring(error.to_string()))
    })
}

fn run_keyring<T, F>(operation: F) -> Result<T, CoreError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CoreError> + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(operation());
    });
    receiver
        .recv_timeout(KEYRING_TIMEOUT)
        .map_err(|_| CoreError::message("keyring timeout"))?
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn connection_copy_input(source: &ConnectionRecord) -> ConnectionInput {
    ConnectionInput {
        id: None,
        name: format!("{} copy", source.name),
        host: source.host.clone(),
        port: source.port,
        database: source.database.clone(),
        username: source.username.clone(),
        password: source.password.clone(),
        ssl: source.ssl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_default_settings() {
        let storage = Storage::in_memory().unwrap();
        let settings = storage.get_settings().unwrap();

        assert_eq!(settings.pool_size, 5);
        assert_eq!(settings.ai_model, None);
    }

    #[test]
    fn normalizes_pool_size() {
        let storage = Storage::in_memory().unwrap();

        assert_eq!(storage.set_pool_size(0).unwrap(), 1);
        assert_eq!(storage.set_pool_size(999).unwrap(), 32);
    }

    #[test]
    fn stores_drafts_and_history() {
        let storage = Storage::in_memory().unwrap();
        storage.save_draft("playground", "select 1").unwrap();
        assert_eq!(
            storage.load_draft("playground").unwrap().as_deref(),
            Some("select 1")
        );

        let id = storage
            .insert_history(NewHistoryEntry {
                connection_id: None,
                query: "select 1".to_string(),
                duration_ms: 4,
                row_count: Some(1),
                is_error: false,
                error_message: None,
                result_json: Some(serde_json::json!({"rows": [{"?column?": 1}]})),
            })
            .unwrap();

        assert_eq!(storage.history_page(None, 0, 20).unwrap().total, 1);
        assert!(storage.history_result(id).unwrap().is_some());
    }

    #[test]
    fn snapshots_keep_the_inclusive_cap_and_stop_on_escaped_utf8_overflow() {
        // JSON string adds two quotes. The exact cap is retained; one byte more
        // stops encoding without allocating the full result.
        for (length, stored) in [
            (RESULT_JSON_CAP_BYTES - 2, true),
            (RESULT_JSON_CAP_BYTES - 1, false),
        ] {
            let result = "x".repeat(length);
            let snapshot = history_snapshot(&result).unwrap();
            assert_eq!(snapshot.json.is_some(), stored);
            assert_eq!(snapshot.size, stored.then_some((length + 2) as i64));
        }
        let result =
            serde_json::json!({ "rows": [{"value": "žluťoučký\n\t\"\\\u{0000}".repeat(100_000)}] });
        let snapshot = history_snapshot(&result).unwrap();
        assert!(snapshot.json.is_none());
        assert_eq!(snapshot.size, None);
    }

    #[test]
    fn oversized_history_retains_sql_counts_without_a_result_snapshot_or_invented_size() {
        let storage = Storage::in_memory().unwrap();
        let result = serde_json::json!({"rows": [{"body": "x".repeat(RESULT_JSON_CAP_BYTES * 4)}]});
        let id = storage
            .insert_history(NewHistoryEntry {
                connection_id: None,
                query: "select body".into(),
                duration_ms: 42,
                row_count: Some(1),
                is_error: false,
                error_message: None,
                result_json: Some(result),
            })
            .unwrap();
        let page = storage.history_page(None, 0, 20).unwrap();
        let entry = &page.entries[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.query, "select body");
        assert_eq!(entry.duration_ms, 42);
        assert_eq!(entry.row_count, Some(1));
        assert_eq!(entry.result_size, None);
        assert!(!entry.has_result);
        assert!(storage.history_result(id).unwrap().is_none());
    }

    #[test]
    fn borrowed_query_history_round_trips_small_results_without_mutating_them() {
        let storage = Storage::in_memory().unwrap();
        let result: QueryResult = serde_json::from_value(serde_json::json!({
            "columns": [], "rows": [{"payload": "null", "id": "9223372036854775807"}], "rowCount": 1,
            "durationMs": 12, "autoLimited": false, "hasMore": false, "jsonValuesAsText": true
        })).unwrap();
        let original = serde_json::to_value(&result).unwrap();
        let id = storage
            .insert_query_history(None, "select payload".into(), &result)
            .unwrap();
        assert_eq!(storage.history_result(id).unwrap(), Some(original.clone()));
        assert_eq!(serde_json::to_value(&result).unwrap(), original);
        assert_eq!(
            storage.history_page(None, 0, 20).unwrap().entries[0].result_size,
            Some(serde_json::to_vec(&result).unwrap().len() as i64)
        );
    }

    #[test]
    fn large_borrowed_query_history_skips_the_payload_and_keeps_query_metadata() {
        let storage = Storage::in_memory().unwrap();
        let result: QueryResult = serde_json::from_value(serde_json::json!({
            "columns": [], "rows": [{"body": "x".repeat(RESULT_JSON_CAP_BYTES * 4)}], "rowCount": 1,
            "durationMs": 12, "autoLimited": false, "hasMore": false
        }))
        .unwrap();
        let id = storage
            .insert_query_history(None, "select body".into(), &result)
            .unwrap();
        let entry = &storage.history_page(None, 0, 20).unwrap().entries[0];
        assert_eq!(entry.duration_ms, 12);
        assert_eq!(entry.row_count, Some(1));
        assert!(!entry.has_result);
        assert_eq!(entry.result_size, None);
        assert!(storage.history_result(id).unwrap().is_none());
        assert_eq!(
            result.rows[0]["body"].as_str().unwrap().len(),
            RESULT_JSON_CAP_BYTES * 4
        );
    }

    #[test]
    fn payload_preflight_is_a_lower_bound_and_stops_at_the_budget() {
        for value in [
            serde_json::json!({"escaped\"": [null, true, false, 123.456, "ž\n\t\\\u{0000}"]}),
            serde_json::json!("x".repeat(100)),
        ] {
            let exact_size = serde_json::to_vec(&value).unwrap().len();
            let mut exact_budget = exact_size;
            assert!(!payload_exceeds_cap(&value, &mut exact_budget));
            assert!(exact_budget <= exact_size);
        }
        let value = serde_json::json!({"body": "x".repeat(RESULT_JSON_CAP_BYTES + 1)});
        let mut budget = RESULT_JSON_CAP_BYTES;
        assert!(payload_exceeds_cap(&value, &mut budget));
    }

    #[test]
    fn saves_and_activates_connections_without_deadlock() {
        let storage = Storage::in_memory().unwrap();
        let first = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "first".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                username: "postgres".to_string(),
                password: None,
                ssl: false,
            })
            .unwrap();
        let second = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "second".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                username: "postgres".to_string(),
                password: None,
                ssl: false,
            })
            .unwrap();

        assert!(second.is_active);
        assert_eq!(storage.activate_connection(&first.id).unwrap().id, first.id);
        assert_eq!(storage.active_connection().unwrap().unwrap().id, first.id);
    }

    #[test]
    fn copies_connection_fields_and_resolved_password_without_reusing_identity() {
        let source = ConnectionRecord {
            id: "source-id".to_string(),
            name: "zenbase docker".to_string(),
            host: "localhost".to_string(),
            port: 5433,
            database: "zenbase".to_string(),
            username: "zenbase".to_string(),
            password: Some("synthetic-test-secret".to_string()),
            ssl: true,
            is_active: true,
            created_at: "original-created-at".to_string(),
            updated_at: "original-updated-at".to_string(),
        };
        let input = connection_copy_input(&source);
        assert_eq!(input.id, None);
        assert_eq!(input.name, "zenbase docker copy");
        assert_eq!(input.host, source.host);
        assert_eq!(input.port, source.port);
        assert_eq!(input.database, source.database);
        assert_eq!(input.username, source.username);
        assert_eq!(input.password, source.password);
        assert_eq!(input.ssl, source.ssl);
    }

    #[test]
    fn duplicates_get_unique_ids_and_names_without_changing_the_active_connection() {
        let storage = Storage::in_memory().unwrap();
        let source = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "zenbase docker".to_string(),
                host: "localhost".to_string(),
                port: 5433,
                database: "zenbase".to_string(),
                username: "zenbase".to_string(),
                password: None,
                ssl: true,
            })
            .unwrap();
        let active = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "other active connection".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                username: "postgres".to_string(),
                password: None,
                ssl: false,
            })
            .unwrap();
        let first = storage.duplicate_connection(&source.id).unwrap();
        let second = storage.duplicate_connection(&source.id).unwrap();
        assert_ne!(first.id, source.id);
        assert_ne!(first.id, second.id);
        assert_eq!(first.name, "zenbase docker copy");
        assert_eq!(second.name, "zenbase docker copy 2");
        assert!(!first.is_active && !second.is_active);
        assert_eq!(first.host, source.host);
        assert_eq!(first.port, source.port);
        assert_eq!(first.database, source.database);
        assert_eq!(first.username, source.username);
        assert_eq!(first.ssl, source.ssl);
        assert_eq!(storage.active_connection().unwrap().unwrap().id, active.id);
        let original = storage.get_connection(&source.id).unwrap().unwrap();
        assert_eq!(original.name, source.name);
        assert_eq!(original.updated_at, source.updated_at);
        assert!(!original.is_active);
        assert_eq!(storage.list_connections().unwrap().len(), 4);
        assert!(storage.duplicate_connection("missing-id").is_err());
        assert_eq!(storage.list_connections().unwrap().len(), 4);
    }
}
