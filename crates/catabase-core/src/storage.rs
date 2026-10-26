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
use sha2::{Digest, Sha256};
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
}

impl HistorySnapshot {
    fn oversized() -> Self {
        Self { json: None }
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
    pub supermaven_enabled: bool,
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
    pub query_hash: String,
    pub result_hash: Option<String>,
    pub query_size: i64,
    pub execution_size: i64,
    pub query_references: i64,
    pub result_references: i64,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStorageStats {
    pub stored_bytes: i64,
    pub allocated_bytes: i64,
    pub saved_bytes: i64,
    pub query_bytes: i64,
    pub result_bytes: i64,
    pub execution_bytes: i64,
    pub unique_queries: i64,
    pub unique_results: i64,
}

/// Shared snapshots contain stable response data only. Preserve volatile fields
/// on each execution so opening an older result never shows a newer run's time.
struct SharedSnapshot {
    hash: String,
    json: String,
    metadata: Option<String>,
}

fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn shared_snapshot(json: String) -> Result<SharedSnapshot, CoreError> {
    // Canonical object-key ordering makes legacy and newly serialized results
    // identical. Row order, column metadata, exact values and paging flags stay
    // part of the hash. Keep malformed legacy payloads intact, not discarded.
    let (json, metadata) = match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(mut value) => {
            let mut metadata = serde_json::Map::new();
            if let Some(object) = value.as_object_mut() {
                for field in ["durationMs", "timings"] {
                    if let Some(value) = object.remove(field) {
                        metadata.insert(field.into(), value);
                    }
                }
            }
            (
                serde_json::to_string(&value)?,
                (!metadata.is_empty())
                    .then(|| serde_json::to_string(&metadata))
                    .transpose()?,
            )
        }
        Err(_) => (json, None),
    };
    Ok(SharedSnapshot {
        hash: content_hash(json.as_bytes()),
        json,
        metadata,
    })
}

fn store_history_payloads(
    conn: &Connection,
    query: &str,
    snapshot: Option<&SharedSnapshot>,
) -> Result<String, CoreError> {
    // Exact SQL bytes: whitespace/comments/string literals must not be rewritten.
    let query_hash = content_hash(query.as_bytes());
    conn.execute(
        "INSERT OR IGNORE INTO history_queries(hash, query, byte_size) VALUES (?1, ?2, ?3)",
        params![query_hash, query, query.len() as i64],
    )?;
    if let Some(snapshot) = snapshot {
        conn.execute("INSERT OR IGNORE INTO history_results(hash, result_json, byte_size) VALUES (?1, ?2, ?3)", params![snapshot.hash, snapshot.json, snapshot.json.len() as i64])?;
    }
    Ok(query_hash)
}

fn collect_history_payloads(conn: &Connection) -> Result<(), CoreError> {
    conn.execute_batch("DELETE FROM history_results WHERE NOT EXISTS (SELECT 1 FROM query_history WHERE result_hash = history_results.hash);
        DELETE FROM history_queries WHERE NOT EXISTS (SELECT 1 FROM query_history WHERE query_hash = history_queries.hash);")?;
    Ok(())
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
        let supermaven_enabled =
            setting_value(&conn, "supermaven_enabled")?.as_deref() == Some("true");
        Ok(Settings {
            pool_size,
            supermaven_enabled,
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

    pub fn set_supermaven_enabled(&self, enabled: bool) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("INSERT INTO settings (key, value) VALUES ('supermaven_enabled', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![enabled.to_string()])?;
        Ok(())
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

    /// Disconnecting keeps credentials and drafts; it only clears the default.
    pub fn deactivate_connection(&self, id: &str) -> Result<(), CoreError> {
        self.lock()?
            .execute("UPDATE connections SET is_active = 0 WHERE id = ?1", [id])?;
        Ok(())
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
        let snapshot = snapshot
            .and_then(|snapshot| snapshot.json)
            .map(shared_snapshot)
            .transpose()?;
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let query_hash = store_history_payloads(&tx, &entry.query, snapshot.as_ref())?;
        tx.execute(
            "INSERT INTO query_history (
                connection_id, query_hash, duration_ms, row_count, result_hash, is_error,
                error_message, result_metadata, executed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.connection_id,
                query_hash,
                i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
                entry.row_count.map(|value| value as i64),
                snapshot.as_ref().map(|snapshot| &snapshot.hash),
                bool_to_i64(entry.is_error),
                entry.error_message,
                snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.metadata.as_ref()),
                Utc::now().to_rfc3339(),
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
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
            (!trimmed.is_empty()).then(|| {
                format!(
                    "%{}%",
                    trimmed
                        .replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            })
        });

        let total = match &search {
            Some(search) => conn.query_row(
                "SELECT COUNT(*) FROM query_history h JOIN history_queries q ON q.hash = h.query_hash
                 WHERE q.query LIKE ?1 ESCAPE '\\' OR h.error_message LIKE ?1 ESCAPE '\\'",
                params![search],
                |row| row.get(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM query_history", [], |row| row.get(0))?,
        };

        let entries = match &search {
            Some(search) => {
                let mut stmt = conn.prepare(&format!("{} WHERE q.query LIKE ?1 ESCAPE '\\' OR h.error_message LIKE ?1 ESCAPE '\\' ORDER BY h.executed_at DESC, h.id DESC LIMIT ?2 OFFSET ?3", history_select()))?;
                stmt.query_map(params![search, normalized_limit, offset], row_to_history)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "{} ORDER BY h.executed_at DESC, h.id DESC LIMIT ?1 OFFSET ?2",
                    history_select()
                ))?;
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
        .map_err(Into::into)
    }

    /// Disk accounting scans SQLite pages. Only request it when the user opens
    /// storage details, never on the query execution or history search hot path.
    pub fn history_storage_stats(&self) -> Result<HistoryStorageStats, CoreError> {
        let conn = self.lock()?;
        let (query_bytes, unique_queries): (i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(byte_size), 0), COUNT(*) FROM history_queries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let (result_bytes, unique_results): (i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(byte_size), 0), COUNT(*) FROM history_results",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let allocated_bytes = conn.query_row("SELECT COALESCE(SUM(pgsize), 0) FROM dbstat WHERE name IN
            (SELECT name FROM sqlite_schema WHERE tbl_name IN ('query_history', 'history_queries', 'history_results'))", [], |row| row.get(0))?;
        let execution_bytes: i64 = conn.query_row(
            &format!("SELECT COALESCE(SUM({HISTORY_EXECUTION_SIZE}), 0) FROM query_history h"),
            [],
            |row| row.get(0),
        )?;
        let referenced_bytes: i64 = conn.query_row("SELECT COALESCE(SUM(q.byte_size + COALESCE(r.byte_size, 0)), 0)
            FROM query_history h JOIN history_queries q ON q.hash = h.query_hash LEFT JOIN history_results r ON r.hash = h.result_hash", [], |row| row.get(0))?;
        Ok(HistoryStorageStats {
            stored_bytes: query_bytes + result_bytes + execution_bytes,
            allocated_bytes,
            saved_bytes: referenced_bytes - query_bytes - result_bytes,
            query_bytes,
            result_bytes,
            execution_bytes,
            unique_queries,
            unique_results,
        })
    }

    pub fn history_result(&self, id: i64) -> Result<Option<serde_json::Value>, CoreError> {
        let conn = self.lock()?;
        let snapshot: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT r.result_json, h.result_metadata FROM query_history h LEFT JOIN history_results r ON r.hash = h.result_hash WHERE h.id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((Some(json), metadata)) = snapshot else {
            return Ok(None);
        };
        let mut result: serde_json::Value = serde_json::from_str(&json)?;
        if let (Some(object), Some(metadata)) = (result.as_object_mut(), metadata) {
            object.extend(serde_json::from_str::<
                serde_json::Map<String, serde_json::Value>,
            >(&metadata)?);
        }
        Ok(Some(result))
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM query_history WHERE id = ?1", params![id])?;
        collect_history_payloads(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn history_purge(&self) -> Result<(), CoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM query_history", [])?;
        collect_history_payloads(&tx)?;
        tx.commit()?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), CoreError> {
        let mut conn = self.lock()?;
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
             );",
        )?;
        migrate_history(&mut conn)?;
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

// Logical bytes of per-execution values, including hash references, but not
// SQLite record/page overhead. Actual history table/index pages come from dbstat.
const HISTORY_EXECUTION_SIZE: &str = "24 + 1 + length(CAST(h.query_hash AS BLOB)) +
    COALESCE(length(CAST(h.result_hash AS BLOB)), 0) + length(CAST(h.executed_at AS BLOB)) +
    COALESCE(length(CAST(h.connection_id AS BLOB)), 0) + COALESCE(length(CAST(h.error_message AS BLOB)), 0) +
    COALESCE(length(CAST(h.result_metadata AS BLOB)), 0)";

fn history_select() -> String {
    format!("SELECT h.id, h.connection_id, q.query, h.duration_ms, h.row_count, r.byte_size, h.is_error,
        h.error_message, h.result_hash IS NOT NULL, h.executed_at, h.query_hash, h.result_hash,
        q.byte_size, {HISTORY_EXECUTION_SIZE},
        (SELECT COUNT(*) FROM query_history WHERE query_hash = h.query_hash),
        (SELECT COUNT(*) FROM query_history WHERE result_hash = h.result_hash)
        FROM query_history h JOIN history_queries q ON q.hash = h.query_hash
        LEFT JOIN history_results r ON r.hash = h.result_hash")
}

fn migrate_history(conn: &mut Connection) -> Result<(), CoreError> {
    let has_legacy = conn
        .prepare("PRAGMA table_info(query_history)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|column| column == "query");
    let tx = conn.transaction()?;
    let high_water: i64 = if has_legacy {
        let sequence = tx
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'query_history'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        tx.execute_batch("ALTER TABLE query_history RENAME TO query_history_legacy; DROP INDEX IF EXISTS idx_history_executed;")?;
        sequence
    } else {
        0
    };
    tx.execute_batch("CREATE TABLE IF NOT EXISTS history_queries (
            hash TEXT PRIMARY KEY, query TEXT NOT NULL, byte_size INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS history_results (
            hash TEXT PRIMARY KEY, result_json TEXT NOT NULL, byte_size INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS query_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT,
            query_hash TEXT NOT NULL REFERENCES history_queries(hash), duration_ms INTEGER NOT NULL,
            row_count INTEGER, result_hash TEXT REFERENCES history_results(hash),
            is_error INTEGER NOT NULL DEFAULT 0, error_message TEXT, result_metadata TEXT, executed_at TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_history_executed ON query_history(executed_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_history_query_hash ON query_history(query_hash);
        CREATE INDEX IF NOT EXISTS idx_history_result_hash ON query_history(result_hash);")?;
    if has_legacy {
        // Stream old entries within the transaction: bounded memory, all-or-
        // nothing migration, stable IDs/timestamps, no database connections.
        {
            let mut stmt = tx.prepare("SELECT id, connection_id, query, duration_ms, row_count, is_error, error_message, result_json, executed_at FROM query_history_legacy ORDER BY id")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let query: String = row.get(2)?;
                let snapshot = row
                    .get::<_, Option<String>>(7)?
                    .map(shared_snapshot)
                    .transpose()?;
                let hash = store_history_payloads(&tx, &query, snapshot.as_ref())?;
                tx.execute("INSERT INTO query_history(id, connection_id, query_hash, duration_ms, row_count, is_error, error_message, result_hash, result_metadata, executed_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", params![row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, hash,
                    row.get::<_, i64>(3)?, row.get::<_, Option<i64>>(4)?, row.get::<_, i64>(5)?, row.get::<_, Option<String>>(6)?,
                    snapshot.as_ref().map(|s| &s.hash), snapshot.as_ref().and_then(|s| s.metadata.as_ref()), row.get::<_, String>(8)?])?;
            }
        }
        tx.execute_batch("DROP TABLE query_history_legacy;")?;
        tx.execute(
            "UPDATE sqlite_sequence SET seq = MAX(seq, ?1) WHERE name = 'query_history'",
            params![high_water],
        )?;
        tx.execute("INSERT INTO sqlite_sequence(name, seq) SELECT 'query_history', ?1 WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'query_history')", params![high_water])?;
    }
    tx.commit()?;
    Ok(())
}

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
        query_hash: row.get(10)?,
        result_hash: row.get(11)?,
        query_size: row.get(12)?,
        execution_size: row.get(13)?,
        query_references: row.get(14)?,
        result_references: row.get(15)?,
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

    fn history_fixture(query: &str, value: i64, ms: u128) -> NewHistoryEntry {
        NewHistoryEntry {
            connection_id: Some("docker".into()),
            query: query.into(),
            duration_ms: ms,
            row_count: Some(1),
            is_error: false,
            error_message: None,
            result_json: Some(
                serde_json::json!({ "columns": [{"name": "id", "typeName": "int4"}], "rows": [{"id": value}], "rowCount": 1, "durationMs": ms, "timings": {"totalMs": ms}, "hasMore": false }),
            ),
        }
    }

    #[test]
    fn history_shares_sql_and_stable_results_but_keeps_every_execution_and_its_timing() {
        let storage = Storage::in_memory().unwrap();
        let first = history_fixture("select 1", 1, 12);
        let first_json = first.result_json.clone();
        let first_id = storage.insert_history(first).unwrap();
        let second = history_fixture("select 1", 1, 33);
        let second_json = second.result_json.clone();
        let second_id = storage.insert_history(second).unwrap();
        // Identical data can be shared even when a different SQL statement or
        // connection produced it. Their provenance remains on each execution.
        let mut third = history_fixture("select 1 -- same result", 1, 44);
        third.connection_id = Some("another-synthetic-connection".into());
        storage.insert_history(third).unwrap();
        assert_eq!(storage.history_result(first_id).unwrap(), first_json);
        assert_eq!(storage.history_result(second_id).unwrap(), second_json);
        let entries = storage.history_page(None, 0, 20).unwrap();
        assert_eq!(entries.total, 3);
        assert_eq!(entries.entries[0].result_references, 3);
        assert_eq!(entries.entries[1].query_references, 2);
        assert_eq!(entries.entries[1].query_hash, entries.entries[2].query_hash);
        assert_eq!(
            entries.entries[0].result_hash,
            entries.entries[2].result_hash
        );
        let stats = storage.history_storage_stats().unwrap();
        assert_eq!(stats.unique_queries, 2);
        assert_eq!(stats.unique_results, 1);
        assert_eq!(
            stats.saved_bytes,
            "select 1".len() as i64 + 2 * stats.result_bytes
        );
        assert_eq!(
            stats.stored_bytes,
            stats.query_bytes + stats.result_bytes + stats.execution_bytes
        );
        assert!(stats.allocated_bytes >= stats.stored_bytes);
    }

    #[test]
    fn changed_results_never_reuse_old_data_and_shared_blobs_survive_deleting_one_reference() {
        let storage = Storage::in_memory().unwrap();
        let a = storage
            .insert_history(history_fixture("select id", 1, 1))
            .unwrap();
        let b = storage
            .insert_history(history_fixture("select id", 1, 2))
            .unwrap();
        let c = storage
            .insert_history(history_fixture("select id", 2, 3))
            .unwrap();
        assert_eq!(storage.history_storage_stats().unwrap().unique_results, 2);
        storage.history_delete(a).unwrap();
        assert_eq!(
            storage.history_result(b).unwrap().unwrap()["rows"][0]["id"],
            1
        );
        assert_eq!(
            storage.history_result(c).unwrap().unwrap()["rows"][0]["id"],
            2
        );
        storage.history_delete(b).unwrap();
        assert_eq!(storage.history_storage_stats().unwrap().unique_results, 1);
        storage.history_purge().unwrap();
        let stats = storage.history_storage_stats().unwrap();
        assert_eq!(
            (
                stats.unique_queries,
                stats.unique_results,
                stats.stored_bytes,
                stats.saved_bytes
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn result_hash_includes_order_types_precision_and_paging_flags() {
        let base = serde_json::json!({"columns": [{"name": "id", "typeName": "int8"}], "rows": [{"id": "9007199254740993"}, {"id": null}], "hasMore": false});
        let hash = shared_snapshot(base.to_string()).unwrap().hash;
        for pointer in [
            "/columns/0/typeName",
            "/rows/0/id",
            "/rows/1/id",
            "/hasMore",
        ] {
            let mut changed = base.clone();
            *changed.pointer_mut(pointer).unwrap() = serde_json::json!("different");
            assert_ne!(hash, shared_snapshot(changed.to_string()).unwrap().hash);
        }
        let mut reordered = base.clone();
        reordered["rows"].as_array_mut().unwrap().reverse();
        assert_ne!(hash, shared_snapshot(reordered.to_string()).unwrap().hash);
    }

    #[test]
    fn history_search_covers_old_entries_errors_and_literal_sql_wildcards() {
        let storage = Storage::in_memory().unwrap();
        let old = storage
            .insert_history(history_fixture("select '100%_exact'", 1, 1))
            .unwrap();
        for i in 0..60 {
            storage
                .insert_history(history_fixture(&format!("select {i}"), i, 1))
                .unwrap();
        }
        let mut failed = history_fixture("bad sql", 0, 1);
        failed.is_error = true;
        failed.error_message = Some("missing_relation".into());
        failed.result_json = None;
        storage.insert_history(failed).unwrap();
        assert!(
            !storage
                .history_page(None, 0, 50)
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.id == old)
        );
        let found = storage.history_page(Some("%_".into()), 0, 20).unwrap();
        assert_eq!(found.total, 1);
        assert_eq!(found.entries[0].id, old);
        assert_eq!(
            storage
                .history_page(Some("missing_relation".into()), 0, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            storage
                .history_page(Some("not present".into()), 0, 20)
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn legacy_history_migration_is_lossless_idempotent_and_preserves_deleted_id_high_water() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE query_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT, query TEXT NOT NULL, duration_ms INTEGER NOT NULL,
            row_count INTEGER, result_size INTEGER, is_error INTEGER NOT NULL DEFAULT 0, error_message TEXT, result_json TEXT, executed_at TEXT NOT NULL);
            CREATE INDEX idx_history_executed ON query_history(executed_at DESC);").unwrap();
        let mut originals = Vec::new();
        for id in [7, 11] {
            let entry = history_fixture("select 'žluťoučký'", 1, id as u128);
            conn.execute("INSERT INTO query_history VALUES (?1, 'docker', ?2, ?1, 1, 0, 0, NULL, ?3, '2025-01-01T00:00:00Z')", params![id, entry.query, entry.result_json.as_ref().unwrap().to_string()]).unwrap();
            originals.push((id, entry.result_json));
        }
        conn.execute_batch("INSERT INTO query_history(id, query, duration_ms, executed_at) VALUES (90, 'deleted', 0, 'old'); DELETE FROM query_history WHERE id=90;").unwrap();
        drop(conn);
        for _ in 0..2 {
            let storage = Storage::open(&path).unwrap();
            assert_eq!(storage.history_page(None, 0, 10).unwrap().total, 2);
            for (id, result) in &originals {
                assert_eq!(storage.history_result(*id).unwrap(), *result);
            }
            let stats = storage.history_storage_stats().unwrap();
            assert_eq!((stats.unique_queries, stats.unique_results), (1, 1));
            assert!(stats.saved_bytes > 0);
            let conn = storage.lock().unwrap();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
                0
            );
            assert!(
                !conn
                    .prepare("PRAGMA table_info(query_history)")
                    .unwrap()
                    .query_map([], |row| row.get::<_, String>(1))
                    .unwrap()
                    .any(|row| matches!(row.unwrap().as_str(), "query" | "result_json"))
            );
        }
        let storage = Storage::open(&path).unwrap();
        assert!(
            storage
                .insert_history(history_fixture("select new", 1, 1))
                .unwrap()
                > 90
        );
    }

    #[test]
    #[ignore = "requires CATABASE_TEST_HISTORY_COPY pointing at a disposable history-migration-test.sqlite3 backup"]
    fn migrates_real_history_copy_without_losing_any_payloads() {
        let path = std::path::PathBuf::from(std::env::var("CATABASE_TEST_HISTORY_COPY").unwrap());
        assert_eq!(path.file_name().unwrap(), "history-migration-test.sqlite3");
        let old =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let originals = old.prepare("SELECT id, query, result_json, executed_at, duration_ms FROM query_history ORDER BY id").unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, String>(3)?, row.get::<_, i64>(4)?)))
            .unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        drop(old);
        let storage = Storage::open(&path).unwrap();
        assert_eq!(
            storage.history_stats().unwrap().total,
            originals.len() as i64
        );
        for (id, query, json, timestamp, duration) in &originals {
            let result = storage.history_result(*id).unwrap();
            let original = json
                .as_ref()
                .map(|json| serde_json::from_str::<serde_json::Value>(json).unwrap());
            assert!(result == original, "snapshot changed for history #{id}");
            let actual: (String, String, i64) = storage.lock().unwrap().query_row("SELECT q.query, h.executed_at, h.duration_ms FROM query_history h JOIN history_queries q ON q.hash=h.query_hash WHERE h.id=?1", params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert!(
                actual == (query.clone(), timestamp.clone(), *duration),
                "execution changed for history #{id}"
            );
        }
        let stats = storage.history_storage_stats().unwrap();
        println!(
            "Migrated {} executions: {} unique queries, {} unique snapshots, {} duplicate bytes avoided; {} bytes in history pages.",
            originals.len(),
            stats.unique_queries,
            stats.unique_results,
            stats.saved_bytes,
            stats.allocated_bytes
        );
    }

    #[test]
    fn migrates_default_settings() {
        let storage = Storage::in_memory().unwrap();
        let settings = storage.get_settings().unwrap();

        assert_eq!(settings.pool_size, 5);
        assert!(!settings.supermaven_enabled);
    }

    #[test]
    fn normalizes_pool_size() {
        let storage = Storage::in_memory().unwrap();

        assert_eq!(storage.set_pool_size(0).unwrap(), 1);
        assert_eq!(storage.set_pool_size(999).unwrap(), 32);
    }

    #[test]
    fn supermaven_is_opt_in_and_legacy_model_settings_never_enable_it() {
        let storage = Storage::in_memory().unwrap();
        storage
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key,value) VALUES ('ai_model','legacy-model')",
                [],
            )
            .unwrap();
        assert!(!storage.get_settings().unwrap().supermaven_enabled);
        storage.set_supermaven_enabled(true).unwrap();
        assert!(storage.get_settings().unwrap().supermaven_enabled);
        storage.set_supermaven_enabled(false).unwrap();
        assert!(!storage.get_settings().unwrap().supermaven_enabled);
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
            assert_eq!(
                snapshot.json.as_ref().map(String::len),
                stored.then_some(length + 2)
            );
        }
        let result =
            serde_json::json!({ "rows": [{"value": "žluťoučký\n\t\"\\\u{0000}".repeat(100_000)}] });
        let snapshot = history_snapshot(&result).unwrap();
        assert!(snapshot.json.is_none());
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
            Some(
                shared_snapshot(serde_json::to_string(&result).unwrap())
                    .unwrap()
                    .json
                    .len() as i64
            )
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
        storage.save_draft("workspace.v1", "saved SQL").unwrap();
        storage.deactivate_connection(&second.id).unwrap();
        assert_eq!(storage.active_connection().unwrap().unwrap().id, first.id);
        storage.deactivate_connection(&first.id).unwrap();
        assert!(storage.active_connection().unwrap().is_none());
        assert_eq!(storage.list_connections().unwrap().len(), 2);
        assert_eq!(
            storage.load_draft("workspace.v1").unwrap().as_deref(),
            Some("saved SQL")
        );
        assert!(storage.activate_connection(&first.id).unwrap().is_active);
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
